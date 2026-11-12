use std::borrow::Cow;
use std::cmp::Reverse;
use std::ops::Range;
use std::path::Path;
use std::sync::{Arc, OnceLock};

use fst::Map;

use crate::Error;
use crate::alphabet::{Alphabet, Key};
use crate::clitics;
use crate::format::{
    self, HAS_PROBABILITIES, LEMMA_ONLY, Layout, MAX_LIST, MAX_SPELLINGS, PROBABILITY_SCALE, Slice,
    Storage, Strings, UD_BITS, VARIANT, WIDE_SPELLINGS, WordValue, unpack_spelling,
};
use crate::rules::{
    self, CAPS_NAME, CLITICS_NAME, INITIAL_NAME, LOWER_NAME, RESTORED_NAME, ROMAN_LETTER, Rule,
    RuleTags, SHORT_CAPS,
};
use crate::ud::{UdTag, UdTags};

mod annotate;
mod context;
mod guess;

#[cfg(any(test, feature = "build"))]
pub(crate) use context::Model;
#[cfg(any(test, feature = "build"))]
pub(crate) use context::sources as context_sources;

#[doc(hidden)]
pub use annotate::Fields;
#[cfg(feature = "cli")]
pub(crate) use annotate::{one_line, set_fields};

/// Where an analysis comes from
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Source {
    /// The word form is in the dictionary
    Dictionary,
    /// The word form is a verb of the dictionary with enclitic pronouns
    Clitics,
    /// A rule gives the analysis: of a token that is not in the dictionary (punctuation, a symbol,
    /// a number, an ordinal number, a Roman numeral, a name, an address, an abbreviation, a letter)
    /// or the name and the Roman numeral of a word with a capital letter that the dictionary has
    /// (`Estados`, `ETA`, `X`)
    Rule,
    /// A lowercase word that is not in the dictionary, guessed: by its last part after a hyphen,
    /// by the word without a known prefix, as a verb with clitics of a guessed host or by the words
    /// of the dictionary with its ending
    Guess,
}

impl Source {
    /// Name of the source: `dictionary`, `clitics`, `rule` or `guess`
    pub fn as_str(self) -> &'static str {
        match self {
            Source::Dictionary => "dictionary",
            Source::Clitics => "clitics",
            Source::Rule => "rule",
            Source::Guess => "guess",
        }
    }
}

/// Analysis of a word form in the dictionary: a form of an inflection table. The numbers are valid
/// only for the dictionary that gave them: with another one they give other forms
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct Entry {
    /// Number of the table
    pub table: u16,
    /// Number of the form in the paradigm of the table
    pub form: u16,
}

impl Entry {
    /// A form of a table by the numbers that an analysis of the same dictionary had
    pub fn new(table: u16, form: u16) -> Self {
        Entry { table, form }
    }
}

/// Analysis of a word form
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct Analysis {
    /// The word form as the dictionary spells it: with its accents; a verb with clitics as written,
    /// in lower case
    pub word: String,
    /// The lemma
    pub lemma: String,
    /// Number of the UD tag for `Dictionary::ud_tag`
    pub ud: u16,
    /// Share of the analysis among the analyses of the word: by the counts of treebanks, equal for
    /// a word they did not see
    pub prob: f64,
    /// Where the analysis comes from
    pub source: Source,
    /// The form of the inflection table
    pub entry: Option<Entry>,
    /// Words of a token of several: a verb and its clitics (`da`, `me`, `lo`) or a contraction
    /// (`de`, `el`); empty for a token of one word. The analysis describes the first of them
    pub words: Vec<String>,
}

impl Analysis {
    /// An analysis from its fields, as `Dictionary::analyze` gave them: for an analysis kept apart
    /// from the dictionary, whose forms are then asked for. `ud` and `entry` are numbers of the
    /// dictionary that gave the analysis and are valid only for it
    pub fn new(
        word: String,
        lemma: String,
        ud: u16,
        prob: f64,
        source: Source,
        entry: Option<Entry>,
        words: Vec<String>,
    ) -> Self {
        Analysis {
            word,
            lemma,
            ud,
            prob,
            source,
            entry,
            words,
        }
    }

    /// Source of the forms of the lexeme of the analysis: the forms of a guessed word are guessed,
    /// the others are words of the dictionary
    fn forms_source(&self) -> Source {
        match self.source {
            Source::Guess => Source::Guess,
            _ => Source::Dictionary,
        }
    }
}

/// Dictionary of word forms
///
/// The dictionary file is mapped into memory: processes with one dictionary share its memory, and
/// a copy (`clone`) shares the mapped file. While a dictionary is open, its file must not be
/// overwritten in place. A dictionary is trusted data, like code: loading rejects a damaged file,
/// but a deliberately crafted one may make the analysis panic, though memory safety holds;
/// [`Dictionary::verify`] rejects such a file
#[derive(Clone)]
pub struct Dictionary {
    storage: Arc<Storage>,
    layout: Layout,
    meta: String,
    ud: UdTags,
    alphabet: Alphabet,
    suffixes: Strings,
    /// Start of every inflection table in `tables`
    table_offsets: Vec<u32>,
    words: Map<Slice>,
    /// Automaton of the endings that guess unknown words
    guesses: Map<Slice>,
    word_count: usize,
    /// Which hosts of clitics a UD tag gives, by number
    hosts: Vec<Host>,
    /// Whether a UD tag is of a name, by number
    names: Vec<bool>,
    /// Whether a UD tag is of a noun, a name, an adjective, a determiner, a pronoun or a numeral,
    /// by number
    nominal: Vec<bool>,
    /// Numbers of the names `Gender` and `Number` in the strings of the tags
    gender_number: [Option<u16>; 2],
    /// Tags of the analyses by rules; `None` if the table of tags has no room for them
    rule_tags: Option<RuleTags>,
    /// Made on the first annotation and shared by the copies
    feats: Arc<OnceLock<TagFeats>>,
    /// The context model, read on the first analysis of a sentence and shared by the copies
    context: Arc<OnceLock<Result<context::Context, String>>>,
}

/// The features of every UD tag in CoNLL-U, `None` without them
type TagFeats = Box<[Option<Box<str>>]>;

/// Analyses of one spelling of the host of a split
struct HostAnalyses {
    found: Vec<Analysis>,
    /// The analyses whose form the token stresses where its accent is
    stressed: Vec<Analysis>,
    /// Whether the dictionary has the spelling as written
    written: bool,
}

/// Parts of speech whose forms without a gender or a number take any
const NOMINAL: [&str; 6] = ["NOUN", "PROPN", "ADJ", "DET", "PRON", "NUM"];

/// A form that takes clitics
#[derive(Clone, Copy, PartialEq, Eq)]
enum Host {
    No,
    /// An infinitive or a gerund
    NonFinite,
    Imperative,
    /// A finite form of the indicative, the subjunctive or the conditional: it takes enclitics
    /// in older and literary writing (`díjole`, `levantóse`)
    Finite,
}

impl std::fmt::Debug for Dictionary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Dictionary")
            .field("meta", &self.meta)
            .field("word_count", &self.word_count)
            .field("ud_tags", &self.ud.len())
            .finish_non_exhaustive()
    }
}

/// Spellings of one key: places of the accents, the start of the list of analyses and the weight
/// among the spellings
enum Spellings<'a> {
    Single { marks: u64, records: u32 },
    Many(SpellingList<'a>),
}

/// Spellings of a key in the section `variants`: two numbers each or, if wide, four
#[derive(Clone, Copy)]
struct SpellingList<'a> {
    numbers: &'a [u32],
    wide: bool,
}

impl SpellingList<'_> {
    fn len(&self) -> usize {
        self.numbers.len() / if self.wide { 4 } else { 2 }
    }

    fn iter(&self) -> impl Iterator<Item = (u64, u32, u32)> {
        let (narrow, wide) = if self.wide {
            (&[][..], self.numbers.as_chunks::<4>().0)
        } else {
            (self.numbers.as_chunks::<2>().0, &[][..])
        };
        narrow
            .iter()
            .map(|&spelling| unpack_spelling(spelling))
            .chain(wide.iter().map(|&[low, high, offset, weight]| {
                (u64::from(low) | u64::from(high) << 32, offset, weight)
            }))
    }
}

impl Spellings<'_> {
    fn iter(&self) -> impl Iterator<Item = (u64, u32, u32)> {
        let (single, many) = match self {
            Spellings::Single { marks, records } => (Some((*marks, *records, 1)), None),
            Spellings::Many(list) => (None, Some(list.iter())),
        };
        single.into_iter().chain(many.into_iter().flatten())
    }

    /// The spelling of a query: the exact one and, if there is none, all spellings of the key.
    /// Spanish spells its accents: an exact word is not looked for under other accents, as
    /// `publico` is not `público`, while a word absent as written may have lost or moved one
    fn matching(&self, marks: u64) -> impl Iterator<Item = (u64, u32, u32)> {
        let exact = self.iter().any(|(own, ..)| own == marks);
        self.iter().filter(move |&(own, ..)| !exact || own == marks)
    }
}

impl Dictionary {
    /// Reads a dictionary from a file: a regular file is mapped into memory, while a pipe, a device
    /// and a file that cannot be mapped are read
    pub fn open(path: impl AsRef<Path>) -> Result<Self, Error> {
        let file = std::fs::File::open(path)?;
        let metadata = file.metadata()?;
        if metadata.is_file() && metadata.len() > 0 {
            // SAFETY: the mapping is only read; the file must not be overwritten while the dictionary
            // is open (see the documentation of `Dictionary`)
            if let Ok(map) = unsafe { memmap2::Mmap::map(&file) } {
                return Self::from_storage(Storage::Mapped(map), true);
            }
        }
        Self::from_storage(format::read(file)?, true)
    }

    /// Reads a dictionary from the bytes of a dictionary file; as with `open`, the bytes must come
    /// from a trusted source
    pub fn from_bytes(data: &[u8]) -> Result<Self, Error> {
        Self::from_storage(Storage::copy(data), true)
    }

    /// Reads a dictionary from the data of the program (`include_bytes!`): bytes aligned to 8 are
    /// read without a copy
    pub fn from_static(data: &'static [u8]) -> Result<Self, Error> {
        Self::from_storage(Storage::of_static(data), true)
    }

    /// The dictionary of esMorph from the crate `esmorph-dict-es`; its structure is checked on every
    /// call, so it is made once and shared between threads. Its checksum is not: the bytes are part
    /// of the program
    #[cfg(feature = "bundled")]
    #[cfg_attr(docsrs, doc(cfg(feature = "bundled")))]
    pub fn bundled() -> Result<Self, Error> {
        if esmorph_dict_es::DICTIONARY.is_empty() {
            return Err(Error::Format(
                "the crate esmorph-dict-es is built without a dictionary file: build it with \
                 `make dictionary` in the repository of esMorph"
                    .into(),
            ));
        }
        Self::from_storage(Storage::of_static(esmorph_dict_es::DICTIONARY), false)
    }

    /// The dictionary of the bytes of a file; `checked` - the checksum of the file is compared
    fn from_storage(storage: Storage, checked: bool) -> Result<Self, Error> {
        let layout = format::layout(storage.bytes(), checked)?;
        let storage = Arc::new(storage.native(&layout));
        let bytes = |range: &Range<usize>| &storage.bytes()[range.clone()];
        let text = |range: &Range<usize>| {
            std::str::from_utf8(bytes(range))
                .map_err(|_| Error::Format("a string is not in UTF-8".into()))
        };
        let ud_tags: Vec<u16> = bytes(&layout.ud_tags)
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&pair| u16::from_le_bytes(pair))
            .collect();
        let mut dictionary = Dictionary {
            meta: text(&layout.meta)?.to_owned(),
            ud: UdTags::read(&Strings::read(bytes(&layout.ud_strings))?, &ud_tags)?,
            alphabet: Alphabet::new(text(&layout.alphabet)?)?,
            suffixes: Strings::read(bytes(&layout.suffixes))?,
            words: automaton(&storage, &layout.words)?,
            guesses: automaton(&storage, &layout.guesses)?,
            table_offsets: Vec::new(),
            word_count: 0,
            hosts: Vec::new(),
            names: Vec::new(),
            nominal: Vec::new(),
            gender_number: [None; 2],
            rule_tags: None,
            feats: Arc::default(),
            context: Arc::default(),
            layout,
            storage: Arc::clone(&storage),
        };
        dictionary.check_paradigms()?;
        dictionary.table_offsets = dictionary.check_tables()?;
        let forms = dictionary.table_forms();
        dictionary.check_records(&forms)?;
        dictionary.check_guesses(&forms)?;
        drop(forms);
        // The number of keys is in the footer of the automaton, which `verify` checks
        dictionary.word_count = (dictionary.words.len())
            .checked_add(dictionary.check_variants()?)
            .ok_or_else(|| Error::Format("wrong number of word forms".into()))?;
        dictionary.rule_tags = RuleTags::add(&mut dictionary.ud);
        dictionary.names = (dictionary.ud.iter())
            .map(|tag| tag.pos() == "PROPN")
            .collect();
        dictionary.nominal = (dictionary.ud.iter())
            .map(|tag| NOMINAL.contains(&tag.pos()))
            .collect();
        dictionary.gender_number = ["Gender", "Number"].map(|name| dictionary.ud.number(name));
        dictionary.hosts = dictionary
            .ud
            .iter()
            .map(|tag| {
                if !matches!(tag.pos(), "VERB" | "AUX") {
                    Host::No
                } else if matches!(tag.feat("VerbForm"), Some("Inf" | "Ger")) {
                    Host::NonFinite
                } else if tag.feat("Mood") == Some("Imp") {
                    Host::Imperative
                } else if matches!(tag.feat("Mood"), Some("Ind" | "Sub" | "Cnd")) {
                    Host::Finite
                } else {
                    Host::No
                }
            })
            .collect();
        Ok(dictionary)
    }

    /// Description of the source of the dictionary
    pub fn meta(&self) -> &str {
        &self.meta
    }

    /// Checks every node of the automata as a lookup reads it, for a dictionary of a source that is
    /// not trusted: loading checks the checksum and the structure of the tables, while a crafted
    /// automaton with a right checksum loads and makes the analysis panic. Takes tens of
    /// milliseconds. A panic of the walk becomes an error, so it needs the default `panic = "unwind"`;
    /// the panic hook may still print its message
    ///
    /// # Errors
    ///
    /// [`Error::Format`] if an automaton is damaged
    pub fn verify(&self) -> Result<(), Error> {
        for (name, map) in [("word forms", &self.words), ("endings", &self.guesses)] {
            let walked =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| walk(map.as_fst())));
            if !matches!(walked, Ok(true)) {
                return Err(Error::Format(format!("the automaton of {name} is damaged")));
            }
        }
        Ok(())
    }

    /// Checksum of the dictionary file: dictionaries with one checksum are one dictionary, so the
    /// numbers of the tags and the forms of their analyses agree
    pub fn checksum(&self) -> u32 {
        self.layout.checksum
    }

    /// Number of word forms in the dictionary
    pub fn len(&self) -> usize {
        self.word_count
    }

    /// Whether the dictionary has no word forms
    pub fn is_empty(&self) -> bool {
        self.word_count == 0
    }

    /// The UD tag of an analysis
    pub fn ud_tag(&self, number: u16) -> Option<UdTag<'_>> {
        self.ud.get(number)
    }

    /// UD tags of the dictionary
    pub fn ud_tags(&self) -> impl ExactSizeIterator<Item = UdTag<'_>> {
        self.ud.iter()
    }

    /// Analyses of a word form, the most probable first. Case does not matter for finding the word,
    /// though a token with a capital letter may also be a name. A word spelled as in the dictionary
    /// gives the analyses of that spelling; a word absent as written is a verb with clitics
    /// (`dámelo`) or is looked for without its accents and with a lost diaeresis. A token that is
    /// neither gets the analyses of the rules: punctuation, a symbol, a number, an ordinal number
    /// in digits, a Roman numeral, a name with a capital letter, an address, an abbreviation. A
    /// lowercase word that is none of these is guessed: by its last part after a hyphen, by the
    /// word without a known prefix, as a verb with clitics of a guessed host or by the dictionary
    /// words with its ending
    pub fn analyze(&self, word: &str) -> Vec<Analysis> {
        let word = normalized(word);
        let (found, exact) = self.spelled(&word);
        // A word absent as written is a verb with clitics before it is a word with other accents:
        // `verte` is `ver` + `te`, not `verté`; a few words of the dictionary are such verbs
        // mostly (`verse`). Clitics follow a verb: a token without letters has none
        let homonym = exact && clitics::homonym(&word);
        if (!exact || homonym) && word.chars().any(char::is_alphabetic) {
            let mut analyses = self.with_clitics(&word);
            if !analyses.is_empty() {
                // A word the dictionary has is a name as rarely as any other: `Vete`, `Trátase`
                let known = !found.is_empty();
                if homonym {
                    for analysis in &mut analyses {
                        analysis.prob *= clitics::HOMONYM_SHARE;
                    }
                    analyses.extend(found.into_iter().map(|analysis| Analysis {
                        prob: analysis.prob * (1.0 - clitics::HOMONYM_SHARE),
                        ..analysis
                    }));
                    analyses.sort_by(|a, b| b.prob.total_cmp(&a.prob));
                }
                if word.chars().any(char::is_uppercase) {
                    let share = if known {
                        CAPS_NAME[2]
                    } else {
                        self.clitics_name(&word, &analyses)
                    };
                    self.add_rules(&word, &[(Rule::Name, share)], &mut analyses);
                }
                return analyses;
            }
        }
        let mut analyses = found;
        // An abbreviation with a period is the noun or symbol of the dictionary without it: `kg.`
        // is `kg`, `pág.` is `pág`
        if analyses.is_empty()
            && let Some(stem) = word.strip_suffix('.')
            && !stem.is_empty()
            && stem.chars().all(|c| c.is_alphabetic() && !c.is_uppercase())
        {
            let known = self.found(stem);
            // A word of the dictionary with the period of a sentence is no abbreviation: `come.`
            if !known.is_empty()
                && !(known.iter()).any(|analysis| {
                    (self.ud.get(analysis.ud))
                        .is_some_and(|tag| matches!(tag.pos(), "NOUN" | "SYM"))
                })
            {
                return Vec::new();
            }
            analyses = (known.into_iter())
                .filter(|analysis| {
                    (self.ud.get(analysis.ud))
                        .is_some_and(|tag| matches!(tag.pos(), "NOUN" | "SYM"))
                })
                .collect();
            let total: f64 = analyses.iter().map(|analysis| analysis.prob).sum();
            for analysis in &mut analyses {
                analysis.prob /= total;
            }
        }
        if analyses.is_empty() {
            let ruled = self.ruled(&word);
            return if ruled.is_empty() {
                self.guess(&word)
            } else {
                ruled
            };
        }
        if let Some(words) = clitics::contraction(&word) {
            for analysis in analyses.iter_mut().filter(|a| a.lemma == words[0]) {
                analysis.words = words.iter().map(|&word| word.to_owned()).collect();
            }
        }
        if rules::caps(&word) {
            self.add_name(&word, &mut analyses);
        } else if word.len() == 1 && rules::roman(&word) {
            // A Roman numeral of one letter that the dictionary knows (`I`, `X`)
            let [number, name] = ROMAN_LETTER;
            let rules = [(Rule::Roman, number), (Rule::Name, name)];
            self.add_rules(&word, &rules, &mut analyses);
        } else if word.chars().any(char::is_uppercase) {
            self.add_capital_name(&word, &mut analyses);
        } else {
            self.lower_names(&mut analyses);
        }
        for analysis in &mut analyses {
            if analysis.source == Source::Dictionary && self.is_name(analysis) {
                analysis.lemma = name_lemma(&word, &analysis.lemma);
            }
        }
        analyses
    }

    fn is_name(&self, analysis: &Analysis) -> bool {
        self.names
            .get(usize::from(analysis.ud))
            .copied()
            .unwrap_or(false)
    }

    /// Share of a name among the analyses of a token with a capital letter that splits as a verb
    /// with clitics: an imperative without an accent mostly is a name (`Mandela`, `Portela`), an
    /// infinitive, a gerund, an imperative with an accent and a finite form mostly are verbs
    /// (`Hacerlo`, `Déjame`, `Levantóse`)
    fn clitics_name(&self, token: &str, analyses: &[Analysis]) -> f64 {
        let host = analyses
            .first()
            .and_then(|analysis| self.hosts.get(usize::from(analysis.ud)));
        let accented =
            (token.chars().flat_map(char::to_lowercase)).any(|c| clitics::without_acute(c) != c);
        match host {
            Some(Host::Imperative) if !accented => CLITICS_NAME[0],
            _ => CLITICS_NAME[1],
        }
    }

    /// Adds the analysis of a name to the analyses of a word with a capital letter that the
    /// dictionary knows without a name of its spelling: an initial (`D.`) and a word without accents
    /// that the dictionary has only with them (`Angel`, `Perez`) are mostly names, another word only
    /// sometimes (`Estados`, `Comisión`), so its name usually goes after the analyses of the
    /// dictionary
    fn add_capital_name(&self, token: &str, analyses: &mut Vec<Analysis>) {
        let written = |analysis: &Analysis| lower_eq(token, &analysis.word);
        if (analyses.iter()).any(|analysis| written(analysis) && self.is_name(analysis)) {
            return;
        }
        let restored = !(token.chars().flat_map(char::to_lowercase))
            .any(|c| c != clitics::without_acute(c) || c == 'ü')
            && !analyses.iter().any(written);
        let mut letters = token.chars();
        let initial = letters.next().is_some_and(char::is_uppercase) && letters.as_str() == ".";
        let share = if initial {
            INITIAL_NAME
        } else if restored {
            RESTORED_NAME
        } else {
            CAPS_NAME[2]
        };
        self.add_rules(token, &[(Rule::Name, share)], analyses);
    }

    /// Lowers the names among the analyses of a lowercase token: the counts of the dictionary do
    /// not tell `Miró` from `miró`, while a lowercase token is a name rarely
    fn lower_names(&self, analyses: &mut [Analysis]) {
        if !analyses.iter().any(|analysis| self.is_name(analysis)) {
            return;
        }
        for analysis in analyses.iter_mut() {
            if self.is_name(analysis) {
                analysis.prob *= LOWER_NAME;
            }
        }
        let total: f64 = analyses.iter().map(|analysis| analysis.prob).sum();
        if total > 0.0 {
            for analysis in analyses.iter_mut() {
                analysis.prob /= total;
            }
        }
        analyses.sort_by(|a, b| b.prob.total_cmp(&a.prob));
    }

    /// Analyses of a token by the rules
    fn ruled(&self, token: &str) -> Vec<Analysis> {
        let Some(tags) = &self.rule_tags else {
            return Vec::new();
        };
        rules::rules(token)
            .into_iter()
            .map(|(lemma, rule, share)| Analysis {
                word: token.to_owned(),
                lemma,
                ud: tags.number(rule),
                prob: share,
                source: Source::Rule,
                entry: None,
                words: Vec::new(),
            })
            .collect()
    }

    /// Adds the analysis of a name to the analyses of a word in capitals that has none, by its
    /// share: a short word is mostly an acronym, so its name usually goes first, while a longer
    /// word and a word of a closed class are mostly words of a headline, so their name usually
    /// goes after the analyses of the dictionary. A Roman numeral in `I` of an open class gets the
    /// analyses of a numeral instead: `VI`, `XI`
    fn add_name(&self, token: &str, analyses: &mut Vec<Analysis>) {
        let pos = |analysis: &Analysis| self.ud.get(analysis.ud).map(|tag| tag.pos());
        if analyses
            .iter()
            .any(|analysis| pos(analysis) == Some("PROPN"))
        {
            return;
        }
        let closed = analyses.iter().any(|analysis| {
            matches!(
                pos(analysis),
                Some("ADP" | "DET" | "CCONJ" | "SCONJ" | "PRON" | "AUX")
            )
        });
        let letters = token.chars().filter(|c| c.is_alphabetic()).count();
        let share = CAPS_NAME[if closed {
            2
        } else {
            usize::from(letters > SHORT_CAPS)
        }];
        if !closed && token.ends_with('I') && rules::roman(token) {
            let (number, name) = rules::roman_shares(token);
            let rules = [(Rule::Roman, share * number), (Rule::Name, share * name)];
            self.add_rules(token, &rules, analyses);
        } else {
            self.add_rules(token, &[(Rule::Name, share)], analyses);
        }
    }

    /// Adds analyses of rules with their shares to the analyses of a token, which keep the rest;
    /// the order stays by the probability
    fn add_rules(&self, token: &str, rules: &[(Rule, f64)], analyses: &mut Vec<Analysis>) {
        let Some(tags) = &self.rule_tags else {
            return;
        };
        let taken: f64 = rules.iter().map(|&(_, share)| share).sum();
        for analysis in analyses.iter_mut() {
            analysis.prob *= 1.0 - taken;
        }
        for &(rule, share) in rules {
            let at = analyses
                .iter()
                .position(|analysis| analysis.prob < share)
                .unwrap_or(analyses.len());
            analyses.insert(
                at,
                Analysis {
                    word: token.to_owned(),
                    lemma: token.to_owned(),
                    ud: tags.number(rule),
                    prob: share,
                    source: Source::Rule,
                    entry: None,
                    words: Vec::new(),
                },
            );
        }
    }

    /// Analyses of the tokens of a sentence, the most probable in the context first: the analyses
    /// of `analyze` ordered by the context model of the dictionary, the probability is its score.
    /// The tokens are those of one sentence in order, as `analyze` takes them (`del`, `dámelo`);
    /// a sentence of one token is analyzed as `analyze`, one longer than `SENTENCE_CHUNK` tokens
    /// by parts of `SENTENCE_CHUNK`. The model is read on the first call
    ///
    /// ```
    /// # #[cfg(feature = "bundled")] {
    /// let dictionary = esmorph::Dictionary::bundled()?;
    /// let sentence = dictionary.analyze_sentence(&["Ella", "la", "vio"])?;
    /// let pos = |analysis: &esmorph::Analysis| dictionary.ud_tag(analysis.ud).unwrap().pos();
    /// assert_eq!(pos(&sentence[1][0]), "PRON");
    /// # }
    /// # Ok::<(), esmorph::Error>(())
    /// ```
    ///
    /// # Errors
    ///
    /// [`Error::Context`] if the dictionary has no context model, [`Error::Format`] if its model
    /// is damaged
    pub fn analyze_sentence<S: AsRef<str>>(
        &self,
        words: &[S],
    ) -> Result<Vec<Vec<Analysis>>, Error> {
        let context = self.context()?;
        let mut sentence = Vec::with_capacity(words.len());
        for (number, chunk) in words.chunks(SENTENCE_CHUNK).enumerate() {
            let analyses: Vec<Vec<Analysis>> = chunk
                .iter()
                .map(|word| self.analyze(word.as_ref()))
                .collect();
            // Without neighbors the model chooses worse than the counts of the dictionary
            if chunk.len() < 2 {
                sentence.extend(analyses);
                continue;
            }
            // The model sees a token as `analyze` reads it: without invisible characters and with
            // combined accents
            let normal: Vec<Cow<str>> =
                chunk.iter().map(|word| normalized(word.as_ref())).collect();
            let normal: Vec<&str> = normal.iter().map(AsRef::as_ref).collect();
            sentence.extend(context.rank(&normal, analyses, number == 0));
        }
        Ok(sentence)
    }

    /// Whether the dictionary has a context model for `analyze_sentence`
    pub fn has_context(&self) -> bool {
        !self.layout.context.is_empty()
    }

    /// The context model of the dictionary: read once for the dictionary and its copies
    fn context(&self) -> Result<&context::Context, Error> {
        // The model is built outside the cell: a fork from another thread while the cell is busy
        // would leave the child waiting for it forever. Two threads may build it at once, one goes
        // to the cell
        let context = match self.context.get() {
            Some(context) => context,
            None => {
                let bytes = &self.storage.bytes()[self.layout.context.clone()];
                let built =
                    if bytes.is_empty() {
                        Err("the dictionary has no context model: a sentence cannot be analyzed in \
                         context"
                        .into())
                    } else {
                        context::Model::parse(bytes)
                            .map(|model| context::Context::new(model, &self.ud))
                            .map_err(|err| err.to_string())
                    };
                self.context.get_or_init(move || built)
            }
        };
        context.as_ref().map_err(|message| {
            if self.has_context() {
                Error::Format(message.clone())
            } else {
                Error::Context(message.clone())
            }
        })
    }

    /// Tags of the parts of speech and features of the dictionary that the dictionary of the
    /// training of its context model had not: with them the model must be trained again
    #[cfg(feature = "build")]
    #[doc(hidden)]
    pub fn context_gaps(&self) -> Result<Vec<String>, Error> {
        Ok(self.context()?.gaps(&self.ud))
    }

    /// Whether the dictionary has the word form, as written or as a verb of the dictionary with
    /// clitics; the analyses of the rules and the guesses do not count
    pub fn is_known(&self, word: &str) -> bool {
        let word = normalized(word);
        self.lookup(&word).is_some() || !self.with_clitics(&word).is_empty()
    }

    /// Analyses of a verb with clitics: of the first split into a host and clitics, the most
    /// clitics first, whose host is an infinitive, a gerund or an imperative of the dictionary. A
    /// finite form is a host only of a token that has no other one and stresses it where its accent
    /// is: enclitics after it belong to older and literary writing (`díjole`, `levantóse`)
    fn with_clitics(&self, word: &str) -> Vec<Analysis> {
        let token = word.to_lowercase();
        // The accent that clitics put on a host marks its stressed vowel: in `piénselo` the host
        // is `piense`, not `píen`. Without an accent, a host as written goes before one with an
        // accent restored: `alientelo` is `aliente` + `lo`, not `alíen` + `te` + `lo`
        let accent = token.chars().position(|c| "áéíóú".contains(c));
        let splits = clitics::splits(&token);
        let mut fallback = None;
        for split in &splits {
            for host in self.host_analyses(split, accent, false) {
                if accent.is_some() && !host.stressed.is_empty() {
                    return with_clitics(&token, split, host.stressed);
                }
                if accent.is_none() && host.written {
                    return with_clitics(&token, split, host.found);
                }
                if fallback.is_none() {
                    fallback = Some((split, host.found));
                }
            }
        }
        if let Some((split, hosts)) = fallback {
            return with_clitics(&token, split, hosts);
        }
        if accent.is_some() {
            for split in &splits {
                for host in self.host_analyses(split, accent, true) {
                    if !host.stressed.is_empty() {
                        return with_clitics(&token, split, host.stressed);
                    }
                }
            }
        }
        Vec::new()
    }

    /// Analyses of the host of a split by the spellings of the host that are hosts of the clitics:
    /// those the token stresses on the vowel `accent`, and whether the dictionary has the spelling
    /// as written; finite forms only if `finite` is asked for
    fn host_analyses(
        &self,
        split: &clitics::Split,
        accent: Option<usize>,
        finite: bool,
    ) -> Vec<HostAnalyses> {
        let mut result = Vec::new();
        for host in clitics::hosts(split.host, split.clitics[0]) {
            let found: Vec<Analysis> = (self.found(&host).into_iter())
                .filter(|analysis| self.takes(analysis, split, finite))
                .collect();
            if found.is_empty() {
                continue;
            }
            let stressed = match accent {
                Some(at) => (found.iter())
                    .filter(|analysis| clitics::stressed(&analysis.word) == Some(at))
                    .cloned()
                    .collect(),
                None => Vec::new(),
            };
            result.push(HostAnalyses {
                written: found.iter().any(|analysis| analysis.word == host),
                stressed,
                found,
            });
        }
        result
    }

    /// Whether the form of an analysis takes the clitics of a split
    fn takes(&self, analysis: &Analysis, split: &clitics::Split, finite: bool) -> bool {
        let first = split.clitics[0];
        match self.hosts.get(usize::from(analysis.ud)) {
            Some(Host::NonFinite) => true,
            Some(Host::Imperative) => {
                let feat = |name| self.ud.get(analysis.ud).and_then(|tag| tag.feat(name));
                if first == "os" {
                    // `os` follows only the imperative of `vosotros`, which loses its `d`:
                    // `cuéntenos` is `cuente` + `nos`, `distribuídos` is no imperative at all
                    analysis.word.ends_with('d') && !split.host.ends_with('d')
                } else if first == "te" {
                    // `te` follows the imperative of `tú`: `siéntate` is of `sentar`, not `sentir`
                    feat("Person") == Some("2") && feat("Number") == Some("Sing")
                } else if split.clitics == ["se"] {
                    feat("Person") != Some("2")
                } else {
                    true
                }
            }
            Some(Host::Finite) => finite,
            _ => false,
        }
    }

    /// Analyses of a word form in the dictionary
    fn found(&self, word: &str) -> Vec<Analysis> {
        self.spelled(word).0
    }

    /// Analyses of a word form in the dictionary and whether the dictionary has it as written
    fn spelled(&self, word: &str) -> (Vec<Analysis>, bool) {
        let Some((key, spellings, as_written)) = self.lookup(word) else {
            return (Vec::new(), false);
        };
        let exact = as_written && spellings.iter().any(|(marks, ..)| marks == key.marks);
        let (count, total) = spellings
            .matching(key.marks)
            .fold((0, 0.0), |(count, total), (.., weight)| {
                (count + 1, total + f64::from(weight))
            });
        let mut analyses = Vec::with_capacity(4);
        for (marks, records, weight) in spellings.matching(key.marks) {
            let mut spelling = self.alphabet.spelling(&key, marks);
            // The exact spelling takes the whole query, as `weight / total` is one
            let weight = f64::from(weight);
            let (records, probabilities) = analyses_at(self.records(), records).unwrap_or_default();
            for (at, &record) in records.iter().enumerate() {
                let entry = Entry {
                    table: (record >> 16) as u16,
                    form: record as u16,
                };
                // Without counts, every analysis of the key has exactly the same share: the sort
                // below then keeps the order of the spellings
                let share = match probabilities.get(at) {
                    Some(&millionths) => {
                        f64::from(millionths) / f64::from(PROBABILITY_SCALE) * (weight / total)
                    }
                    None => weight / records.len() as f64 / total,
                };
                // The last analysis of a spelling takes its string
                let word = if at + 1 == records.len() {
                    std::mem::take(&mut spelling)
                } else {
                    spelling.clone()
                };
                if let Some(analysis) = self.analysis(word, entry, share) {
                    analyses.push(analysis);
                }
            }
        }
        // Shares are stored in millionths: their sum may miss 1 by a few of them
        let sum: f64 = analyses.iter().map(|analysis| analysis.prob).sum();
        if sum > 0.0 {
            for analysis in &mut analyses {
                analysis.prob /= sum;
            }
        }
        if count > 1 {
            analyses.sort_by(|a, b| b.prob.total_cmp(&a.prob));
        }
        (analyses, exact)
    }

    /// The key of a word, its spellings and whether the key is as written: the key as written,
    /// then with a lost diaeresis restored
    fn lookup(&self, word: &str) -> Option<(Key, Spellings<'_>, bool)> {
        let key = self.alphabet.input_key(word)?;
        if let Some(spellings) = self.spellings(&key) {
            return Some((key, spellings, true));
        }
        self.alphabet
            .diaeresis_keys(&key)
            .into_iter()
            .find_map(|key| {
                let spellings = self.spellings(&key)?;
                Some((key, spellings, false))
            })
    }

    fn spellings(&self, key: &Key) -> Option<Spellings<'_>> {
        Some(match WordValue::unpack(self.words.get(key.bytes())?) {
            WordValue::Single { marks, records } => Spellings::Single { marks, records },
            WordValue::Many(offset) => {
                let lists = self.variants().get(offset as usize..)?;
                Spellings::Many(spelling_list(lists)?.0)
            }
        })
    }

    /// Analysis of a spelling by a form of a table
    fn analysis(&self, word: String, entry: Entry, prob: f64) -> Option<Analysis> {
        let lexeme = self.lexeme(entry, &word)?;
        let (lemma, ud) = (
            lexeme.spell(&self.suffixes, 0)?,
            lexeme.ud(usize::from(entry.form))?,
        );
        Some(Analysis {
            word,
            lemma,
            ud,
            prob,
            source: Source::Dictionary,
            entry: Some(entry),
            words: Vec::new(),
        })
    }

    /// Forms of the lexeme of an analysis in the order of its paradigm: the lemma, the main forms by
    /// their features, then the variants; the probability of each is one. A verb with clitics
    /// gives the forms of its verb, without clitics; an analysis without a table, of a rule, is
    /// its only form
    pub fn forms(&self, analysis: &Analysis) -> Vec<Analysis> {
        if analysis.entry.is_none() {
            return vec![Analysis {
                prob: 1.0,
                ..analysis.clone()
            }];
        }
        let Some(lexeme) = self.lexeme_of(analysis) else {
            return Vec::new();
        };
        (0..lexeme.cells.len())
            .filter(|&form| lexeme.cells[form] & LEMMA_ONLY == 0)
            .filter_map(|form| self.form(&lexeme, form, analysis.forms_source()))
            .map(|form| with_lemma(form, &analysis.lemma))
            .collect()
    }

    /// The form of the lexeme of an analysis with the features `feats` ("name", "value"): of the
    /// forms that have them, the one that keeps most of the other features of the analysis, then
    /// changes fewest of them (`chulitos` in the singular is `chulito`, not `chulita`) and, for a
    /// noun, a name, an adjective, a determiner, a pronoun or a numeral, has fewest features more
    /// and lacks no gender or number of the analysis (`lo` in the dative is `le`, not `se`), then a
    /// variant only for a variant, unless a main form of such a word is closer in spelling, then a
    /// form of the tag of the analysis, then the first in the paradigm; of the forms with equal
    /// features, the closest in spelling to the analysis if it has such forms too, as `hablase` and
    /// `hablara`. An infinitive and a gerund asked for features they have not become finite forms:
    /// `hablar` with `Tense=Past` is `hablé`. A form of such a part of speech without a gender or a
    /// number takes any, though a form that has it goes first: `grande` with `Gender=Fem` is
    /// `grande`. A value of a form may list several (`Case=Acc,Nom`): any of them matches. A verb
    /// with clitics inflects as its verb, without clitics; an analysis of a rule gives itself if it
    /// has the features or, as a nominal, lacks the gender or number
    pub fn inflect(&self, analysis: &Analysis, feats: &[(&str, &str)]) -> Option<Analysis> {
        if analysis.entry.is_none() {
            let tag = self.ud.get(analysis.ud)?;
            let nominal = NOMINAL.contains(&tag.pos());
            return (feats.iter())
                .all(|&(name, value)| {
                    has(tag.feat(name), value)
                        || (nominal
                            && matches!(name, "Gender" | "Number")
                            && tag.feat(name).is_none())
                })
                .then(|| Analysis {
                    prob: 1.0,
                    ..analysis.clone()
                });
        }
        let lexeme = self.lexeme_of(analysis)?;
        let own_form = usize::from(analysis.entry?.form);
        let own = *lexeme.cells.get(own_form)?;
        let tag = self.ud.get(analysis.ud)?;
        // Names and values as numbers of the strings of the tags, compared as numbers; a name
        // without a number is in no tag
        let feats: Vec<(Option<u16>, Value)> = (feats.iter())
            .map(|&(name, text)| (self.ud.number(name), self.value(text)))
            .collect();
        let mut wanted: Vec<(Option<u16>, Value)> = (tag.feat_pairs())
            .map(|(name, number)| {
                let text = tag.text(number);
                (
                    Some(name),
                    Value {
                        number: Some(number),
                        text,
                    },
                )
            })
            .collect();
        let mut want = |name, value| match wanted.iter_mut().find(|(known, _)| *known == name) {
            Some(feat) => feat.1 = value,
            None => wanted.push((name, value)),
        };
        let verb_form = self.ud.number("VerbForm");
        if matches!(tag.feat("VerbForm"), Some("Inf" | "Ger"))
            && feats.iter().all(|&(name, _)| name != verb_form)
            && feats
                .iter()
                .any(|&(name, value)| !has_value(tag, name, value))
        {
            want(verb_form, self.value("Fin"));
        }
        for &(name, value) in &feats {
            want(name, value);
        }
        // Of the forms of one tag the closest in spelling to the analysis, if its own tag has
        // several: `hablase` with `Person=3` is `hablase`, `elegido` with `Gender=Fem` is
        // `elegida`, not `electa`; of equally close ones the analysis itself: `paga` stays `paga`,
        // not `pagada`, and `ha` stays `ha`, not `hay`
        let several = lexeme.cells.iter().filter(|&&cell| cell == own).count() > 1;
        let own_word = lexeme.spell(&self.suffixes, own_form)?;
        let common_prefix = |form: usize| {
            let word = lexeme.spell(&self.suffixes, form).unwrap_or_default();
            let common = |same: fn(char, char) -> bool| {
                (own_word.chars().zip(word.chars()))
                    .take_while(|&(a, b)| same(a, b))
                    .count()
            };
            let plain = common(|a, b| clitics::without_acute(a) == clitics::without_acute(b));
            (plain, common(|a, b| a == b))
        };
        let closeness = |form: usize| if several { common_prefix(form) } else { (0, 0) };
        // Forms of one tag are one group in the place of its first form
        let group = |form: usize| {
            (lexeme.cells.iter())
                .position(|&cell| cell == lexeme.cells[form])
                .unwrap_or(form)
        };
        let own_tag = tag;
        let either = |name: Option<u16>| name.is_some() && self.gender_number.contains(&name);
        let lacks = |tag: UdTag<'_>, name: Option<u16>| {
            name.and_then(|name| tag.feat_value(name)).is_none()
        };
        let candidates = (0..lexeme.cells.len())
            .filter(|&form| lexeme.cells[form] & LEMMA_ONLY == 0)
            .filter_map(|form| {
                let number = (lexeme.cells[form] & UD_BITS) as u16;
                let tag = self.ud.get(number)?;
                let nominal = self
                    .nominal
                    .get(usize::from(number))
                    .copied()
                    .unwrap_or(false);
                let fits = feats.iter().all(|&(name, value)| {
                    has_value(tag, name, value) || (nominal && either(name) && lacks(tag, name))
                });
                if !fits {
                    return None;
                }
                // A form with another value of a kept feature is worse than one without it
                // (`chulitos` in the singular is `chulito`, not `chulita`); of equally fitting
                // forms the one with the gender and number that the analysis has goes first:
                // `lo` in the dative is `le`, not `se`, while `ustedes` in the feminine stays
                // `ustedes`
                let (mut contradicted, mut missed, mut absent) = (0, 0, 0);
                for &(name, value) in &wanted {
                    match name.and_then(|name| tag.feat_value(name)) {
                        Some(_) if has_value(tag, name, value) => {}
                        // A value of several parts shares one: `Acc` of `Acc,Nom`
                        Some(known) if shares_part(tag.text(known), value.text) => missed += 1,
                        Some(_) => {
                            contradicted += 1;
                            missed += 1;
                        }
                        None => {
                            missed += 1;
                            if nominal && either(name) && !lacks(own_tag, name) {
                                absent += 1;
                            }
                        }
                    }
                }
                // A pronoun with a feature more is another word: `vos` is not `usted`
                let extra = (tag.feat_names())
                    .filter(|&name| nominal && !either(Some(name)))
                    .filter(|&name| wanted.iter().all(|&(known, _)| known != Some(name)))
                    .count();
                let variant = (lexeme.cells[form] ^ own) & VARIANT != 0;
                let key = (
                    missed,
                    contradicted,
                    extra,
                    absent,
                    variant,
                    lexeme.cells[form] != own,
                    group(form),
                    Reverse(closeness(form)),
                    form != own_form,
                    form,
                );
                Some((key, form))
            });
        // The best form and the best of the other kind, a main form for a variant
        let (mut best, mut other) = (None, None);
        for candidate in candidates {
            if candidate.0.4 && other.is_none_or(|known| candidate < known) {
                other = Some(candidate);
            }
            if best.is_none_or(|known| candidate < known) {
                best = Some(candidate);
            }
        }
        let (key, mut best) = best?;
        // A nominal variant takes a main form closer in spelling than its variants: `buen` in the
        // plural is `buenos`, not the older `bones`; a verb keeps its variety: `sigás` gives `seguí`
        if let Some((main_key, main)) = other
            && own & VARIANT != 0
            && NOMINAL.contains(&tag.pos())
            && !key.4
            && (main_key.0, main_key.1, main_key.2, main_key.3) == (key.0, key.1, key.2, key.3)
            && common_prefix(main) > common_prefix(best)
        {
            best = main;
        }
        let form = self.form(&lexeme, best, analysis.forms_source())?;
        Some(with_lemma(form, &analysis.lemma))
    }

    /// A value of a feature with the number of its string
    fn value<'t>(&self, text: &'t str) -> Value<'t> {
        Value {
            number: self.ud.number(text),
            text,
        }
    }

    /// Lexeme of an analysis: of its verb for a verb with clitics
    fn lexeme_of<'a>(&'a self, analysis: &'a Analysis) -> Option<Lexeme<'a>> {
        let word = match analysis.source {
            Source::Clitics => analysis.words.first()?,
            Source::Guess if !analysis.words.is_empty() => &analysis.words[0],
            _ => &analysis.word,
        };
        self.lexeme(analysis.entry?, word)
    }

    /// Lexeme of the word form `word`, the form `entry` of a table
    fn lexeme<'a>(&'a self, entry: Entry, word: &'a str) -> Option<Lexeme<'a>> {
        let start = *self.table_offsets.get(usize::from(entry.table))? as usize;
        let tables = self.tables();
        let paradigm = self.paradigm(*tables.get(start)?)?;
        let cells = tables.get(start + 1..start + 1 + paradigm.len())?;
        let suffix = self
            .suffixes
            .get(*paradigm.get(usize::from(entry.form))? as usize)?;
        Some(Lexeme {
            table: entry.table,
            stem: word.strip_suffix(suffix)?,
            paradigm,
            cells,
        })
    }

    /// Analysis of a form of a lexeme
    fn form(&self, lexeme: &Lexeme, form: usize, source: Source) -> Option<Analysis> {
        let word = lexeme.spell(&self.suffixes, form)?;
        let lemma = lexeme.spell(&self.suffixes, 0)?;
        let words = match clitics::contraction(&word) {
            Some(words) if words[0] == lemma => words.iter().map(|&word| word.to_owned()).collect(),
            _ => Vec::new(),
        };
        Some(Analysis {
            word,
            lemma,
            ud: lexeme.ud(form)?,
            prob: 1.0,
            source,
            entry: Some(Entry {
                table: lexeme.table,
                form: form as u16,
            }),
            words,
        })
    }

    fn paradigm(&self, number: u32) -> Option<&[u32]> {
        let offsets = self.paradigm_offsets();
        let at = number as usize;
        let (start, end) = (*offsets.get(at)? as usize, *offsets.get(at + 1)? as usize);
        self.paradigms().get(start..end)
    }

    fn numbers<T: bytemuck::Pod>(&self, range: &Range<usize>) -> &[T] {
        format::numbers(&self.storage.bytes()[range.clone()])
    }

    fn paradigm_offsets(&self) -> &[u32] {
        self.numbers(&self.layout.paradigm_offsets)
    }

    fn paradigms(&self) -> &[u32] {
        self.numbers(&self.layout.paradigms)
    }

    fn tables(&self) -> &[u32] {
        self.numbers(&self.layout.tables)
    }

    fn records(&self) -> &[u32] {
        self.numbers(&self.layout.records)
    }

    fn variants(&self) -> &[u32] {
        self.numbers(&self.layout.variants)
    }

    fn guess_records(&self) -> &[u32] {
        self.numbers(&self.layout.guess_records)
    }

    /// Paradigms refer only to existing suffixes
    fn check_paradigms(&self) -> Result<(), Error> {
        let (offsets, paradigms) = (self.paradigm_offsets(), self.paradigms());
        let invalid = |msg: &str| Err(Error::Format(msg.into()));
        if offsets.first() != Some(&0)
            || offsets.last().map(|&end| end as usize) != Some(paradigms.len())
        {
            return invalid("the bounds of paradigms do not match the table of paradigms");
        }
        if offsets.windows(2).any(|bounds| bounds[1] <= bounds[0]) {
            return invalid("a paradigm of a wrong length");
        }
        if paradigms
            .iter()
            .any(|&suffix| suffix as usize >= self.suffixes.len())
        {
            return invalid("a paradigm refers outside the table of suffixes");
        }
        Ok(())
    }

    /// Inflection tables refer to existing paradigms, forms and UD tags; returns the starts of the tables
    fn check_tables(&self) -> Result<Vec<u32>, Error> {
        let invalid = |msg: &str| Err(Error::Format(msg.into()));
        let (tables, offsets) = (self.tables(), self.paradigm_offsets());
        let mut starts = Vec::new();
        let mut start = 0;
        while start < tables.len() {
            if starts.len() > usize::from(u16::MAX) {
                return invalid("more than 65 536 inflection tables");
            }
            let paradigm = tables[start] as usize;
            let Some(&[begin, end]) = paradigm
                .checked_add(2)
                .and_then(|stop| offsets.get(paradigm..stop))
            else {
                return invalid("a table refers to a missing paradigm");
            };
            let count = (end - begin) as usize;
            let Some(cells) = tables.get(start + 1..start + 1 + count) else {
                return invalid("an inflection table is truncated");
            };
            if cells.iter().any(|&cell| {
                cell & !(UD_BITS | VARIANT | LEMMA_ONLY) != 0
                    || (cell & UD_BITS) as usize >= self.ud.len()
            }) {
                return invalid("a table refers to a missing UD tag or has unknown flags");
            }
            starts.push(start as u32);
            start += 1 + count;
        }
        Ok(starts)
    }

    /// The flags and UD tags of the forms of every table: tables and paradigms are checked already
    fn table_forms(&self) -> Vec<&[u32]> {
        let (tables, offsets) = (self.tables(), self.paradigm_offsets());
        (self.table_offsets.iter())
            .map(|&start| {
                let start = start as usize;
                let paradigm = tables[start] as usize;
                let count = (offsets[paradigm + 1] - offsets[paradigm]) as usize;
                &tables[start + 1..start + 1 + count]
            })
            .collect()
    }

    /// Lists of analyses exactly cover their section, are not longer than `MAX_LIST`, their tables
    /// and forms exist and are word forms, not only lemmas, and their probabilities are no more
    /// than one
    fn check_records(&self, table_forms: &[&[u32]]) -> Result<(), Error> {
        let lists = self.records();
        let invalid = |msg: &str| Err(Error::Format(msg.into()));
        let mut start = 0;
        while start < lists.len() {
            let head = lists[start];
            let count = (head & !HAS_PROBABILITIES) as usize;
            if count > MAX_LIST {
                return invalid("a list is longer than 255 items");
            }
            let items_end = start + 1 + count;
            let end = items_end
                + if head & HAS_PROBABILITIES != 0 {
                    count
                } else {
                    0
                };
            let (Some(items), Some(probabilities)) =
                (lists.get(start + 1..items_end), lists.get(items_end..end))
            else {
                return invalid("a list is truncated");
            };
            if probabilities.iter().any(|&value| value > PROBABILITY_SCALE) {
                return invalid("a probability is greater than one");
            }
            let exists = |&record: &u32| {
                table_forms
                    .get((record >> 16) as usize)
                    .and_then(|cells| cells.get((record & 0xFFFF) as usize))
                    .is_some_and(|&cell| cell & LEMMA_ONLY == 0)
            };
            if !items.iter().all(exists) {
                return invalid("a list refers to a missing table or form or to a lemma");
            }
            start = end;
        }
        Ok(())
    }

    /// Number of word forms over the keys of the automaton: the keys with several spellings in the
    /// section `variants`; lengths, references to the lists of analyses and weights are checked
    fn check_variants(&self) -> Result<usize, Error> {
        let invalid = |msg: &str| Err(Error::Format(msg.into()));
        let records = self.records().len();
        let mut extra = 0;
        let mut rest = self.variants();
        while !rest.is_empty() {
            let Some((variants, after)) = spelling_list(rest) else {
                return invalid("a list of spellings is truncated");
            };
            if variants.len() == 0
                || variants.len() > MAX_SPELLINGS
                || variants
                    .iter()
                    .any(|(_, offset, weight)| offset as usize >= records || weight == 0)
            {
                return invalid("a wrong list of spellings");
            }
            extra += variants.len() - 1;
            rest = after;
        }
        Ok(extra)
    }

    /// Lists of forms for guessing exactly cover their section, are not longer than `MAX_LIST` and
    /// refer to word forms of existing tables with numbers above zero. An ending of the automaton
    /// that refers inside a list gives forms that the guessing checks as it reads them
    fn check_guesses(&self, table_forms: &[&[u32]]) -> Result<(), Error> {
        let invalid = |msg: &str| Err(Error::Format(msg.into()));
        let lists = self.guess_records();
        let mut start = 0;
        while start < lists.len() {
            let Some(list) = guess_list(lists, start as u64) else {
                return invalid("a list of guesses is truncated");
            };
            let fits = |&[record, count]: &[u32; 2]| {
                count > 0
                    && (table_forms.get((record >> 16) as usize))
                        .and_then(|cells| cells.get((record & 0xFFFF) as usize))
                        .is_some_and(|&cell| cell & LEMMA_ONLY == 0)
            };
            if list.is_empty() || list.len() > MAX_LIST || !list.iter().all(fits) {
                return invalid("a wrong list of guesses");
            }
            start += 1 + 2 * list.len();
        }
        Ok(())
    }
}

/// List of forms for guessing from the place `offset` of the section: `(table << 16) | form` and
/// the number of word forms of each
fn guess_list(lists: &[u32], offset: u64) -> Option<&[[u32; 2]]> {
    let start = usize::try_from(offset).ok()?;
    let count = *lists.get(start)? as usize;
    let list = lists.get(start + 1..)?.get(..2 * count)?;
    Some(list.as_chunks::<2>().0)
}

/// Forms of an inflection table with the stem of a word form
struct Lexeme<'a> {
    table: u16,
    stem: &'a str,
    /// Numbers of the suffixes of the forms
    paradigm: &'a [u32],
    /// Flags and UD tags of the forms
    cells: &'a [u32],
}

impl Lexeme<'_> {
    fn spell(&self, suffixes: &Strings, form: usize) -> Option<String> {
        let suffix = suffixes.get(*self.paradigm.get(form)? as usize)?;
        let mut word = String::with_capacity(self.stem.len() + suffix.len());
        word.push_str(self.stem);
        word.push_str(suffix);
        Some(word)
    }

    fn ud(&self, form: usize) -> Option<u16> {
        Some((*self.cells.get(form)? & UD_BITS) as u16)
    }
}

/// The lemma of a name of the dictionary for a token: the token if it is the lemma in another case
/// (`PSOE`, `EE.UU.`), otherwise the lemma with a capital letter (`Madrid`)
fn name_lemma(token: &str, lemma: &str) -> String {
    if token.chars().any(char::is_uppercase) && lower_eq(token, lemma) {
        return token.to_owned();
    }
    let mut chars = lemma.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// Whether a token in lower case is `lower`
fn lower_eq(token: &str, lower: &str) -> bool {
    token.chars().flat_map(char::to_lowercase).eq(lower.chars())
}

/// A form with the lemma of the analysis it comes from, which may be the lemma of the table in
/// another case: the forms of `Madrid` have the lemma `Madrid`
fn with_lemma(mut form: Analysis, lemma: &str) -> Analysis {
    if form.lemma != lemma && form.lemma == lemma.to_lowercase() {
        form.lemma = lemma.to_owned();
    }
    form
}

/// Whether a value of a feature is `wanted` or lists it: `Acc,Nom` has `Acc`
fn has(value: Option<&str>, wanted: &str) -> bool {
    value.is_some_and(|value| value == wanted || value.split(',').any(|part| part == wanted))
}

/// A value of a feature: the number of its string in the strings of the tags, if it has one
#[derive(Clone, Copy)]
struct Value<'a> {
    number: Option<u16>,
    text: &'a str,
}

/// Whether a tag has the value of the feature whose name has the number `name`, as `has`: the
/// numbers of equal strings are equal
fn has_value(tag: UdTag<'_>, name: Option<u16>, value: Value) -> bool {
    let Some(known) = name.and_then(|name| tag.feat_value(name)) else {
        return false;
    };
    Some(known) == value.number || {
        let text = tag.text(known);
        text.contains(',') && text.split(',').any(|part| part == value.text)
    }
}

/// Whether two values of a feature share a part: `Acc` and `Acc,Nom`
fn shares_part(value: &str, other: &str) -> bool {
    (value.split(',')).any(|part| other.split(',').any(|known| known == part))
}

/// Analyses of a token by the analyses of its host and its clitics
fn with_clitics(token: &str, split: &clitics::Split, hosts: Vec<Analysis>) -> Vec<Analysis> {
    with_host_source(token, split, hosts, Source::Clitics)
}

/// Analyses of a token by the analyses of its host and its clitics, with their source
fn with_host_source(
    token: &str,
    split: &clitics::Split,
    hosts: Vec<Analysis>,
    source: Source,
) -> Vec<Analysis> {
    let total: f64 = hosts.iter().map(|analysis| analysis.prob).sum();
    hosts
        .into_iter()
        .map(|host| {
            let mut words = Vec::with_capacity(1 + split.clitics.len());
            words.push(host.word);
            words.extend(split.clitics.iter().map(|&clitic| clitic.to_owned()));
            Analysis {
                word: token.to_owned(),
                lemma: host.lemma,
                ud: host.ud,
                prob: host.prob / total,
                source,
                entry: host.entry,
                words,
            }
        })
        .collect()
}

/// List of analyses from the place `offset` of the section: the analyses and their probabilities,
/// empty if the list has none
fn analyses_at(lists: &[u32], offset: u32) -> Option<(&[u32], &[u32])> {
    let start = offset as usize + 1;
    let head = *lists.get(start - 1)?;
    let count = (head & !HAS_PROBABILITIES) as usize;
    if count > MAX_LIST {
        return None;
    }
    let (records, rest) = lists.get(start..)?.split_at_checked(count)?;
    let probabilities = if head & HAS_PROBABILITIES != 0 {
        rest.get(..count)?
    } else {
        &[]
    };
    Some((records, probabilities))
}

/// List of spellings "length, four numbers of each" at the start of `lists` and the rest after it
fn spelling_list(lists: &[u32]) -> Option<(SpellingList<'_>, &[u32])> {
    let (&count, rest) = lists.split_first()?;
    let wide = count & WIDE_SPELLINGS != 0;
    let count = usize::try_from(count & !WIDE_SPELLINGS)
        .ok()
        .filter(|&count| count <= MAX_LIST)?;
    let (numbers, rest) = rest.split_at_checked(if wide { 4 } else { 2 } * count)?;
    Some((SpellingList { numbers, wide }, rest))
}

/// Automaton of a section of a dictionary. A damaged automaton loads without errors but panics when
/// walked; the checksum of the file catches damage, and the address of the root is checked here:
/// `Fst::new` checks it only for an empty automaton
fn automaton(storage: &Arc<Storage>, range: &Range<usize>) -> Result<Map<Slice>, Error> {
    let slice = Slice {
        storage: Arc::clone(storage),
        range: range.clone(),
    };
    let map = Map::new(slice).map_err(|err| Error::Format(err.to_string()))?;
    // The root is written last and is followed by 20 bytes: the number of keys, the address of the
    // root and a checksum
    let bytes = map.as_fst().as_bytes();
    let end = bytes.len() - 4;
    let root = u64::from_le_bytes(bytes[end - 8..end].try_into().unwrap_or_default());
    let empty = root == 0 && bytes.len() == 36;
    if !empty && root.checked_add(21) != Some(bytes.len() as u64) {
        return Err(Error::Format(
            "wrong address of the root of the automaton".into(),
        ));
    }
    Ok(map)
}

/// Reads every node of an automaton reachable from its root as `Map::get` reads it: its transitions,
/// the lookup of every byte it has and of one it has not (and of all 256 bytes in a node with an
/// index of bytes) and its final output. `false` if a lookup does not find a transition of the node,
/// a transition leads to a node written after its own, as no automaton of fst does, or the outputs
/// along a path overflow
fn walk(fst: &fst::raw::Fst<Slice>) -> bool {
    // fst indexes the bytes of a node with more transitions
    const INDEXED: usize = 32;
    let mut seen = vec![0u64; fst.as_bytes().len() / 64 + 1];
    let mut nodes = Vec::new();
    let mut stack = vec![fst.root().addr()];
    while let Some(addr) = stack.pop() {
        let Some(word) = seen.get_mut(addr / 64) else {
            return false;
        };
        if *word & (1 << (addr % 64)) != 0 {
            continue;
        }
        *word |= 1 << (addr % 64);
        nodes.push(addr);
        let node = fst.node(addr);
        let mut inputs = [false; 256];
        for i in 0..node.len() {
            let transition = node.transition(i);
            inputs[usize::from(transition.inp)] = true;
            if transition.addr >= addr || node.find_input(transition.inp) != Some(i) {
                return false;
            }
            stack.push(transition.addr);
        }
        // The values are kept: a lookup that the optimizer leaves out would not panic
        if node.len() > INDEXED {
            for byte in 0..=u8::MAX {
                std::hint::black_box(node.find_input(byte));
            }
        } else if let Some(absent) = (0..=u8::MAX).find(|&byte| !inputs[usize::from(byte)]) {
            std::hint::black_box(node.find_input(absent));
        }
    }
    // The largest output from every node to the end of a key and the number of keys after it,
    // from the nodes written first: the number of keys in the footer is not checked by fst
    nodes.sort_unstable();
    let mut largest = vec![0u64; nodes.len()];
    let mut keys = vec![0u64; nodes.len()];
    for (index, &addr) in nodes.iter().enumerate() {
        let node = fst.node(addr);
        let (mut most, mut count) = match node.is_final() {
            true => (node.final_output().value(), 1u64),
            false => (0, 0),
        };
        for transition in node.transitions() {
            let Ok(next) = nodes.binary_search(&transition.addr) else {
                return false;
            };
            match transition.out.value().checked_add(largest[next]) {
                Some(total) => most = most.max(total),
                None => return false,
            }
            match count.checked_add(keys[next]) {
                Some(total) => count = total,
                None => return false,
            }
        }
        largest[index] = most;
        keys[index] = count;
    }
    (nodes.binary_search(&fst.root().addr())).is_ok_and(|root| keys[root] == fst.len() as u64)
}

/// Length of a part of a sentence in `Dictionary::analyze_sentence`: the model analyzes a long
/// sentence by parts, so its memory does not grow with the input
pub const SENTENCE_CHUNK: usize = 256;

/// A query in the terms of the dictionary: invisible characters are dropped, a combining mark
/// joins its letter, and a hyphen of Unicode is the hyphen-minus. Tokens of a text keep them
fn normalized(word: &str) -> Cow<'_, str> {
    if word.is_ascii()
        || !word
            .chars()
            .any(|c| is_invisible(c) || is_hyphen(c) || combined(' ', c).is_some())
    {
        return Cow::Borrowed(word);
    }
    let mut out = String::with_capacity(word.len());
    for c in word.chars() {
        if is_invisible(c) {
            continue;
        }
        if is_hyphen(c) {
            out.push('-');
            continue;
        }
        match out.chars().next_back().and_then(|last| combined(last, c)) {
            Some(joined) => {
                out.pop();
                out.push(joined);
            }
            None => out.push(c),
        }
    }
    Cow::Owned(out)
}

/// Soft hyphen, zero width characters, marks of the direction of text, invisible operators,
/// the combining grapheme joiner and the byte order mark
fn is_invisible(c: char) -> bool {
    matches!(
        c,
        '\u{AD}'
            | '\u{34F}'
            | '\u{200B}'..='\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{2069}'
            | '\u{FEFF}'
    )
}

/// The hyphen and the non-breaking hyphen of Unicode
fn is_hyphen(c: char) -> bool {
    matches!(c, '\u{2010}' | '\u{2011}')
}

/// The letter of a base and a combining mark of Spanish; the base `' '` tells only the mark apart
fn combined(base: char, mark: char) -> Option<char> {
    let table: &[(char, char)] = match mark {
        '\u{301}' => &[
            ('a', 'á'),
            ('e', 'é'),
            ('i', 'í'),
            ('o', 'ó'),
            ('u', 'ú'),
            ('A', 'Á'),
            ('E', 'É'),
            ('I', 'Í'),
            ('O', 'Ó'),
            ('U', 'Ú'),
            (' ', ' '),
        ],
        '\u{303}' => &[('n', 'ñ'), ('N', 'Ñ'), (' ', ' ')],
        '\u{308}' => &[('u', 'ü'), ('U', 'Ü'), (' ', ' ')],
        _ => return None,
    };
    table
        .iter()
        .find(|&&(letter, _)| letter == base)
        .map(|&(_, joined)| joined)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::builder::Builder;
    use crate::format::tests::with_checksum;

    const FORMS: &[(&str, &str, &str, &str)] = &[
        ("casa", "NOUN", "casa", "Gender=Fem|Number=Sing"),
        ("casa", "NOUN", "casas", "Gender=Fem|Number=Plur"),
        (
            "casar",
            "VERB",
            "casas",
            "Mood=Ind|Number=Sing|Person=2|Tense=Pres|VerbForm=Fin",
        ),
        ("casar", "VERB", "casar", "VerbForm=Inf"),
        ("camión", "NOUN", "camión", "Gender=Masc|Number=Sing"),
        ("camión", "NOUN", "camiones", "Gender=Masc|Number=Plur"),
        ("público", "ADJ", "público", "Gender=Masc|Number=Sing"),
        (
            "publicar",
            "VERB",
            "publico",
            "Mood=Ind|Number=Sing|Person=1|Tense=Pres|VerbForm=Fin",
        ),
        (
            "publicar",
            "VERB",
            "publicó",
            "Mood=Ind|Number=Sing|Person=3|Tense=Past|VerbForm=Fin",
        ),
        ("más", "ADV", "más", ""),
        ("mas", "CCONJ", "mas", ""),
        ("pingüino", "NOUN", "pingüino", "Gender=Masc|Number=Sing"),
        (
            "averiguar",
            "VERB",
            "averigüé",
            "Mood=Ind|Number=Sing|Person=1|Tense=Past|VerbForm=Fin",
        ),
        ("ambigú", "NOUN", "ambigúes", "Gender=Masc|Number=Plur"),
        ("año", "NOUN", "año", "Gender=Masc|Number=Sing"),
        ("ano", "NOUN", "ano", "Gender=Masc|Number=Sing"),
        ("yo", "PRON", "me", "Person=1"),
        ("guión", "NOUN", "guiones", "Number=Plur"),
        (
            "ver",
            "VERB",
            "vi",
            "Mood=Ind|Number=Sing|Person=1|Tense=Past|VerbForm=Fin",
        ),
    ];

    fn ranked_dictionary() -> Dictionary {
        let mut builder = Builder::new();
        builder.meta("{}");
        for &(lemma, pos, form, feats) in FORMS {
            builder.add_form(lemma, pos, form, feats).unwrap();
        }
        builder
            .add_form("medicar", "VERB", "medicó", "Tense=Past")
            .unwrap();
        builder
            .add_form("médico", "NOUN", "médico", "Number=Sing")
            .unwrap();
        let casas = "Mood=Ind|Number=Sing|Person=2|Tense=Pres|VerbForm=Fin";
        builder
            .place("casar", "VERB", "casas", casas, 0, Some(0.75))
            .unwrap();
        let plural = "Gender=Fem|Number=Plur";
        builder
            .place("casa", "NOUN", "casas", plural, 1, Some(0.25))
            .unwrap();
        Dictionary::from_bytes(&builder.build().unwrap()).unwrap()
    }

    #[test]
    fn places_order_the_analyses_and_keep_probabilities() {
        let analyses = ranked_dictionary().analyze("casas");
        let summary: Vec<_> = analyses
            .iter()
            .map(|a| (a.lemma.as_str(), a.prob))
            .collect();
        assert_eq!(summary, [("casar", 0.75), ("casa", 0.25)]);
    }

    #[test]
    fn a_list_without_probabilities_has_equal_shares() {
        let analyses = dictionary().analyze("casas");
        assert_eq!(analyses.len(), 2);
        assert!(analyses.iter().all(|analysis| analysis.prob == 0.5));
        assert_eq!(dictionary().analyze("camión")[0].prob, 1.0);
    }

    #[test]
    fn several_spellings_share_the_probability() {
        let analyses = ranked_dictionary().analyze("medico");
        let summary: Vec<_> = analyses.iter().map(|a| (a.word.as_str(), a.prob)).collect();
        assert_eq!(summary.len(), 2);
        assert!(summary.iter().all(|&(_, prob)| prob == 0.5));
    }

    fn clitic_dictionary() -> Dictionary {
        let mut builder = Builder::new();
        builder.meta("{}");
        let imperative = "Mood=Imp|Number=Sing|Person=2|VerbForm=Fin";
        builder.add_form("dar", "VERB", "da", imperative).unwrap();
        builder
            .add_form("dar", "VERB", "da", "Mood=Ind|Person=3")
            .unwrap();
        builder
            .add_form("dar", "VERB", "dar", "VerbForm=Inf")
            .unwrap();
        builder
            .add_form("ir", "VERB", "vamos", "Mood=Imp|Person=1")
            .unwrap();
        builder
            .add_form("cometer", "VERB", "comete", "Mood=Ind")
            .unwrap();
        builder
            .add_form("comer", "VERB", "come", imperative)
            .unwrap();
        for (form, person) in [("cuente", "3"), ("cuenten", "3")] {
            let feats = format!("Mood=Imp|Person={person}");
            builder.add_form("contar", "VERB", form, &feats).unwrap();
        }
        builder
            .add_form("sentar", "VERB", "sentad", "Mood=Imp|Person=2")
            .unwrap();
        builder.add_form("de", "ADP", "del", "").unwrap();
        builder.add_form("casa", "NOUN", "casa", "").unwrap();
        let past = "Mood=Ind|Number=Sing|Person=3|Tense=Past|VerbForm=Fin";
        builder.add_form("decir", "VERB", "dijo", past).unwrap();
        builder
            .add_form("sentar", "VERB", "sienta", imperative)
            .unwrap();
        let polite = "Mood=Imp|Number=Sing|Person=3|VerbForm=Fin";
        builder
            .add_form("sentir", "VERB", "sienta", polite)
            .unwrap();
        builder
            .add_form("ver", "VERB", "ver", "VerbForm=Inf")
            .unwrap();
        builder
            .add_form("verter", "VERB", "verté", imperative)
            .unwrap();
        builder.add_form("versar", "VERB", "verse", polite).unwrap();
        builder
            .add_form("alentar", "VERB", "aliente", polite)
            .unwrap();
        let plural = "Mood=Imp|Number=Plur|Person=3|VerbForm=Fin";
        builder.add_form("aliar", "VERB", "alíen", plural).unwrap();
        let vosotros = "Mood=Imp|Number=Plur|Person=2|VerbForm=Fin";
        (builder.add_form("distribuir", "VERB", "distribuid", vosotros)).unwrap();
        let participle = "Gender=Masc|Number=Plur|Tense=Past|VerbForm=Part";
        (builder.add_form("distribuir", "VERB", "distribuidos", participle)).unwrap();
        Dictionary::from_bytes(&builder.build().unwrap()).unwrap()
    }

    #[test]
    fn a_verb_with_clitics_is_its_host() {
        let dictionary = clitic_dictionary();
        let analyses = dictionary.analyze("dámelo");
        assert_eq!(analyses.len(), 1, "only the imperative takes clitics");
        let analysis = &analyses[0];
        assert_eq!(analysis.word, "dámelo");
        assert_eq!(analysis.lemma, "dar");
        assert_eq!(analysis.words, ["da", "me", "lo"]);
        assert_eq!(analysis.source, Source::Clitics);
        assert_eq!(analysis.prob, 1.0);
        assert_eq!(dictionary.analyze("darlo")[0].words, ["dar", "lo"]);
        assert_eq!(dictionary.analyze("vámonos")[0].words, ["vamos", "nos"]);
        assert_eq!(dictionary.analyze("cuéntenos")[0].words, ["cuente", "nos"]);
        assert_eq!(dictionary.analyze("sentaos")[0].words, ["sentad", "os"]);
        assert_eq!(dictionary.analyze("daros")[0].words, ["dar", "os"]);
        assert!(dictionary.is_known("dámelo"));
    }

    #[test]
    fn a_token_with_a_capital_that_splits_may_be_a_name() {
        let dictionary = clitic_dictionary();
        let sources = |word| {
            let analyses = dictionary.analyze(word);
            let total: f64 = analyses.iter().map(|analysis| analysis.prob).sum();
            assert!((total - 1.0).abs() < 1e-9, "{word}: {total}");
            (analyses.into_iter())
                .map(|analysis| (analysis.lemma, analysis.source))
                .collect::<Vec<_>>()
        };
        // An imperative without an accent is mostly a name (`Mandela`), an imperative with one
        // and an infinitive mostly a verb
        assert_eq!(
            sources("Dame"),
            [
                ("Dame".into(), Source::Rule),
                ("dar".into(), Source::Clitics)
            ]
        );
        for word in ["Dámelo", "Darlo"] {
            assert_eq!(
                sources(word),
                [("dar".into(), Source::Clitics), (word.into(), Source::Rule)]
            );
        }
        assert_eq!(dictionary.analyze("Díjole")[0].source, Source::Clitics);
        assert_eq!(dictionary.analyze("DÁMELO")[0].source, Source::Clitics);
    }

    #[test]
    fn a_finite_form_takes_clitics_with_its_accent() {
        let dictionary = clitic_dictionary();
        let analyses = dictionary.analyze("díjole");
        assert_eq!(analyses[0].lemma, "decir");
        assert_eq!(analyses[0].words, ["dijo", "le"]);
        assert_eq!(analyses[0].source, Source::Clitics);
        assert!(dictionary.is_known("díjole"));
        // Without the accent of the token it takes none
        assert!(dictionary.analyze("dijole").is_empty());
    }

    #[test]
    fn clitics_agree_with_the_person_of_the_imperative() {
        let dictionary = clitic_dictionary();
        let lemmas: Vec<String> = (dictionary.analyze("siéntate").into_iter())
            .map(|analysis| analysis.lemma)
            .collect();
        assert_eq!(lemmas, ["sentar"]);
        let lemmas: Vec<String> = (dictionary.analyze("siéntase").into_iter())
            .map(|analysis| analysis.lemma)
            .collect();
        assert_eq!(lemmas, ["sentir"]);
    }

    #[test]
    fn a_verb_with_clitics_goes_before_other_accents() {
        let dictionary = clitic_dictionary();
        // `verte` is `ver` + `te`, not the imperative `verté`
        assert_eq!(dictionary.analyze("verte")[0].words, ["ver", "te"]);
        // `verse` is mostly `ver` + `se`, though the dictionary has it
        let analyses = dictionary.analyze("verse");
        assert_eq!(analyses[0].words, ["ver", "se"]);
        assert_eq!(analyses[1].lemma, "versar");
        assert!(analyses.windows(2).all(|pair| pair[0].prob >= pair[1].prob));
        // A word the dictionary has is no more a name for its clitics
        let capital = dictionary.analyze("Verse");
        assert_eq!(capital[0].words, ["ver", "se"]);
        assert!(capital.windows(2).all(|pair| pair[0].prob >= pair[1].prob));
        let total: f64 = analyses.iter().map(|analysis| analysis.prob).sum();
        assert!((total - 1.0).abs() < 1e-9, "{total}");
        // Without an accent, a host as written goes first
        assert_eq!(dictionary.analyze("alientelo")[0].lemma, "alentar");
        // The `d` of the imperative does not stay before `os`
        let analyses = dictionary.analyze("distribuídos");
        assert!(analyses.iter().all(|analysis| analysis.words.is_empty()));
        assert_eq!(analyses[0].lemma, "distribuir");
    }

    #[test]
    fn a_word_of_the_dictionary_is_not_split() {
        let dictionary = clitic_dictionary();
        let analyses = dictionary.analyze("comete");
        assert_eq!(analyses[0].lemma, "cometer");
        assert!(analyses[0].words.is_empty());
        assert_eq!(
            dictionary.analyze("cómetelo")[0].words,
            ["come", "te", "lo"]
        );
        // No host: a noun takes no clitics
        assert!(dictionary.analyze("casalo").is_empty());
        assert!(!dictionary.is_known("casalo"));
    }

    fn inflection_dictionary() -> Dictionary {
        let mut builder = Builder::new();
        builder.meta("{}");
        let fin = |mood: &str, tense: &str, person: &str, number: &str| {
            let tense = if tense.is_empty() {
                String::new()
            } else {
                format!("|Tense={tense}")
            };
            format!("Mood={mood}|Number={number}|Person={person}{tense}|VerbForm=Fin")
        };
        let verb = [
            ("hablar", "VerbForm=Inf".to_owned()),
            ("hablando", "VerbForm=Ger".to_owned()),
            (
                "hablado",
                "Gender=Masc|Number=Sing|Tense=Past|VerbForm=Part".to_owned(),
            ),
            (
                "habladas",
                "Gender=Fem|Number=Plur|Tense=Past|VerbForm=Part".to_owned(),
            ),
            ("hablo", fin("Ind", "Pres", "1", "Sing")),
            ("hablas", fin("Ind", "Pres", "2", "Sing")),
            ("hablás", fin("Ind", "Pres", "2", "Sing")),
            ("habla", fin("Ind", "Pres", "3", "Sing")),
            ("hablamos", fin("Ind", "Pres", "1", "Plur")),
            ("hablé", fin("Ind", "Past", "1", "Sing")),
            ("habló", fin("Ind", "Past", "3", "Sing")),
            ("hablara", fin("Sub", "Imp", "1", "Sing")),
            ("hablase", fin("Sub", "Imp", "1", "Sing")),
            ("hablara", fin("Sub", "Imp", "3", "Sing")),
            ("hablase", fin("Sub", "Imp", "3", "Sing")),
            ("habla", fin("Imp", "", "2", "Sing")),
            ("hablá", fin("Imp", "", "2", "Sing")),
            ("hablad", fin("Imp", "", "2", "Plur")),
            ("fablar", "VerbForm=Inf".to_owned()),
        ];
        for (form, feats) in &verb {
            builder.add_form("hablar", "VERB", form, feats).unwrap();
        }
        for (form, feats) in [
            ("hablás", fin("Ind", "Pres", "2", "Sing")),
            ("hablá", fin("Imp", "", "2", "Sing")),
            ("fablar", "VerbForm=Inf".to_owned()),
        ] {
            builder
                .mark_variant("hablar", "VERB", form, &feats)
                .unwrap();
        }
        for (form, feats) in [
            ("gatos", "Gender=Masc|Number=Plur"),
            ("gata", "Gender=Fem|Number=Sing"),
            ("gato", "Gender=Masc|Number=Sing"),
            ("gatas", "Gender=Fem|Number=Plur"),
        ] {
            builder.add_form("gato", "NOUN", form, feats).unwrap();
        }
        for (form, feats) in [
            ("yo", "Case=Nom|Number=Sing|Person=1"),
            ("me", "Case=Acc|Number=Sing|Person=1"),
            ("nosotros", "Case=Acc,Nom|Number=Plur|Person=1"),
        ] {
            builder.add_form("yo", "PRON", form, feats).unwrap();
        }
        builder
            .add_form("guión", "NOUN", "guiones", "Number=Plur")
            .unwrap();
        builder.add_form("a", "ADP", "a", "").unwrap();
        builder.add_form("a", "ADP", "al", "").unwrap();
        Dictionary::from_bytes(&builder.build().unwrap()).unwrap()
    }

    fn analysis_of(dictionary: &Dictionary, word: &str, feats: &str) -> Analysis {
        dictionary
            .analyze(word)
            .into_iter()
            .find(|a| dictionary.ud_tag(a.ud).unwrap().feats_string() == feats)
            .unwrap()
    }

    fn inflected(dictionary: &Dictionary, analysis: &Analysis, feats: &[(&str, &str)]) -> String {
        dictionary.inflect(analysis, feats).unwrap().word
    }

    #[test]
    fn forms_go_in_the_order_of_the_paradigm() {
        let dictionary = inflection_dictionary();
        let forms: Vec<String> = dictionary
            .forms(&dictionary.analyze("habló")[0])
            .into_iter()
            .map(|form| form.word)
            .collect();
        assert_eq!(
            forms,
            [
                "hablar", "hablando", "hablado", "habladas", "hablo", "hablas", "habla",
                "hablamos", "hablé", "habló", "hablara", "hablase", "hablara", "hablase", "habla",
                "hablad", "fablar", "hablás", "hablá"
            ]
        );
        let gato = dictionary.forms(&dictionary.analyze("gatas")[0]);
        let words: Vec<&str> = gato.iter().map(|form| form.word.as_str()).collect();
        assert_eq!(words, ["gato", "gata", "gatos", "gatas"]);
        assert!(
            gato.iter()
                .all(|form| form.lemma == "gato" && form.prob == 1.0)
        );
        assert_eq!(
            gato[1],
            analysis_of(&dictionary, "gata", "Gender=Fem|Number=Sing")
        );
    }

    #[test]
    fn the_more_frequent_of_equal_forms_goes_first() {
        let present = "Mood=Ind|Number=Sing|Person=3|Tense=Pres|VerbForm=Fin";
        let build = |counts: &[(&str, u32)]| {
            let mut builder = Builder::new();
            for form in ["haber", "ha", "hay"] {
                let feats = if form == "haber" {
                    "VerbForm=Inf"
                } else {
                    present
                };
                builder.add_form("haber", "VERB", form, feats).unwrap();
            }
            for &(form, count) in counts {
                builder
                    .set_count("haber", "VERB", form, present, count)
                    .unwrap();
            }
            let dictionary = Dictionary::from_bytes(&builder.build().unwrap()).unwrap();
            let haber = &dictionary.analyze("haber")[0];
            let ha = &dictionary.analyze("ha")[0];
            (
                inflected(&dictionary, haber, &[("Tense", "Pres"), ("Person", "3")]),
                // The analysis that starts the more frequent form stays itself
                inflected(&dictionary, ha, &[]),
                inflected(&dictionary, ha, &[("Person", "3")]),
            )
        };
        let ha = ("ha".to_owned(), "ha".to_owned(), "ha".to_owned());
        assert_eq!(build(&[]), ha);
        assert_eq!(
            build(&[("hay", 30), ("ha", 2)]),
            ("hay".to_owned(), ha.1.clone(), ha.2.clone())
        );
        // A difference within chance does not count
        assert_eq!(build(&[("hay", 3), ("ha", 1)]), ha);
        assert_eq!(build(&[("ha", 2)]), ha);
    }

    #[test]
    fn equal_forms_go_by_their_place_in_the_source() {
        let present = "Mood=Ind|Number=Sing|Person=1|Tense=Pres|VerbForm=Fin";
        let build = |ranks: &[(&str, u32)]| {
            let mut builder = Builder::new();
            builder
                .add_form("ser", "VERB", "ser", "VerbForm=Inf")
                .unwrap();
            for form in ["seigo", "soy"] {
                builder.add_form("ser", "VERB", form, present).unwrap();
            }
            for &(form, rank) in ranks {
                builder
                    .set_rank("ser", "VERB", form, present, rank)
                    .unwrap();
            }
            let dictionary = Dictionary::from_bytes(&builder.build().unwrap()).unwrap();
            let ser = &dictionary.analyze("ser")[0];
            inflected(&dictionary, ser, &[("Mood", "Ind"), ("Person", "1")])
        };
        // Without places the letters decide; a form with a place goes before one without
        assert_eq!(build(&[]), "seigo");
        assert_eq!(build(&[("soy", 5)]), "soy");
        assert_eq!(build(&[("soy", 5), ("seigo", 2)]), "seigo");
    }

    #[test]
    fn equal_forms_keep_the_spelling_of_the_analysis() {
        let mut builder = Builder::new();
        let part = |gender: &str| format!("Gender={gender}|Number=Sing|Tense=Past|VerbForm=Part");
        builder
            .add_form("elegir", "VERB", "elegir", "VerbForm=Inf")
            .unwrap();
        for (form, gender) in [
            ("elegido", "Masc"),
            ("electo", "Masc"),
            ("elegida", "Fem"),
            ("electa", "Fem"),
        ] {
            builder
                .add_form("elegir", "VERB", form, &part(gender))
                .unwrap();
        }
        // Only the masculine has a count: the feminine goes in the order of letters
        builder
            .set_count("elegir", "VERB", "elegido", &part("Masc"), 18)
            .unwrap();
        let pronoun = "Case=Nom|Number=Sing|Person=3";
        builder.add_form("él", "PRON", "él", pronoun).unwrap();
        builder
            .add_form(
                "él",
                "PRON",
                "ella",
                "Case=Nom|Gender=Fem|Number=Sing|Person=3",
            )
            .unwrap();
        builder.add_form("él", "PRON", "ello", pronoun).unwrap();
        let dictionary = Dictionary::from_bytes(&builder.build().unwrap()).unwrap();
        let fem = [("Gender", "Fem")];
        assert_eq!(
            inflected(&dictionary, &dictionary.analyze("elegido")[0], &fem),
            "elegida"
        );
        assert_eq!(
            inflected(&dictionary, &dictionary.analyze("electo")[0], &fem),
            "electa"
        );
        // Without a form of its own tag to keep, the first form of the paradigm
        let elegir = &dictionary.analyze("elegir")[0];
        assert_eq!(
            inflected(&dictionary, elegir, &[("VerbForm", "Part")]),
            "elegido"
        );
        // The lemma first in the table is still one of its group
        assert_eq!(
            inflected(&dictionary, &dictionary.analyze("ello")[0], &[]),
            "ello"
        );
        assert_eq!(
            inflected(&dictionary, &dictionary.analyze("él")[0], &[]),
            "él"
        );
    }

    #[test]
    fn a_pronoun_with_a_feature_more_is_another_word() {
        let mut builder = Builder::new();
        for (form, feats) in [
            ("tú", "Case=Nom|Number=Sing|Person=2"),
            ("usted", "Case=Nom|Number=Sing|Person=2|Polite=Form"),
            ("vos", "Case=Nom|Number=Sing|Person=2"),
            ("vosotros", "Case=Nom|Number=Plur|Person=2"),
            ("ustedes", "Case=Nom|Number=Plur|Person=2|Polite=Form"),
        ] {
            builder.add_form("tú", "PRON", form, feats).unwrap();
        }
        let dictionary = Dictionary::from_bytes(&builder.build().unwrap()).unwrap();
        let plural = [("Number", "Plur")];
        assert_eq!(
            inflected(&dictionary, &dictionary.analyze("tú")[0], &plural),
            "vosotros"
        );
        assert_eq!(
            inflected(&dictionary, &dictionary.analyze("vos")[0], &[]),
            "vos"
        );
        let usted = &dictionary.analyze("usted")[0];
        assert_eq!(inflected(&dictionary, usted, &plural), "ustedes");
    }

    #[test]
    fn marks_need_an_added_form() {
        let mut builder = Builder::new();
        builder.add_form("a", "X", "a", "").unwrap();
        assert!(builder.mark_variant("a", "X", "b", "").is_err());
        assert!(builder.set_count("a", "Y", "a", "", 1).is_err());
        assert!(builder.mark_variant("a", "X", "a", "Number").is_err());
        assert!(builder.set_count("a", "X", "a", "", 1).is_ok());
    }

    #[test]
    fn unknown_flags_of_a_table_are_an_error() {
        let clean = bytes();
        let tables = format::layout(&clean, true).unwrap().tables;
        // The second form of the first table, `ambigúes` of `ambigú`: its UD tag and no flags
        let at = tables.start + 8;
        let mut damaged = clean.clone();
        damaged[at + 2] |= 0x04;
        let error = Dictionary::from_bytes(&with_checksum(damaged)).unwrap_err();
        assert!(error.to_string().contains("flags"), "{error}");
        let mut lemma_only = clean.clone();
        lemma_only[at + 2] |= 0x02;
        let error = Dictionary::from_bytes(&with_checksum(lemma_only)).unwrap_err();
        assert!(error.to_string().contains("lemma"), "{error}");
    }

    #[test]
    fn a_lemma_that_is_no_form_is_left_out() {
        let dictionary = inflection_dictionary();
        let forms = dictionary.forms(&dictionary.analyze("guiones")[0]);
        assert_eq!(forms.len(), 1);
        assert_eq!(
            (forms[0].word.as_str(), forms[0].lemma.as_str()),
            ("guiones", "guión")
        );
        let analysis = &dictionary.analyze("guiones")[0];
        assert!(
            dictionary
                .inflect(analysis, &[("Number", "Sing")])
                .is_none()
        );
    }

    #[test]
    fn inflect_keeps_the_other_features() {
        let dictionary = inflection_dictionary();
        let gata = &dictionary.analyze("gata")[0];
        assert_eq!(inflected(&dictionary, gata, &[("Number", "Plur")]), "gatas");
        assert_eq!(inflected(&dictionary, gata, &[("Gender", "Masc")]), "gato");
        assert_eq!(inflected(&dictionary, gata, &[]), "gata");
        assert!(dictionary.inflect(gata, &[("Number", "Dual")]).is_none());
        assert!(dictionary.inflect(gata, &[("Tense", "Past")]).is_none());
        // A form without a gender takes any gender, but not any tense
        let yo = &dictionary.analyze("yo")[0];
        assert_eq!(inflected(&dictionary, yo, &[("Gender", "Fem")]), "yo");
        assert!(dictionary.inflect(yo, &[("Tense", "Past")]).is_none());
        // A form with the gender goes first; a verb has no form without a gender
        assert_eq!(inflected(&dictionary, gata, &[("Number", "Sing")]), "gata");
        let hablar = &dictionary.analyze("hablar")[0];
        assert_eq!(
            inflected(&dictionary, hablar, &[("Gender", "Fem")]),
            "habladas"
        );
        let hablamos = analysis_of(
            &dictionary,
            "hablamos",
            "Mood=Ind|Number=Plur|Person=1|Tense=Pres|VerbForm=Fin",
        );
        assert_eq!(
            inflected(&dictionary, &hablamos, &[("Number", "Sing")]),
            "hablo"
        );
        assert_eq!(
            inflected(
                &dictionary,
                &hablamos,
                &[("Tense", "Past"), ("Number", "Sing")]
            ),
            "hablé"
        );
        let participle = &dictionary.analyze("hablado")[0];
        assert_eq!(
            inflected(
                &dictionary,
                participle,
                &[("Gender", "Fem"), ("Number", "Plur")]
            ),
            "habladas"
        );
    }

    #[test]
    fn an_infinitive_becomes_a_finite_form() {
        let dictionary = inflection_dictionary();
        let hablar = &dictionary.analyze("hablar")[0];
        assert_eq!(
            inflected(&dictionary, hablar, &[("Tense", "Past")]),
            "hablé"
        );
        assert_eq!(
            inflected(&dictionary, hablar, &[("Person", "3"), ("Tense", "Past")]),
            "habló"
        );
        assert_eq!(inflected(&dictionary, hablar, &[("Mood", "Imp")]), "habla");
        assert_eq!(
            inflected(&dictionary, hablar, &[("VerbForm", "Ger")]),
            "hablando"
        );
        assert_eq!(
            inflected(&dictionary, hablar, &[("VerbForm", "Inf")]),
            "hablar"
        );
        let gerund = &dictionary.analyze("hablando")[0];
        assert_eq!(
            inflected(&dictionary, gerund, &[("Number", "Plur")]),
            "hablamos"
        );
    }

    #[test]
    fn inflect_keeps_variants_and_the_place_among_equal_forms() {
        let dictionary = inflection_dictionary();
        let main = analysis_of(
            &dictionary,
            "hablas",
            "Mood=Ind|Number=Sing|Person=2|Tense=Pres|VerbForm=Fin",
        );
        let vos = &dictionary.analyze("hablás")[0];
        assert_eq!(inflected(&dictionary, &main, &[("Mood", "Imp")]), "habla");
        assert_eq!(inflected(&dictionary, vos, &[("Mood", "Imp")]), "hablá");
        assert_eq!(inflected(&dictionary, vos, &[("Person", "1")]), "hablo");
        assert_eq!(
            inflected(&dictionary, &dictionary.analyze("fablar")[0], &[]),
            "fablar"
        );
        let se = analysis_of(
            &dictionary,
            "hablase",
            "Mood=Sub|Number=Sing|Person=1|Tense=Imp|VerbForm=Fin",
        );
        assert_eq!(inflected(&dictionary, &se, &[("Person", "3")]), "hablase");
        let ra = analysis_of(
            &dictionary,
            "hablara",
            "Mood=Sub|Number=Sing|Person=1|Tense=Imp|VerbForm=Fin",
        );
        assert_eq!(inflected(&dictionary, &ra, &[("Person", "3")]), "hablara");
    }

    #[test]
    fn a_variant_takes_a_closer_main_form() {
        let mut builder = Builder::new();
        builder.meta("{}");
        let forms = [
            ("bueno", "Gender=Masc|Number=Sing", false),
            ("buen", "Gender=Masc|Number=Sing", true),
            ("bon", "Gender=Masc|Number=Sing", true),
            ("buenos", "Gender=Masc|Number=Plur", false),
            ("bones", "Gender=Masc|Number=Plur", true),
        ];
        for (form, feats, variant) in forms {
            builder.add_form("buen", "ADJ", form, feats).unwrap();
            if variant {
                builder.mark_variant("buen", "ADJ", form, feats).unwrap();
            }
        }
        let dictionary = Dictionary::from_bytes(&builder.build().unwrap()).unwrap();
        let plural = [("Number", "Plur")];
        let buen = &dictionary.analyze("buen")[0];
        assert_eq!(inflected(&dictionary, buen, &plural), "buenos");
        let bon = &dictionary.analyze("bon")[0];
        assert_eq!(inflected(&dictionary, bon, &plural), "bones");
    }

    #[test]
    fn a_value_may_list_several() {
        let dictionary = inflection_dictionary();
        let yo = &dictionary.analyze("yo")[0];
        assert_eq!(
            inflected(&dictionary, yo, &[("Number", "Plur")]),
            "nosotros"
        );
        assert_eq!(inflected(&dictionary, yo, &[("Case", "Acc")]), "me");
        let we = &dictionary.analyze("nosotros")[0];
        assert_eq!(inflected(&dictionary, we, &[("Number", "Sing")]), "yo");
    }

    #[test]
    fn a_verb_with_clitics_inflects_as_its_verb() {
        let dictionary = clitic_dictionary();
        let token = &dictionary.analyze("dámelo")[0];
        let forms: Vec<String> = (dictionary.forms(token).into_iter())
            .map(|form| form.word)
            .collect();
        assert!(forms.contains(&"dar".to_owned()) && forms.contains(&"da".to_owned()));
        let infinitive = dictionary.inflect(token, &[("VerbForm", "Inf")]).unwrap();
        assert_eq!(infinitive.word, "dar");
        assert_eq!(infinitive.source, Source::Dictionary);
        assert!(infinitive.words.is_empty());
    }

    #[test]
    fn a_form_of_a_contraction_has_its_words() {
        let dictionary = inflection_dictionary();
        let forms = dictionary.forms(&dictionary.analyze("a")[0]);
        let words: Vec<(&str, Vec<String>)> = forms
            .iter()
            .map(|form| (form.word.as_str(), form.words.clone()))
            .collect();
        assert_eq!(
            words,
            [("a", vec![]), ("al", vec!["a".into(), "el".into()])]
        );
    }

    #[test]
    fn an_analysis_without_its_table_has_no_forms() {
        let dictionary = inflection_dictionary();
        let mut analysis = dictionary.analyze("gata")[0].clone();
        analysis.word = "perro".into();
        assert!(dictionary.forms(&analysis).is_empty());
        assert!(dictionary.inflect(&analysis, &[]).is_none());
    }

    #[test]
    fn tokens_out_of_the_dictionary_get_the_analyses_of_the_rules() {
        let dictionary = dictionary();
        let summary = |token: &str| -> Vec<(String, String, String, &'static str)> {
            dictionary
                .analyze(token)
                .into_iter()
                .map(|analysis| {
                    let tag = dictionary.ud_tag(analysis.ud).unwrap();
                    let pos = format!("{} {}", tag.pos(), tag.feats_string());
                    (analysis.word, analysis.lemma, pos, analysis.source.as_str())
                })
                .collect()
        };
        assert_eq!(
            summary("¿"),
            [(
                "¿".into(),
                "¿".into(),
                "PUNCT PunctSide=Ini|PunctType=Qest".into(),
                "rule"
            )]
        );
        assert_eq!(
            summary("10.000"),
            [(
                "10.000".into(),
                "10000".into(),
                "NUM NumForm=Digit|NumType=Card".into(),
                "rule"
            )]
        );
        assert_eq!(
            summary("Josep"),
            [("Josep".into(), "Josep".into(), "PROPN ".into(), "rule")]
        );
        assert_eq!(summary("XXI")[0].2, "NUM NumForm=Roman|NumType=Card");
        assert!(summary("qzxw").is_empty());
        // The rules give analyses, not words of the dictionary
        assert!(!dictionary.is_known("Josep") && !dictionary.is_known(","));
        // A Roman numeral in `I` that the dictionary knows is a numeral first
        let summary_vi: Vec<String> = (dictionary.analyze("VI").into_iter())
            .map(|analysis| dictionary.ud_tag(analysis.ud).unwrap().pos().to_owned())
            .collect();
        assert_eq!(summary_vi, ["NUM", "VERB", "PROPN"]);
        // A known word in capitals of a closed class is a name last
        let me = dictionary.analyze("ME");
        assert_eq!(me.len(), 2);
        assert_eq!((me[1].lemma.as_str(), me[1].source), ("ME", Source::Rule));
        let total: f64 = me.iter().map(|analysis| analysis.prob).sum();
        assert!((total - 1.0).abs() < 1e-9, "{total}");
    }

    #[test]
    fn an_analysis_without_a_lexeme_is_its_only_form() {
        let dictionary = inflection_dictionary();
        let number = &dictionary.analyze("10.000")[0];
        assert_eq!(number.entry, None);
        let forms = dictionary.forms(number);
        assert_eq!(forms.len(), 1);
        assert_eq!((forms[0].word.as_str(), forms[0].prob), ("10.000", 1.0));
        let card = [("NumType", "Card")];
        assert_eq!(dictionary.inflect(number, &card).unwrap().word, "10.000");
        // Without a gender or a number it takes any, as a form of a table does
        let plural = [("Number", "Plur")];
        assert_eq!(dictionary.inflect(number, &plural).unwrap().word, "10.000");
        assert!(dictionary.inflect(number, &[("NumType", "Ord")]).is_none());
    }

    #[test]
    fn a_contraction_has_its_words() {
        assert_eq!(clitic_dictionary().analyze("del")[0].words, ["de", "el"]);
        assert!(clitic_dictionary().analyze("casa")[0].words.is_empty());
    }

    #[test]
    fn frequencies_decide_between_spellings() {
        let mut builder = Builder::new();
        builder.meta("{}");
        builder
            .add_form("fotografía", "NOUN", "fotografía", "")
            .unwrap();
        builder
            .add_form("fotografía", "X", "fotografía", "")
            .unwrap();
        builder
            .add_form("fotografiar", "VERB", "fotografiá", "")
            .unwrap();
        builder.set_frequency("fotografía", 6);
        let dictionary = Dictionary::from_bytes(&builder.build().unwrap()).unwrap();
        let analyses = dictionary.analyze("fotografia");
        assert_eq!(analyses[0].word, "fotografía");
        assert_eq!(analyses.last().unwrap().word, "fotografiá");
        let total: f64 = analyses.iter().map(|analysis| analysis.prob).sum();
        assert!((total - 1.0).abs() < 1e-5, "{total}");
        // The exact spelling keeps the whole query
        assert_eq!(dictionary.analyze("fotografiá")[0].prob, 1.0);
    }

    #[test]
    fn spellings_without_frequencies_stay_in_their_order() {
        // Whichever spelling has more analyses, the order of accent places decides
        for (one, two) in [("autonóma", "autónoma"), ("autónoma", "autonóma")] {
            let mut builder = Builder::new();
            builder.meta("{}");
            builder.add_form("a", "NOUN", one, "").unwrap();
            builder.add_form("b", "NOUN", two, "").unwrap();
            builder.add_form("c", "NOUN", two, "").unwrap();
            let dictionary = Dictionary::from_bytes(&builder.build().unwrap()).unwrap();
            let analyses = dictionary.analyze("autonoma");
            assert_eq!(analyses.len(), 3);
            assert!(
                analyses
                    .iter()
                    .all(|analysis| analysis.prob == analyses[0].prob)
            );
            assert_eq!(analyses[0].word, "autónoma");
        }
    }

    #[test]
    fn a_zero_weight_of_a_spelling_is_an_error() {
        let mut builder = Builder::new();
        builder.meta("{}");
        builder.add_form("a", "NOUN", "ána", "").unwrap();
        builder.add_form("b", "NOUN", "aná", "").unwrap();
        builder.set_frequency("ána", 123_456_789);
        let bytes = builder.build().unwrap();
        let at = bytes
            .windows(4)
            .position(|window| window == 123_456_790u32.to_le_bytes())
            .unwrap();
        let mut damaged = bytes.clone();
        damaged[at..at + 4].copy_from_slice(&0u32.to_le_bytes());
        let error = Dictionary::from_bytes(&with_checksum(damaged)).unwrap_err();
        assert!(error.to_string().contains("spellings"), "{error}");
    }

    #[test]
    fn a_wrong_probability_is_an_error() {
        let bytes = {
            let mut builder = Builder::new();
            builder.meta("{}");
            builder.add_form("a", "X", "a", "").unwrap();
            builder.add_form("a", "Y", "a", "").unwrap();
            builder.place("a", "X", "a", "", 0, Some(1.0)).unwrap();
            builder.place("a", "Y", "a", "", 1, Some(0.0)).unwrap();
            builder.build().unwrap()
        };
        assert!(Dictionary::from_bytes(&bytes).is_ok());
        // The list "2 | flag, two analyses, 1 000 000, 0": a probability over one is refused
        let million = PROBABILITY_SCALE.to_le_bytes();
        let at = bytes
            .windows(4)
            .position(|window| window == million)
            .unwrap();
        let mut damaged = bytes.clone();
        damaged[at..at + 4].copy_from_slice(&(PROBABILITY_SCALE + 1).to_le_bytes());
        let error = Dictionary::from_bytes(&with_checksum(damaged)).unwrap_err();
        assert!(error.to_string().contains("probability"), "{error}");
    }

    pub(crate) fn bytes() -> Vec<u8> {
        let mut builder = Builder::new();
        builder.meta("{}");
        for &(lemma, pos, form, feats) in FORMS {
            builder.add_form(lemma, pos, form, feats).unwrap();
        }
        builder.build().unwrap()
    }

    #[test]
    fn features_in_conllu_order() {
        // By name without regard to case, the tags of the rules too: `Number` before `NumForm`
        let check = |dictionary: Dictionary| {
            for tag in dictionary.ud_tags() {
                let names: Vec<String> = tag.feats().map(|(name, _)| name.to_lowercase()).collect();
                assert!(names.is_sorted(), "{}", tag.feats_string());
            }
        };
        check(dictionary());
        #[cfg(feature = "bundled")]
        check(Dictionary::bundled().unwrap());
    }

    fn dictionary() -> Dictionary {
        Dictionary::from_bytes(&bytes()).unwrap()
    }

    fn summary(dictionary: &Dictionary, word: &str) -> Vec<(String, String, String)> {
        dictionary
            .analyze(word)
            .into_iter()
            .map(|analysis| {
                let tag = dictionary.ud_tag(analysis.ud).unwrap();
                (analysis.word, analysis.lemma, tag.pos().to_owned())
            })
            .collect()
    }

    fn lemmas(dictionary: &Dictionary, word: &str) -> Vec<String> {
        summary(dictionary, word)
            .into_iter()
            .map(|row| row.1)
            .collect()
    }

    #[test]
    fn analyses_of_a_form() {
        let dictionary = dictionary();
        let analyses = dictionary.analyze("casas");
        assert_eq!(analyses.len(), 2);
        assert_eq!(analyses[0].source.as_str(), "dictionary");
        assert_eq!(lemmas(&dictionary, "casas"), ["casa", "casar"]);
        let tag = dictionary.ud_tag(analyses[0].ud).unwrap();
        assert_eq!(tag.pos(), "NOUN");
        assert_eq!(tag.feats_string(), "Gender=Fem|Number=Plur");
        assert!(dictionary.analyze("qzx").is_empty());
        assert!(!dictionary.is_known("perro"));
        assert!(dictionary.analyze("").is_empty());
        assert!(!dictionary.is_empty());
        assert_eq!(dictionary.meta(), "{}");
    }

    #[test]
    fn lemma_with_another_accent() {
        let dictionary = dictionary();
        // The accent moves inside the suffix
        assert_eq!(lemmas(&dictionary, "camiones"), ["camión"]);
        assert_eq!(lemmas(&dictionary, "camión"), ["camión"]);
    }

    #[test]
    fn lemma_with_no_common_start() {
        let dictionary = dictionary();
        assert_eq!(
            summary(&dictionary, "me"),
            [("me".into(), "yo".into(), "PRON".into())]
        );
        // The lemma has no row of its own: it is no word form of the dictionary
        assert!(dictionary.analyze("yo").is_empty());
        assert!(dictionary.analyze("guión").is_empty());
        assert_eq!(lemmas(&dictionary, "guiones"), ["guión"]);
    }

    #[test]
    fn case_does_not_matter() {
        let dictionary = dictionary();
        // A word with a capital letter may be a name too, after the dictionary
        for word in ["Camión", "cAmIóN"] {
            assert_eq!(lemmas(&dictionary, word), ["camión", word], "{word}");
        }
        // A word in capitals may be a name too: after the dictionary for a long word, first for a
        // short one
        assert_eq!(lemmas(&dictionary, "CAMIÓN"), ["camión", "CAMIÓN"]);
        assert_eq!(lemmas(&dictionary, "PINGÜINO"), ["pingüino", "PINGÜINO"]);
        assert_eq!(lemmas(&dictionary, "AÑO"), ["AÑO", "año"]);
    }

    fn name_dictionary() -> Dictionary {
        let mut builder = Builder::new();
        builder.meta("{}");
        builder.add_form("madrid", "PROPN", "madrid", "").unwrap();
        builder.add_form("psoe", "PROPN", "psoe", "").unwrap();
        let noun = "Gender=Fem|Number=Sing";
        builder.add_form("rosa", "NOUN", "rosa", noun).unwrap();
        builder.add_form("rosa", "PROPN", "rosa", "").unwrap();
        let noun = "Gender=Masc|Number=Sing";
        builder.add_form("ángel", "NOUN", "ángel", noun).unwrap();
        builder.add_form("x", "PRON", "x", "").unwrap();
        builder.add_form("don", "NOUN", "d.", "").unwrap();
        Dictionary::from_bytes(&builder.build().unwrap()).unwrap()
    }

    #[test]
    fn names_keep_their_case() {
        let dictionary = name_dictionary();
        assert_eq!(lemmas(&dictionary, "Madrid"), ["Madrid"]);
        assert_eq!(lemmas(&dictionary, "madrid"), ["Madrid"]);
        assert_eq!(lemmas(&dictionary, "PSOE"), ["PSOE"]);
        let analysis = &dictionary.analyze("Madrid")[0];
        assert_eq!(dictionary.forms(analysis)[0].lemma, "Madrid");
        assert_eq!(dictionary.inflect(analysis, &[]).unwrap().lemma, "Madrid");
    }

    #[test]
    fn a_word_with_a_capital_may_be_a_name() {
        let dictionary = name_dictionary();
        // A word without accents that the dictionary has only with them is mostly a name
        assert_eq!(
            summary(&dictionary, "Angel"),
            [
                ("Angel".into(), "Angel".into(), "PROPN".into()),
                ("ángel".into(), "ángel".into(), "NOUN".into())
            ]
        );
        // An initial is mostly a name
        assert_eq!(summary(&dictionary, "D.")[0].2, "PROPN");
        // A word with a name of its spelling gets no other
        assert_eq!(dictionary.analyze("Rosa").len(), 2);
        // A lowercase word is a name rarely
        assert_eq!(summary(&dictionary, "rosa")[0].2, "NOUN");
    }

    #[test]
    fn a_roman_letter_of_the_dictionary_is_a_number_too() {
        let dictionary = name_dictionary();
        let poses: Vec<String> = (summary(&dictionary, "X").into_iter())
            .map(|row| row.2)
            .collect();
        assert_eq!(poses, ["NUM", "PRON", "PROPN"]);
        assert_eq!(summary(&dictionary, "x")[0].2, "PRON");
    }

    #[test]
    fn exact_spelling_wins_over_other_accents() {
        let dictionary = dictionary();
        assert_eq!(lemmas(&dictionary, "mas"), ["mas"]);
        assert_eq!(lemmas(&dictionary, "más"), ["más"]);
        assert_eq!(lemmas(&dictionary, "publico"), ["publicar"]);
        assert_eq!(lemmas(&dictionary, "público"), ["público"]);
        assert_eq!(summary(&dictionary, "publicó")[0].0, "publicó");
    }

    #[test]
    fn word_absent_as_written_looks_through_accents() {
        let dictionary = dictionary();
        // A lost accent
        assert_eq!(lemmas(&dictionary, "camion"), ["camión"]);
        assert_eq!(summary(&dictionary, "camion")[0].0, "camión");
        // A moved accent gives all spellings of the key
        // A grave accent is no accent of Spanish: the word is not in the dictionary
        assert!(
            (dictionary.analyze("pùblico").iter()).all(|analysis| analysis.source == Source::Guess)
        );
        let all: Vec<String> = summary(&dictionary, "públicó")
            .into_iter()
            .map(|row| row.0)
            .collect();
        assert_eq!(all, ["publico", "público", "publicó"]);
        assert!(dictionary.is_known("camion") && dictionary.is_known("públicó"));
        // A tilde is no accent: n and ñ are different letters
        assert_eq!(lemmas(&dictionary, "ano"), ["ano"]);
        assert_eq!(lemmas(&dictionary, "año"), ["año"]);
    }

    #[test]
    fn lost_diaeresis() {
        let dictionary = dictionary();
        assert_eq!(summary(&dictionary, "pinguino")[0].0, "pingüino");
        assert_eq!(summary(&dictionary, "averigué")[0].0, "averigüé");
        assert_eq!(summary(&dictionary, "averigue")[0].0, "averigüé");
        assert!(dictionary.is_known("pinguino"));
        // A form with `gú` is no `gü`
        assert_eq!(summary(&dictionary, "ambigúes")[0].0, "ambigúes");
        assert_eq!(summary(&dictionary, "ambigues")[0].0, "ambigúes");
        assert!(!dictionary.is_known("guerra"));
    }

    #[test]
    fn normalizes_queries() {
        let dictionary = dictionary();
        // Combining marks after the letter, also in capitals
        assert_eq!(lemmas(&dictionary, "camio\u{301}n"), ["camión"]);
        assert_eq!(lemmas(&dictionary, "CAMIO\u{301}N")[0], "camión");
        assert_eq!(lemmas(&dictionary, "an\u{303}o"), ["año"]);
        assert_eq!(lemmas(&dictionary, "pingu\u{308}ino"), ["pingüino"]);
        // Invisible characters
        assert_eq!(lemmas(&dictionary, "ca\u{AD}sa"), ["casa"]);
        assert_eq!(lemmas(&dictionary, "\u{FEFF}casa\u{200B}"), ["casa"]);
        assert!(dictionary.is_known("ca\u{AD}sa"));
        assert!(dictionary.analyze("\u{AD}").is_empty());
        // A mark on a letter that does not take it
        assert!(dictionary.analyze("cas\u{301}a").is_empty());
        assert_eq!(normalized("casa"), Cow::Borrowed("casa"));
        assert_eq!(normalized("camión"), Cow::Borrowed("camión"));
        // A lone mark has nothing to join
        assert_eq!(normalized("\u{301}a"), "\u{301}a");
    }

    #[test]
    fn counts_spellings_as_word_forms() {
        let dictionary = dictionary();
        // Every form of the table once: casa and casas, casar, camión, camiones, público, publico,
        // publicó, más, mas, pingüino, averigüé, ambigúes, año, ano, me, guiones, vi
        assert_eq!(dictionary.len(), 18);
    }

    #[test]
    fn long_spelling_goes_to_variants() {
        // An accent beyond the bits of the value of the automaton
        let word = format!("{}á", "a".repeat(45));
        let mut builder = Builder::new();
        builder.add_form(&word, "NOUN", &word, "").unwrap();
        let dictionary = Dictionary::from_bytes(&builder.build().unwrap()).unwrap();
        assert_eq!(lemmas(&dictionary, &word), std::slice::from_ref(&word));
        assert_eq!(dictionary.len(), 1);
    }

    #[test]
    fn empty_dictionary() {
        let bytes = Builder::new().build().unwrap();
        let dictionary = Dictionary::from_bytes(&bytes).unwrap();
        assert!(dictionary.is_empty());
        assert!(dictionary.analyze("casa").is_empty());
        assert!(!dictionary.is_known("casa"));
    }

    #[test]
    fn opens_files() {
        let dir = std::env::temp_dir().join(format!("esmorph-open-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("es.dict");
        std::fs::write(&path, bytes()).unwrap();
        let dictionary = Dictionary::open(&path).unwrap();
        assert_eq!(lemmas(&dictionary, "casas"), ["casa", "casar"]);
        // A copy and an analysis outlive the original
        let copy = dictionary.clone();
        drop(dictionary);
        assert_eq!(lemmas(&copy, "casas"), ["casa", "casar"]);
        assert!(matches!(
            Dictionary::open(dir.join("none")),
            Err(Error::Io(_))
        ));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn reads_static_data() {
        let data: &'static [u8] = Box::leak(bytes().into_boxed_slice());
        let dictionary = Dictionary::from_static(data).unwrap();
        assert_eq!(lemmas(&dictionary, "casas"), ["casa", "casar"]);
        assert!(Dictionary::from_static(&[]).is_err());
    }

    #[test]
    fn number_of_keys_is_verified() {
        // The footer of an automaton: the number of keys, the address of the root, a checksum
        let clean = bytes();
        let at = format::layout(&clean, true).unwrap().words.end - 20;
        let keys = u64::from_le_bytes(clean[at..at + 8].try_into().unwrap());
        let with_keys = |keys: u64| {
            let mut data = clean.clone();
            data[at..at + 8].copy_from_slice(&keys.to_le_bytes());
            Dictionary::from_bytes(&with_checksum(data))
        };
        let dictionary = with_keys(keys).unwrap();
        assert_eq!(dictionary.words.len() as u64, keys);
        assert!(dictionary.verify().is_ok());
        let fewer = with_keys(keys - 1).unwrap();
        assert!(fewer.verify().is_err());
        assert!(with_keys(u64::MAX).is_err());
    }

    #[test]
    fn verified_automata_never_panic() {
        // A damaged automaton with the checksum written anew loads: `verify` rejects every one on
        // which the analysis of the words and of guessed words would panic
        let clean = bytes();
        let layout = format::layout(&clean, true).unwrap();
        let words: Vec<String> = (FORMS.iter())
            .flat_map(|&(_, _, form, _)| {
                [form.to_owned(), format!("zz{form}"), form.to_uppercase()]
            })
            .collect();
        let analyze = |dictionary: &Dictionary| {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                for word in &words {
                    let _ = dictionary.analyze(word);
                    let _ = dictionary.is_known(word);
                }
            }))
            .is_ok()
        };
        let (mut rejected, mut caught) = (0, 0);
        for at in layout.words.clone().chain(layout.guesses.clone()) {
            for flip in [0x01, 0x02, 0x04, 0x08, 0x10, 0x20, 0x40, 0x80, 0xFF] {
                let mut damaged = clean.clone();
                damaged[at] ^= flip;
                let Ok(dictionary) = Dictionary::from_bytes(&with_checksum(damaged)) else {
                    continue;
                };
                if dictionary.verify().is_err() {
                    rejected += 1;
                    caught += usize::from(!analyze(&dictionary));
                    continue;
                }
                assert!(analyze(&dictionary), "byte {at} ^ {flip:#04x}");
            }
        }
        assert!(caught > 0 && rejected >= caught, "{rejected} {caught}");
        assert!(Dictionary::from_bytes(&clean).unwrap().verify().is_ok());
    }

    #[test]
    fn damaged_files_never_panic() {
        // Any byte of any section, with the checksum written anew so the checks of the sections are
        // reached; the automata are left out: only the checksum catches damage in them
        let clean = bytes();
        let layout = format::layout(&clean, true).unwrap();
        let automata = |at: &usize| layout.words.contains(at) || layout.guesses.contains(at);
        for at in (16..clean.len()).filter(|at| !automata(at)) {
            for flip in [0x01, 0x10, 0xFF] {
                let mut damaged = clean.clone();
                damaged[at] ^= flip;
                let Ok(dictionary) = Dictionary::from_bytes(&with_checksum(damaged)) else {
                    continue;
                };
                for &(_, _, form, _) in FORMS {
                    let _ = dictionary.analyze(form);
                    let _ = dictionary.is_known(form);
                }
            }
        }
    }
}
