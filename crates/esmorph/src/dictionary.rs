use std::borrow::Cow;
use std::ops::Range;
use std::path::Path;
use std::sync::Arc;

use fst::Map;

use crate::Error;
use crate::alphabet::{Alphabet, Key};
use crate::clitics;
use crate::format::{
    self, HAS_PROBABILITIES, LEMMA_ONLY, Layout, MAX_LIST, MAX_SPELLINGS, PROBABILITY_SCALE, Slice,
    Storage, Strings, UD_BITS, VARIANT, WordValue,
};
use crate::ud::{UdTag, UdTags};

/// Where an analysis comes from
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Source {
    /// The word form is in the dictionary
    Dictionary,
    /// The word form is a verb of the dictionary with enclitic pronouns
    Clitics,
}

impl Source {
    /// Name of the source: `dictionary`, `clitics`
    pub fn as_str(self) -> &'static str {
        match self {
            Source::Dictionary => "dictionary",
            Source::Clitics => "clitics",
        }
    }
}

/// Analysis of a word form in the dictionary: a form of an inflection table
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Entry {
    /// Number of the table
    pub table: u16,
    /// Number of the form in the paradigm of the table
    pub form: u16,
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

/// Dictionary of word forms
///
/// The dictionary file is mapped into memory: processes with one dictionary share its memory, and
/// a copy (`clone`) shares the mapped file. While a dictionary is open, its file must not be
/// overwritten in place
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
    word_count: usize,
    /// Which hosts of clitics a UD tag gives, by number
    hosts: Vec<Host>,
}

/// A form that takes clitics
#[derive(Clone, Copy, PartialEq, Eq)]
enum Host {
    No,
    /// An infinitive or a gerund
    NonFinite,
    Imperative,
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
    Many(&'a [[u32; 4]]),
}

impl Spellings<'_> {
    fn iter(&self) -> impl Iterator<Item = (u64, u32, u32)> {
        let (single, many) = match self {
            Spellings::Single { marks, records } => (Some((*marks, *records, 1)), &[][..]),
            Spellings::Many(variants) => (None, *variants),
        };
        single
            .into_iter()
            .chain(many.iter().map(|&spelling| unpack(spelling)))
    }

    /// The spelling of a query: the exact one and, if there is none, all spellings of the key.
    /// Spanish spells its accents: an exact word is not looked for under other accents, as
    /// `publico` is not `público`, while a word absent as written may have lost or moved one
    fn matching(&self, marks: u64) -> impl Iterator<Item = (u64, u32, u32)> {
        let exact = self.iter().any(|(own, ..)| own == marks);
        self.iter().filter(move |&(own, ..)| !exact || own == marks)
    }
}

fn unpack([low, high, offset, weight]: [u32; 4]) -> (u64, u32, u32) {
    (u64::from(low) | u64::from(high) << 32, offset, weight)
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
                return Self::from_storage(Storage::Mapped(map));
            }
        }
        Self::from_storage(format::read(file)?)
    }

    /// Reads a dictionary from the bytes of a dictionary file
    pub fn from_bytes(data: &[u8]) -> Result<Self, Error> {
        Self::from_storage(Storage::copy(data))
    }

    /// Reads a dictionary from the data of the program (`include_bytes!`): bytes aligned to 8 are
    /// read without a copy
    pub fn from_static(data: &'static [u8]) -> Result<Self, Error> {
        Self::from_storage(Storage::of_static(data))
    }

    /// The dictionary of esMorph from the crate `esmorph-dict-es`; it is checked on every call, so
    /// it is made once and shared between threads
    #[cfg(feature = "bundled")]
    #[cfg_attr(docsrs, doc(cfg(feature = "bundled")))]
    pub fn bundled() -> Result<Self, Error> {
        if esmorph_dict_es::DICTIONARY.is_empty() {
            return Err(Error::Format(
                "the crate esmorph-dict-es is built without a dictionary file".into(),
            ));
        }
        Self::from_static(esmorph_dict_es::DICTIONARY)
    }

    fn from_storage(storage: Storage) -> Result<Self, Error> {
        let layout = format::layout(storage.bytes())?;
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
            table_offsets: Vec::new(),
            word_count: 0,
            hosts: Vec::new(),
            layout,
            storage: Arc::clone(&storage),
        };
        dictionary.check_paradigms()?;
        dictionary.table_offsets = dictionary.check_tables()?;
        dictionary.check_records()?;
        dictionary.word_count = dictionary.words.len() + dictionary.check_variants()?;
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

    /// Analyses of a word form, the most probable first. Case does not matter. A word spelled as in
    /// the dictionary gives the analyses of that spelling; a word absent as written is looked for
    /// without its accents and with a lost diaeresis. A word absent from the dictionary may be a
    /// verb with clitics (`dámelo`). An unknown word has no analyses
    pub fn analyze(&self, word: &str) -> Vec<Analysis> {
        let word = normalized(word);
        let mut analyses = self.found(&word);
        if analyses.is_empty() {
            return self.with_clitics(&word);
        }
        if let Some(words) = clitics::contraction(&word) {
            for analysis in analyses.iter_mut().filter(|a| a.lemma == words[0]) {
                analysis.words = words.iter().map(|&word| word.to_owned()).collect();
            }
        }
        analyses
    }

    /// Whether `analyze` finds analyses of the word form: it is in the dictionary or a verb of the
    /// dictionary with clitics
    pub fn is_known(&self, word: &str) -> bool {
        let word = normalized(word);
        self.lookup(&word).is_some() || !self.with_clitics(&word).is_empty()
    }

    /// Analyses of a verb with clitics: of the first split into a host and clitics, the most
    /// clitics first, whose host is an infinitive, a gerund or an imperative of the dictionary
    fn with_clitics(&self, word: &str) -> Vec<Analysis> {
        let token = word.to_lowercase();
        // The accent that clitics put on a host marks its stressed vowel: in `piénselo` the host
        // is `piense`, not `píen`; without such a host the accent is not looked at
        let accent = token.chars().position(|c| "áéíóú".contains(c));
        let mut fallback = None;
        for split in clitics::splits(&token) {
            let (stressed, any) = self.host_analyses(&split, accent, fallback.is_none());
            if let Some(hosts) = stressed {
                return with_clitics(&token, &split, hosts);
            }
            if let Some(hosts) = any {
                if accent.is_none() {
                    return with_clitics(&token, &split, hosts);
                }
                fallback = Some((split, hosts));
            }
        }
        fallback
            .map(|(split, hosts)| with_clitics(&token, &split, hosts))
            .unwrap_or_default()
    }

    /// Analyses of the host of a split that is an infinitive, a gerund or an imperative of the
    /// dictionary: of the first spelling of the host stressed on the vowel `accent` and, if `any`
    /// is asked for, of the first spelling with any stress
    fn host_analyses(
        &self,
        split: &clitics::Split,
        accent: Option<usize>,
        any: bool,
    ) -> (Option<Vec<Analysis>>, Option<Vec<Analysis>>) {
        let first = split.clitics[0];
        let mut unstressed = None;
        for host in clitics::hosts(split.host, first) {
            let found: Vec<Analysis> = (self.found(&host).into_iter())
                .filter(|analysis| match self.hosts.get(usize::from(analysis.ud)) {
                    Some(Host::NonFinite) => true,
                    // `os` follows only the imperative of `vosotros`, which loses its `d`:
                    // `cuéntenos` is `cuente` + `nos`, not `cuenten` + `os`
                    Some(Host::Imperative) => first != "os" || host.ends_with('d'),
                    _ => false,
                })
                .collect();
            if found.is_empty() {
                continue;
            }
            let Some(at) = accent else {
                return (None, Some(found));
            };
            let stressed: Vec<Analysis> = (found.iter())
                .filter(|analysis| clitics::stressed(&analysis.word) == Some(at))
                .cloned()
                .collect();
            if !stressed.is_empty() {
                return (Some(stressed), unstressed);
            }
            if any && unstressed.is_none() {
                unstressed = Some(found);
            }
        }
        (None, unstressed)
    }

    /// Analyses of a word form in the dictionary
    fn found(&self, word: &str) -> Vec<Analysis> {
        let Some((key, spellings)) = self.lookup(word) else {
            return Vec::new();
        };
        let (count, total) = spellings
            .matching(key.marks)
            .fold((0, 0.0), |(count, total), (.., weight)| {
                (count + 1, total + f64::from(weight))
            });
        let mut analyses = Vec::new();
        for (marks, records, weight) in spellings.matching(key.marks) {
            let spelling = self.alphabet.spelling(&key, marks);
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
                if let Some(analysis) = self.analysis(&spelling, entry, share) {
                    analyses.push(analysis);
                }
            }
        }
        if count > 1 {
            analyses.sort_by(|a, b| b.prob.total_cmp(&a.prob));
        }
        analyses
    }

    /// The key of a word and its spellings: the key as written, then with a lost diaeresis restored
    fn lookup(&self, word: &str) -> Option<(Key, Spellings<'_>)> {
        let key = self.alphabet.input_key(word)?;
        if let Some(spellings) = self.spellings(&key) {
            return Some((key, spellings));
        }
        self.alphabet
            .diaeresis_keys(&key)
            .into_iter()
            .find_map(|key| {
                let spellings = self.spellings(&key)?;
                Some((key, spellings))
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
    fn analysis(&self, spelling: &str, entry: Entry, prob: f64) -> Option<Analysis> {
        let lexeme = self.lexeme(entry, spelling)?;
        Some(Analysis {
            word: spelling.to_owned(),
            lemma: lexeme.spell(&self.suffixes, 0)?,
            ud: lexeme.ud(usize::from(entry.form))?,
            prob,
            source: Source::Dictionary,
            entry: Some(entry),
            words: Vec::new(),
        })
    }

    /// Forms of the lexeme of an analysis in the order of its paradigm: the lemma, the main forms by
    /// their features, then the variants; the probability of each is one. A verb with clitics
    /// gives the forms of its verb, without clitics
    pub fn forms(&self, analysis: &Analysis) -> Vec<Analysis> {
        let Some(lexeme) = self.lexeme_of(analysis) else {
            return Vec::new();
        };
        (0..lexeme.cells.len())
            .filter(|&form| lexeme.cells[form] & LEMMA_ONLY == 0)
            .filter_map(|form| self.form(&lexeme, form))
            .collect()
    }

    /// The form of the lexeme of an analysis with the features `feats` ("name", "value"): of the
    /// forms that have them, the one that keeps most of the other features of the analysis, then
    /// a variant only for a variant, then the one in the place of the analysis among the forms
    /// with equal features, then the first in the paradigm. An infinitive and a gerund
    /// asked for features they have not become finite forms: `hablar` with `Tense=Past` is
    /// `hablé`. A form of a noun, an adjective, a determiner or a pronoun without a gender or a
    /// number takes any, though a form that has it goes first: `grande` with `Gender=Fem` is
    /// `grande`. A value of a form may list several (`Case=Acc,Nom`): any of them matches. A verb
    /// with clitics inflects as its verb, without clitics
    pub fn inflect(&self, analysis: &Analysis, feats: &[(&str, &str)]) -> Option<Analysis> {
        let lexeme = self.lexeme_of(analysis)?;
        let own_form = usize::from(analysis.entry?.form);
        let own = *lexeme.cells.get(own_form)?;
        let tag = self.ud.get(analysis.ud)?;
        let mut wanted: Vec<(&str, &str)> = tag.feats().collect();
        let mut want = |name, value| match wanted.iter_mut().find(|(known, _)| *known == name) {
            Some(feat) => feat.1 = value,
            None => wanted.push((name, value)),
        };
        if matches!(tag.feat("VerbForm"), Some("Inf" | "Ger"))
            && feats.iter().all(|&(name, _)| name != "VerbForm")
            && feats
                .iter()
                .any(|&(name, value)| !has(tag.feat(name), value))
        {
            want("VerbForm", "Fin");
        }
        for &(name, value) in feats {
            want(name, value);
        }
        // Forms of one tag go one after another; the place among them is kept: `hablase` with
        // `Person=3` is `hablase`, not `hablara`
        let rank = |form: usize| {
            lexeme.cells[..form]
                .iter()
                .rev()
                .take_while(|&&cell| cell == lexeme.cells[form])
                .count()
        };
        let own_rank = rank(own_form);
        let best = (0..lexeme.cells.len())
            .filter(|&form| lexeme.cells[form] & LEMMA_ONLY == 0)
            .filter_map(|form| {
                let tag = self.ud.get((lexeme.cells[form] & UD_BITS) as u16)?;
                let nominal =
                    matches!(tag.pos(), "NOUN" | "PROPN" | "ADJ" | "DET" | "PRON" | "NUM");
                let fits = feats.iter().all(|&(name, value)| {
                    has(tag.feat(name), value)
                        || (nominal
                            && matches!(name, "Gender" | "Number")
                            && tag.feat(name).is_none())
                });
                let missed = (wanted.iter())
                    .filter(|&&(name, value)| !has(tag.feat(name), value))
                    .count();
                let variant = (lexeme.cells[form] ^ own) & VARIANT != 0;
                fits.then_some(((missed, variant, rank(form) != own_rank, form), form))
            })
            .min()?
            .1;
        self.form(&lexeme, best)
    }

    /// Lexeme of an analysis: of its verb for a verb with clitics
    fn lexeme_of<'a>(&'a self, analysis: &'a Analysis) -> Option<Lexeme<'a>> {
        let word = match analysis.source {
            Source::Clitics => analysis.words.first()?,
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
    fn form(&self, lexeme: &Lexeme, form: usize) -> Option<Analysis> {
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
            source: Source::Dictionary,
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

    /// Lists of analyses exactly cover their section, are not longer than `MAX_LIST`, their tables
    /// and forms exist and are word forms, not only lemmas, and their probabilities are no more
    /// than one
    fn check_records(&self) -> Result<(), Error> {
        let (tables, offsets) = (self.tables(), self.paradigm_offsets());
        // Forms of every table: tables and paradigms are checked already
        let table_forms: Vec<&[u32]> = self
            .table_offsets
            .iter()
            .map(|&start| {
                let start = start as usize;
                let paradigm = tables[start] as usize;
                let count = (offsets[paradigm + 1] - offsets[paradigm]) as usize;
                &tables[start + 1..start + 1 + count]
            })
            .collect();
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
            if variants.is_empty()
                || variants.len() > MAX_SPELLINGS
                || variants
                    .iter()
                    .any(|&[_, _, offset, weight]| offset as usize >= records || weight == 0)
            {
                return invalid("a wrong list of spellings");
            }
            extra += variants.len() - 1;
            rest = after;
        }
        Ok(extra)
    }
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

/// Whether a value of a feature is `wanted` or lists it: `Acc,Nom` has `Acc`
fn has(value: Option<&str>, wanted: &str) -> bool {
    value.is_some_and(|value| value == wanted || value.split(',').any(|part| part == wanted))
}

/// Analyses of a token by the analyses of its host and its clitics
fn with_clitics(token: &str, split: &clitics::Split, hosts: Vec<Analysis>) -> Vec<Analysis> {
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
                source: Source::Clitics,
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
fn spelling_list(lists: &[u32]) -> Option<(&[[u32; 4]], &[u32])> {
    let (&count, rest) = lists.split_first()?;
    let count = usize::try_from(count)
        .ok()
        .filter(|&count| count <= MAX_LIST)?;
    let (list, rest) = rest.split_at_checked(4 * count)?;
    Some((list.as_chunks::<4>().0, rest))
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

/// A query in the terms of the dictionary: invisible characters are dropped, and a combining mark
/// joins its letter. Tokens of a text keep both
fn normalized(word: &str) -> Cow<'_, str> {
    if word.is_ascii()
        || !word
            .chars()
            .any(|c| is_invisible(c) || combined(' ', c).is_some())
    {
        return Cow::Borrowed(word);
    }
    let mut out = String::with_capacity(word.len());
    for c in word.chars() {
        if is_invisible(c) {
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

/// Soft hyphen, zero width characters and the byte order mark
fn is_invisible(c: char) -> bool {
    matches!(
        c,
        '\u{AD}' | '\u{200B}'..='\u{200D}' | '\u{2060}' | '\u{FEFF}'
    )
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
        Dictionary::from_bytes(&builder.build().unwrap()).unwrap()
    }

    #[test]
    fn a_verb_with_clitics_is_its_host() {
        let dictionary = clitic_dictionary();
        let analyses = dictionary.analyze("Dámelo");
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
            inflected(&dictionary, haber, &[("Tense", "Pres"), ("Person", "3")])
        };
        assert_eq!(build(&[]), "ha");
        assert_eq!(build(&[("hay", 7), ("ha", 2)]), "hay");
        assert_eq!(build(&[("ha", 2)]), "ha");
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
        let tables = format::layout(&clean).unwrap().tables;
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
        analysis.entry = None;
        assert!(dictionary.forms(&analysis).is_empty());
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
        assert!(dictionary.analyze("perro").is_empty());
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
        for word in ["CAMIÓN", "Camión", "cAmIóN"] {
            assert_eq!(lemmas(&dictionary, word), ["camión"], "{word}");
        }
        assert_eq!(lemmas(&dictionary, "AÑO"), ["año"]);
        assert_eq!(lemmas(&dictionary, "PINGÜINO"), ["pingüino"]);
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
        assert_eq!(lemmas(&dictionary, "pùblico"), Vec::<String>::new());
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
        assert_eq!(lemmas(&dictionary, "CAMIO\u{301}N"), ["camión"]);
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
        // publicó, más, mas, pingüino, averigüé, ambigúes, año, ano, me, guiones
        assert_eq!(dictionary.len(), 17);
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
    fn damaged_files_never_panic() {
        // Any byte of any section, with the checksum written anew so the checks of the sections are
        // reached; the automaton is left out: only the checksum catches damage in it
        let clean = bytes();
        let automaton = format::layout(&clean).unwrap().words;
        for at in (16..clean.len()).filter(|at| !automaton.contains(at)) {
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
