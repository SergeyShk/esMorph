use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, HashMap};

use fst::MapBuilder;

use crate::Error;
use crate::alphabet::{Alphabet, MAX_KEY, mark_order};
use crate::format::{
    self, HAS_PROBABILITIES, LEMMA_ONLY, MAX_ENDING, MAX_LIST, MAX_SPELLINGS, PROBABILITY_SCALE,
    Sections, VARIANT, WordValue,
};

/// Parts of speech whose forms guess unknown words: the closed classes are in the dictionary
const GUESSED: [&str; 4] = ["NOUN", "ADJ", "VERB", "ADV"];
/// A table of fewer lexemes is more an exception than a sample for guessing
const MIN_TABLE_LEXEMES: usize = 3;
/// An ending that fewer word forms of the dictionary have is a mark of a word, not a sample
const MIN_ENDING_COUNT: u32 = 3;
/// How many of the most frequent forms of tables an ending keeps for guessing
const MAX_GUESSES: usize = 10;

/// Builder of a dictionary from the forms of lexemes
///
/// A lexeme is the forms of one lemma and part of speech. The longest common start of its forms is
/// its stem and the rest are the suffixes of a paradigm; lexemes with one paradigm and the same tags
/// share an inflection table. The forms of a table go in the order of the paradigm: the lemma, then
/// the main forms by their features (`VerbForm`, `Mood`, `Tense`, `Number`, `Person`, `Gender`...),
/// then the variants in the same order. An analysis of a word form refers to a table and a form.
/// The result depends only on the added forms, not on their order
#[derive(Default)]
pub struct Builder {
    meta: String,
    /// Forms of a lexeme: the form and its normalized features, and what is known of it
    lexemes: BTreeMap<(String, String), BTreeMap<(String, String), Form>>,
    /// How often texts have a word form
    frequencies: HashMap<String, u32>,
    /// How often texts have a part of speech with features
    tag_frequencies: HashMap<(String, String), u32>,
}

/// Place of an analysis in the list of its word form
#[derive(Clone, Copy, Default)]
struct Place {
    number: u32,
    probability: Option<u32>,
}

/// An added form: its place among the analyses of its word, whether it is a variant and how
/// often texts have it
#[derive(Clone, Copy, Default)]
struct Form {
    place: Place,
    variant: bool,
    count: u32,
}

/// A form of a lexeme in its table: its word, features and UD tag, its flags and count
struct Row<'a> {
    word: &'a str,
    feats: &'a str,
    ud: u16,
    place: Place,
    flags: u32,
    count: u32,
}

impl<'a> Row<'a> {
    /// Place in a table: the main forms before the variants, then by the features, the more
    /// frequent of equal forms first, then in the order of letters
    fn order(
        &self,
    ) -> (
        u32,
        ([usize; FEATURE_ORDER.len()], &'a str),
        Reverse<u32>,
        &'a str,
    ) {
        (
            self.flags,
            paradigm_key(self.feats),
            Reverse(self.count),
            self.word,
        )
    }
}

/// Order of the values of features in a paradigm: a form without a feature goes first, an unknown
/// value after the known ones
const FEATURE_ORDER: [(&str, &[&str]); 9] = [
    ("VerbForm", &["Inf", "Ger", "Part", "Fin"]),
    ("Mood", &["Ind", "Cnd", "Sub", "Imp"]),
    ("Tense", &["Pres", "Imp", "Past", "Fut"]),
    ("Number", &["Sing", "Plur"]),
    ("Person", &["1", "2", "3"]),
    ("Gender", &["Masc", "Fem"]),
    ("Case", &["Nom", "Acc", "Dat"]),
    ("PrepCase", &["Npr", "Pre"]),
    ("Reflex", &["Yes"]),
];

/// An analysis of a word form: `(table << 16) | form` and its place
struct Listed {
    record: u32,
    place: Place,
}

impl Builder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Description of the source of the dictionary, kept in the file as it is
    pub fn meta(&mut self, meta: impl Into<String>) -> &mut Self {
        self.meta = meta.into();
        self
    }

    /// Adds a form of a lexeme: words are in lower case, `feats` in CoNLL-U notation in any order
    /// (`Gender=Fem|Number=Sing`, an empty string for none); the same form is added once
    pub fn add_form(
        &mut self,
        lemma: &str,
        pos: &str,
        form: &str,
        feats: &str,
    ) -> Result<(), Error> {
        for word in [lemma, form] {
            if word.is_empty() || word.contains(['\0', '\t', '\n']) {
                return Err(Error::Build(format!("wrong word {word:?}")));
            }
            if word.chars().flat_map(char::to_lowercase).ne(word.chars()) {
                return Err(Error::Build(format!("word {word:?} is not in lower case")));
            }
        }
        if pos.is_empty() || pos.contains(['\t', '\n']) {
            return Err(Error::Build(format!("wrong part of speech {pos:?}")));
        }
        let feats = normalized(feats)?;
        self.lexemes
            .entry((lemma.to_owned(), pos.to_owned()))
            .or_default()
            .entry((form.to_owned(), feats))
            .or_default();
        Ok(())
    }

    /// Sets the place of an added form among the analyses of its word: lower places go first, and
    /// equal places stay in the order of lexemes. A list with a probability keeps them for all its
    /// analyses; the others have zero
    pub fn place(
        &mut self,
        lemma: &str,
        pos: &str,
        form: &str,
        feats: &str,
        number: u32,
        probability: Option<f64>,
    ) -> Result<(), Error> {
        let feats = normalized(feats)?;
        let place = &mut self.form(lemma, pos, form, feats)?.place;
        *place = Place {
            number,
            probability: probability
                .map(|value| {
                    if (0.0..=1.0).contains(&value) {
                        // An analysis that exists is never impossible
                        Ok(((value * f64::from(PROBABILITY_SCALE)).round() as u32).max(1))
                    } else {
                        Err(Error::Build(format!(
                            "wrong probability {value} of form {form:?}"
                        )))
                    }
                })
                .transpose()?,
        };
        Ok(())
    }

    /// Marks an added form as a variant of another form with the same features: `hablás` of
    /// `hablas`, `fablar` of `hablar`. The analyses of the form stay the same, while inflection
    /// gives a variant only from a variant
    pub fn mark_variant(
        &mut self,
        lemma: &str,
        pos: &str,
        form: &str,
        feats: &str,
    ) -> Result<(), Error> {
        let feats = normalized(feats)?;
        self.form(lemma, pos, form, feats)?.variant = true;
        Ok(())
    }

    /// Sets how often texts have an added form with its lemma and features: of the forms of a
    /// lexeme with the same features the more frequent goes first, as `hay` before `ha` in the verb
    /// `haber`; forms without a count go in the order of letters
    pub fn set_count(
        &mut self,
        lemma: &str,
        pos: &str,
        form: &str,
        feats: &str,
        count: u32,
    ) -> Result<(), Error> {
        let feats = normalized(feats)?;
        self.form(lemma, pos, form, feats)?.count = count;
        Ok(())
    }

    fn form(
        &mut self,
        lemma: &str,
        pos: &str,
        form: &str,
        feats: String,
    ) -> Result<&mut Form, Error> {
        self.lexemes
            .get_mut(&(lemma.to_owned(), pos.to_owned()))
            .and_then(|forms| forms.get_mut(&(form.to_owned(), feats)))
            .ok_or_else(|| Error::Build(format!("no form {form:?} of lexeme {lemma:?} {pos}")))
    }

    /// Sets how often texts have a word form. Spellings of one key share a query that lost its
    /// accents by this number and the number of their analyses: without it every analysis of the
    /// key has an equal share
    pub fn set_frequency(&mut self, form: &str, frequency: u32) {
        self.frequencies.insert(form.to_owned(), frequency);
    }

    /// Sets how often texts have a part of speech with features: a form of a table guesses
    /// unknown words by the number of its word forms with an ending times this number and one
    pub fn set_tag_frequency(
        &mut self,
        pos: &str,
        feats: &str,
        frequency: u32,
    ) -> Result<(), Error> {
        let feats = normalized(feats)?;
        self.tag_frequencies
            .insert((pos.to_owned(), feats), frequency);
        Ok(())
    }

    /// Builds the dictionary in the binary format
    pub fn build(&self) -> Result<Vec<u8>, Error> {
        let mut tags = Interned::default();
        let mut suffixes = Interned::default();
        let mut paradigms: Interned<Vec<u32>> = Interned::default();
        let mut tables: Interned<(u32, Vec<u32>)> = Interned::default();
        let mut words: BTreeMap<&str, Vec<Listed>> = BTreeMap::new();
        // Forms of the open classes that may guess unknown words, and the lexemes of every table
        let mut samples: Vec<(&str, u32, u64)> = Vec::new();
        let mut lexemes_of: HashMap<u32, usize> = HashMap::new();
        for ((lemma, pos), forms) in &self.lexemes {
            let mut rows = Vec::with_capacity(forms.len() + 1);
            for ((form, feats), info) in forms {
                rows.push(Row {
                    word: form,
                    feats,
                    ud: tags.number((pos.clone(), feats.clone()), "UD tags", u16::MAX)? as u16,
                    place: info.place,
                    flags: if info.variant { VARIANT } else { 0 },
                    count: info.count,
                });
            }
            rows.sort_by(|a, b| a.order().cmp(&b.order()));
            // The lemma is the first form of a table; a lemma that is no form of its lexeme stays a
            // form only for analyses
            match rows.iter().position(|row| row.word == lemma) {
                Some(at) => {
                    let row = rows.remove(at);
                    rows.insert(0, row);
                }
                None => rows.insert(
                    0,
                    Row {
                        word: lemma,
                        feats: "",
                        ud: tags.number((pos.clone(), String::new()), "UD tags", u16::MAX)? as u16,
                        place: Place::default(),
                        flags: LEMMA_ONLY,
                        count: 0,
                    },
                ),
            }
            if rows.len() > 1 << 16 {
                return Err(Error::Build(format!("lexeme {lemma:?} has too many forms")));
            }
            let stem = common_start(rows.iter().map(|row| row.word));
            let mut paradigm = Vec::with_capacity(rows.len());
            for row in &rows {
                let suffix = row.word[stem..].to_owned();
                paradigm.push(suffixes.number(suffix, "suffixes", u32::MAX)?);
            }
            let paradigm = paradigms.number(paradigm, "paradigms", u32::MAX)?;
            let cells = rows
                .iter()
                .map(|row| row.flags | u32::from(row.ud))
                .collect();
            let table = tables.number((paradigm, cells), "inflection tables", u16::MAX)?;
            *lexemes_of.entry(table).or_default() += 1;
            for (form, row) in rows.iter().enumerate() {
                if row.flags & LEMMA_ONLY == 0 {
                    words.entry(row.word).or_default().push(Listed {
                        record: table << 16 | form as u32,
                        place: row.place,
                    });
                    if row.flags & VARIANT == 0 && GUESSED.contains(&pos.as_str()) {
                        let key = (pos.clone(), row.feats.to_owned());
                        let weight = self.tag_frequencies.get(&key).copied().unwrap_or(0);
                        samples.push((row.word, table << 16 | form as u32, u64::from(weight) + 1));
                    }
                }
            }
        }
        let (guesses, guess_records) = guess_index(&samples, &lexemes_of)?;

        let alphabet = alphabet(words.keys().copied());
        let letters = Alphabet::new(&alphabet).map_err(|err| match err {
            Error::Format(msg) => Error::Build(msg),
            err => err,
        })?;

        // Equal lists of analyses of different word forms are kept once, spellings of one key together
        let mut records = Vec::new();
        let mut offsets: HashMap<Vec<u32>, u32> = HashMap::new();
        let mut keys: BTreeMap<Vec<u8>, Vec<(u64, u32, u32)>> = BTreeMap::new();
        for (word, analyses) in &mut words {
            if analyses.len() > MAX_LIST {
                return Err(Error::Build(format!(
                    "word form {word:?} has more than {MAX_LIST} analyses"
                )));
            }
            let key = letters.spelling_key(word).ok_or_else(|| {
                Error::Build(format!(
                    "word form {word:?} is longer than {MAX_KEY} letters"
                ))
            })?;
            analyses.sort_by_key(|analysis| analysis.place.number);
            let frequency = self.frequencies.get(*word).copied().unwrap_or(0);
            let weight = frequency.saturating_add(analyses.len() as u32);
            let offset = *offsets.entry(list_of(analyses)).or_insert_with_key(|list| {
                let offset = records.len() as u32;
                records.extend_from_slice(list);
                offset
            });
            keys.entry(key.bytes().to_vec())
                .or_default()
                .push((key.marks, offset, weight));
        }

        let mut variants = Vec::new();
        let mut automaton = MapBuilder::memory();
        for (key, mut spellings) in keys {
            if spellings.len() > MAX_SPELLINGS {
                return Err(Error::Build(format!(
                    "word form {:?} has more than {MAX_SPELLINGS} spellings",
                    letters.decode(&key, 0)
                )));
            }
            spellings.sort_by(|a, b| mark_order(a.0, b.0));
            let single = match spellings[..] {
                [(marks, records, _)] => WordValue::Single { marks, records }.pack(),
                _ => None,
            };
            let value = match single {
                Some(value) => value,
                None => {
                    let offset = u32::try_from(variants.len())
                        .map_err(|_| Error::Build("too many spellings".into()))?;
                    variants.push(spellings.len() as u32);
                    for &(marks, records, weight) in &spellings {
                        variants.extend([marks as u32, (marks >> 32) as u32, records, weight]);
                    }
                    WordValue::Many(offset).pack().unwrap_or_default()
                }
            };
            automaton.insert(key, value).map_err(build_error)?;
        }

        let mut paradigm_offsets = vec![0u32];
        let mut paradigm_numbers = Vec::new();
        for paradigm in &paradigms.items {
            paradigm_numbers.extend_from_slice(paradigm);
            paradigm_offsets.push(paradigm_numbers.len() as u32);
        }
        let table_numbers = tables
            .items
            .iter()
            .flat_map(|(paradigm, cells)| std::iter::once(*paradigm).chain(cells.iter().copied()))
            .collect();
        let (ud_strings, ud_tags) = ud_tables(&tags.items)?;
        Ok(format::write(&Sections {
            meta: self.meta.clone(),
            ud_strings,
            ud_tags,
            alphabet,
            suffixes: suffixes.items,
            paradigm_offsets,
            paradigms: paradigm_numbers,
            tables: table_numbers,
            words: automaton.into_inner().map_err(build_error)?,
            records,
            variants,
            guesses,
            guess_records,
        }))
    }
}

/// The index of guessing: an automaton of endings of word forms and the lists of the forms of
/// tables with each ending, the most frequent first, with their numbers of word forms. An ending
/// keeps only what its shorter ending does not tell: a list equal to it is left out
fn guess_index(
    samples: &[(&str, u32, u64)],
    lexemes_of: &HashMap<u32, usize>,
) -> Result<(Vec<u8>, Vec<u32>), Error> {
    // Number of word forms and their weight by the frequency of the tag
    let mut counts: BTreeMap<&str, HashMap<u32, (u32, u64)>> = BTreeMap::new();
    for &(word, record, weight) in samples {
        if lexemes_of.get(&(record >> 16)).copied().unwrap_or(0) < MIN_TABLE_LEXEMES {
            continue;
        }
        let starts: Vec<usize> = word.char_indices().map(|(at, _)| at).collect();
        // The stem of a guessed word keeps one letter at least
        for length in 1..=MAX_ENDING.min(starts.len().saturating_sub(1)) {
            let ending = &word[starts[starts.len() - length]..];
            let count = counts.entry(ending).or_default().entry(record).or_default();
            count.0 += 1;
            count.1 += weight;
        }
    }
    let lists: BTreeMap<&str, Vec<(u32, u32)>> = counts
        .into_iter()
        .filter(|(_, forms)| {
            forms.values().map(|&(count, _)| count).sum::<u32>() >= MIN_ENDING_COUNT
        })
        .map(|(ending, forms)| {
            let mut forms: Vec<(u32, u64)> = forms
                .into_iter()
                .map(|(record, (_, weight))| (record, weight))
                .collect();
            forms.sort_by_key(|&(record, weight)| (Reverse(weight), record));
            forms.truncate(MAX_GUESSES);
            // Weights above the numbers of the section go down together: the order stays
            let largest = forms.first().map_or(0, |&(_, weight)| weight);
            let scale = largest / u64::from(u32::MAX) + 1;
            let forms: Vec<(u32, u32)> = forms
                .into_iter()
                .map(|(record, weight)| {
                    let weight = u32::try_from(weight / scale).unwrap_or(u32::MAX);
                    (record, weight.max(1))
                })
                .collect();
            (ending, forms)
        })
        .collect();
    let mut records = Vec::new();
    let mut offsets: HashMap<&[(u32, u32)], u64> = HashMap::new();
    let mut automaton = MapBuilder::memory();
    for (ending, forms) in &lists {
        let shorter = ending.char_indices().nth(1).map(|(at, _)| &ending[at..]);
        if shorter.is_some_and(|shorter| lists.get(shorter) == Some(forms)) {
            continue;
        }
        let offset = *offsets.entry(forms).or_insert_with(|| {
            let offset = records.len() as u64;
            records.push(forms.len() as u32);
            for &(record, count) in forms.iter() {
                records.extend([record, count]);
            }
            offset
        });
        automaton
            .insert(ending.as_bytes(), offset)
            .map_err(build_error)?;
    }
    Ok((automaton.into_inner().map_err(build_error)?, records))
}

/// List of analyses as the section `records` keeps it
fn list_of(analyses: &[Listed]) -> Vec<u32> {
    let weighted = analyses
        .iter()
        .any(|analysis| analysis.place.probability.is_some());
    let flag = if weighted { HAS_PROBABILITIES } else { 0 };
    let mut list = Vec::with_capacity(1 + analyses.len() * if weighted { 2 } else { 1 });
    list.push(analyses.len() as u32 | flag);
    list.extend(analyses.iter().map(|analysis| analysis.record));
    if weighted {
        list.extend(
            analyses
                .iter()
                .map(|analysis| analysis.place.probability.unwrap_or(0)),
        );
    }
    list
}

/// Numbers of values in the order of their first appearance
struct Interned<T> {
    items: Vec<T>,
    numbers: HashMap<T, u32>,
}

impl<T> Default for Interned<T> {
    fn default() -> Self {
        Interned {
            items: Vec::new(),
            numbers: HashMap::new(),
        }
    }
}

impl<T: Clone + Eq + std::hash::Hash> Interned<T> {
    fn number(&mut self, value: T, what: &str, last: impl Into<u64>) -> Result<u32, Error> {
        if let Some(&number) = self.numbers.get(&value) {
            return Ok(number);
        }
        let number = self.items.len() as u64;
        let last = last.into();
        if number > last {
            return Err(Error::Build(format!("more than {} {what}", last + 1)));
        }
        self.items.push(value.clone());
        self.numbers.insert(value, number as u32);
        Ok(number as u32)
    }
}

/// Place of features in a paradigm: the ranks of the values of `FEATURE_ORDER`, then the features
fn paradigm_key(feats: &str) -> ([usize; FEATURE_ORDER.len()], &str) {
    let value = |name: &str| {
        feats
            .split('|')
            .find_map(|feat| feat.strip_prefix(name)?.strip_prefix('='))
    };
    let ranks = FEATURE_ORDER.map(|(name, values)| match value(name) {
        None => 0,
        Some(value) => values
            .iter()
            .position(|&known| known == value)
            .map_or(values.len() + 1, |at| at + 1),
    });
    (ranks, feats)
}

/// Features in the order of CoNLL-U: alphabetical without regard to case
fn normalized(feats: &str) -> Result<String, Error> {
    let invalid = || Error::Build(format!("wrong UD features {feats:?}"));
    let mut pairs = Vec::new();
    for feat in feats.split('|').filter(|_| !feats.is_empty()) {
        match feat.split_once('=') {
            Some((name, value)) if !name.is_empty() && !value.is_empty() => {
                pairs.push((name, value));
            }
            _ => return Err(invalid()),
        }
    }
    pairs.sort_by_cached_key(|&(name, _)| (name.to_ascii_lowercase(), name));
    if pairs.windows(2).any(|pair| pair[0].0 == pair[1].0) || feats.contains(['\t', '\n']) {
        return Err(invalid());
    }
    Ok(pairs
        .iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join("|"))
}

/// Length in bytes of the longest common start of the words, on a boundary of letters
fn common_start<'a>(mut words: impl Iterator<Item = &'a str>) -> usize {
    let Some(first) = words.next() else {
        return 0;
    };
    let mut len = first.len();
    for word in words {
        len = first[..len]
            .char_indices()
            .zip(word.chars())
            .find(|((_, a), b)| a != b)
            .map_or(len.min(word.len()), |((at, _), _)| at);
        // A shorter word ends inside the start only on a boundary of its own letters
        while !first.is_char_boundary(len) {
            len -= 1;
        }
    }
    len
}

/// Letters of the keys of word forms in order: accented vowels are written as plain ones
fn alphabet<'a>(words: impl Iterator<Item = &'a str>) -> String {
    let letters: BTreeSet<char> = words
        .flat_map(str::chars)
        .map(|letter| match letter {
            'á' => 'a',
            'é' => 'e',
            'í' => 'i',
            'ó' => 'o',
            'ú' => 'u',
            letter => letter,
        })
        .collect();
    letters.into_iter().collect()
}

/// Strings and UD tags as numbers of strings
fn ud_tables(tags: &[(String, String)]) -> Result<(Vec<String>, Vec<u16>), Error> {
    let mut strings = Vec::new();
    let mut numbers = HashMap::new();
    let mut number = |text: &str| -> Result<u16, Error> {
        if let Some(&number) = numbers.get(text) {
            return Ok(number);
        }
        let number = u16::try_from(strings.len())
            .map_err(|_| Error::Build("more than 65 536 strings in UD tags".into()))?;
        strings.push(text.to_owned());
        numbers.insert(text.to_owned(), number);
        Ok(number)
    };
    let mut out = Vec::new();
    for (pos, feats) in tags {
        let feats: Vec<(&str, &str)> = feats
            .split('|')
            .filter_map(|feat| feat.split_once('='))
            .collect();
        out.push(number(pos)?);
        out.push(feats.len() as u16);
        for (name, value) in feats {
            out.extend([number(name)?, number(value)?]);
        }
        out.push(0);
    }
    Ok((strings, out))
}

fn build_error(err: fst::Error) -> Error {
    Error::Build(err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks_forms() {
        let mut builder = Builder::new();
        assert!(
            builder
                .add_form("casa", "NOUN", "casas", "Number=Plur")
                .is_ok()
        );
        for (lemma, pos, form) in [
            ("", "NOUN", "a"),
            ("a", "NOUN", ""),
            ("a\tb", "NOUN", "a"),
            ("a", "", "a"),
            ("a", "NO\tUN", "a"),
            ("Casa", "NOUN", "casa"),
            ("casa", "NOUN", "CASAS"),
        ] {
            assert!(
                builder.add_form(lemma, pos, form, "").is_err(),
                "{lemma:?} {pos:?} {form:?}"
            );
        }
        for feats in [
            "Number",
            "=Plur",
            "Number=",
            "Number=Plur|",
            "Number=Plur|Number=Sing",
        ] {
            assert!(
                builder.add_form("a", "NOUN", "a", feats).is_err(),
                "{feats}"
            );
        }
    }

    #[test]
    fn places_are_checked() {
        let mut builder = Builder::new();
        builder.add_form("a", "X", "a", "").unwrap();
        assert!(builder.place("a", "X", "b", "", 0, None).is_err());
        assert!(builder.place("b", "X", "a", "", 0, None).is_err());
        assert!(builder.place("a", "X", "a", "", 0, Some(1.5)).is_err());
        assert!(builder.place("a", "X", "a", "", 0, Some(-0.5)).is_err());
        assert!(builder.place("a", "X", "a", "", 0, Some(f64::NAN)).is_err());
        assert!(builder.place("a", "X", "a", "", 0, Some(1.0)).is_ok());
    }

    #[test]
    fn features_in_conllu_order() {
        // Alphabetical without regard to case: Number comes before NumForm and NumType
        let ordered = "Gender=Fem|Number=Sing|NumForm=Word|NumType=Card";
        assert_eq!(normalized(ordered).unwrap(), ordered);
        assert_eq!(
            normalized("NumType=Card|NumForm=Word|Number=Sing|Gender=Fem").unwrap(),
            ordered
        );
        assert_eq!(normalized("").unwrap(), "");
    }

    #[test]
    fn common_start_stops_on_letters() {
        let start = |words: &[&str]| common_start(words.iter().copied());
        assert_eq!(start(&["casa", "casas"]), 4);
        assert_eq!(start(&["casas", "casa"]), 4);
        assert_eq!(start(&["camión", "camiones"]), 4);
        assert_eq!(start(&["él", "ella"]), 0);
        assert_eq!(start(&["año", "años", "añ"]), "añ".len());
        assert_eq!(start(&["ñ", "ñu"]), "ñ".len());
        // Letters that share their first byte differ on the second one
        assert_eq!(start(&["áa", "éa"]), 0);
        assert_eq!(start(&[]), 0);
    }

    #[test]
    fn independent_of_the_order_of_forms() {
        let forms = [
            ("casa", "NOUN", "casa", "Gender=Fem|Number=Sing"),
            ("casa", "NOUN", "casas", "Gender=Fem|Number=Plur"),
            ("casar", "VERB", "casas", "Person=2"),
            ("él", "PRON", "ella", "Gender=Fem"),
        ];
        let build = |forms: &[(&str, &str, &str, &str)]| {
            let mut builder = Builder::new();
            for &(lemma, pos, form, feats) in forms {
                builder.add_form(lemma, pos, form, feats).unwrap();
            }
            builder.build().unwrap()
        };
        let reversed: Vec<_> = forms.iter().rev().copied().collect();
        assert_eq!(build(&forms), build(&reversed));
    }

    #[test]
    fn limits_lists_the_dictionary_reads() {
        let mut builder = Builder::new();
        for number in 0..=MAX_LIST {
            builder
                .add_form(&format!("a{number}"), "NOUN", "a", "")
                .unwrap();
        }
        let err = builder.build().err().unwrap();
        assert!(err.to_string().contains("\"a\" has more than"), "{err}");

        let mut builder = Builder::new();
        let word = "a".repeat(MAX_KEY + 1);
        builder.add_form(&word, "NOUN", &word, "").unwrap();
        assert!(
            builder
                .build()
                .err()
                .unwrap()
                .to_string()
                .contains("longer")
        );

        let mut builder = Builder::new();
        // Nine spellings of one key
        for mask in 0..9u32 {
            let word: String = (0..4)
                .map(|at| if mask >> at & 1 == 1 { 'á' } else { 'a' })
                .collect();
            builder.add_form(&word, "NOUN", &word, "").unwrap();
        }
        let err = builder.build().err().unwrap();
        assert!(err.to_string().contains("\"aaaa\" has more than"), "{err}");
    }
}
