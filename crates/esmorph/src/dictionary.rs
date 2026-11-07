use std::borrow::Cow;
use std::ops::Range;
use std::path::Path;
use std::sync::Arc;

use fst::Map;

use crate::Error;
use crate::alphabet::{Alphabet, Key};
use crate::format::{self, Layout, MAX_LIST, MAX_SPELLINGS, Slice, Storage, Strings, WordValue};
use crate::ud::{UdTag, UdTags};

/// Where an analysis comes from
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Source {
    /// The word form is in the dictionary
    Dictionary,
}

impl Source {
    /// Name of the source: `dictionary`
    pub fn as_str(self) -> &'static str {
        match self {
            Source::Dictionary => "dictionary",
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
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Analysis {
    /// The word form as the dictionary spells it: with its accents
    pub word: String,
    /// The lemma
    pub lemma: String,
    /// Number of the UD tag for `Dictionary::ud_tag`
    pub ud: u16,
    /// Where the analysis comes from
    pub source: Source,
    /// The form of the inflection table
    pub entry: Option<Entry>,
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

/// Spellings of one key: places of the accents and the start of the list of analyses
enum Spellings<'a> {
    Single { marks: u64, records: u32 },
    Many(&'a [[u32; 3]]),
}

impl Spellings<'_> {
    fn iter(&self) -> impl Iterator<Item = (u64, u32)> {
        let (single, many) = match self {
            Spellings::Single { marks, records } => (Some((*marks, *records)), &[][..]),
            Spellings::Many(variants) => (None, *variants),
        };
        single
            .into_iter()
            .chain(many.iter().map(|&triple| unpack(triple)))
    }

    /// The spelling of a query: the exact one and, if there is none, all spellings of the key.
    /// Spanish spells its accents: an exact word is not looked for under other accents, as
    /// `publico` is not `público`, while a word absent as written may have lost or moved one
    fn matching(&self, marks: u64) -> impl Iterator<Item = (u64, u32)> {
        let exact = self.iter().any(|(own, _)| own == marks);
        self.iter().filter(move |&(own, _)| !exact || own == marks)
    }
}

fn unpack([low, high, offset]: [u32; 3]) -> (u64, u32) {
    (u64::from(low) | u64::from(high) << 32, offset)
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
            layout,
            storage: Arc::clone(&storage),
        };
        dictionary.check_paradigms()?;
        dictionary.table_offsets = dictionary.check_tables()?;
        dictionary.check_records()?;
        dictionary.word_count = dictionary.words.len() + dictionary.check_variants()?;
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

    /// Analyses of a word form. Case does not matter. A word spelled as in the dictionary gives the
    /// analyses of that spelling; a word absent as written is looked for without its accents and
    /// with a lost diaeresis. An unknown word has no analyses
    pub fn analyze(&self, word: &str) -> Vec<Analysis> {
        let word = normalized(word);
        let Some((key, spellings)) = self.lookup(&word) else {
            return Vec::new();
        };
        let mut analyses = Vec::new();
        for (marks, records) in spellings.matching(key.marks) {
            let spelling = self.alphabet.spelling(&key, marks);
            for &record in self.records_at(records).unwrap_or_default() {
                let entry = Entry {
                    table: (record >> 16) as u16,
                    form: record as u16,
                };
                if let Some(analysis) = self.analysis(&spelling, entry) {
                    analyses.push(analysis);
                }
            }
        }
        analyses
    }

    /// Whether `analyze` finds the word form in the dictionary
    pub fn is_known(&self, word: &str) -> bool {
        self.lookup(&normalized(word)).is_some()
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
                Spellings::Many(triples(lists)?.0)
            }
        })
    }

    /// Analysis of a spelling by a form of a table: the stem of the spelling and the suffix of the
    /// lemma form
    fn analysis(&self, spelling: &str, entry: Entry) -> Option<Analysis> {
        let start = *self.table_offsets.get(usize::from(entry.table))? as usize;
        let tables = self.tables();
        let paradigm = self.paradigm(*tables.get(start)?)?;
        let cell = *tables.get(start + 1 + usize::from(entry.form))?;
        let suffix = self
            .suffixes
            .get(*paradigm.get(usize::from(entry.form))? as usize)?;
        let stem = spelling.strip_suffix(suffix)?;
        let lemma_suffix = self
            .suffixes
            .get(*paradigm.get((cell >> 16) as usize)? as usize)?;
        let mut lemma = String::with_capacity(stem.len() + lemma_suffix.len());
        lemma.push_str(stem);
        lemma.push_str(lemma_suffix);
        Some(Analysis {
            word: spelling.to_owned(),
            lemma,
            ud: cell as u16,
            source: Source::Dictionary,
            entry: Some(entry),
        })
    }

    fn paradigm(&self, number: u32) -> Option<&[u32]> {
        let offsets = self.paradigm_offsets();
        let at = number as usize;
        let (start, end) = (*offsets.get(at)? as usize, *offsets.get(at + 1)? as usize);
        self.paradigms().get(start..end)
    }

    fn records_at(&self, offset: u32) -> Option<&[u32]> {
        list(self.records(), offset)
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
                (cell >> 16) as usize >= count || (cell & 0xFFFF) as usize >= self.ud.len()
            }) {
                return invalid("a table refers to a missing form or UD tag");
            }
            starts.push(start as u32);
            start += 1 + count;
        }
        Ok(starts)
    }

    /// Lists of analyses exactly cover their section, are not longer than `MAX_LIST`, and their
    /// tables and forms exist
    fn check_records(&self) -> Result<(), Error> {
        let (tables, offsets) = (self.tables(), self.paradigm_offsets());
        // Number of forms of every table: tables and paradigms are checked already
        let form_counts: Vec<usize> = self
            .table_offsets
            .iter()
            .map(|&start| {
                let paradigm = tables[start as usize] as usize;
                (offsets[paradigm + 1] - offsets[paradigm]) as usize
            })
            .collect();
        let lists = self.records();
        let invalid = |msg: &str| Err(Error::Format(msg.into()));
        let mut start = 0;
        while start < lists.len() {
            let count = lists[start] as usize;
            if count > MAX_LIST {
                return invalid("a list is longer than 255 items");
            }
            let end = start + 1 + count;
            let Some(items) = lists.get(start + 1..end) else {
                return invalid("a list is truncated");
            };
            let exists = |&record: &u32| {
                form_counts
                    .get((record >> 16) as usize)
                    .is_some_and(|&forms| ((record & 0xFFFF) as usize) < forms)
            };
            if !items.iter().all(exists) {
                return invalid("a list refers to a missing table or form");
            }
            start = end;
        }
        Ok(())
    }

    /// Number of word forms over the keys of the automaton: the keys with several spellings in the
    /// section `variants`; lengths and references to the lists of analyses are checked
    fn check_variants(&self) -> Result<usize, Error> {
        let invalid = |msg: &str| Err(Error::Format(msg.into()));
        let records = self.records().len();
        let mut extra = 0;
        let mut rest = self.variants();
        while !rest.is_empty() {
            let Some((variants, after)) = triples(rest) else {
                return invalid("a list of spellings is truncated");
            };
            if variants.is_empty()
                || variants.len() > MAX_SPELLINGS
                || variants
                    .iter()
                    .any(|&[_, _, offset]| offset as usize >= records)
            {
                return invalid("a wrong list of spellings");
            }
            extra += variants.len() - 1;
            rest = after;
        }
        Ok(extra)
    }
}

/// List "length, items" from the place `offset` of the section `lists`
fn list(lists: &[u32], offset: u32) -> Option<&[u32]> {
    let start = offset as usize + 1;
    let count = *lists.get(start - 1)? as usize;
    if count > MAX_LIST {
        return None;
    }
    lists.get(start..start + count)
}

/// List "length, triples of numbers" at the start of `lists` and the rest after it
fn triples(lists: &[u32]) -> Option<(&[[u32; 3]], &[u32])> {
    let (&count, rest) = lists.split_first()?;
    let count = usize::try_from(count)
        .ok()
        .filter(|&count| count <= MAX_LIST)?;
    let (list, rest) = rest.split_at_checked(3 * count)?;
    Some((list.as_chunks::<3>().0, rest))
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
