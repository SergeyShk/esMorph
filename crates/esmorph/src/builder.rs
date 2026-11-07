use std::collections::{BTreeMap, BTreeSet, HashMap};

use fst::MapBuilder;

use crate::Error;
use crate::alphabet::{Alphabet, MAX_KEY, mark_order};
use crate::format::{self, MAX_LIST, MAX_SPELLINGS, Sections, WordValue};

/// Builder of a dictionary from the forms of lexemes
///
/// A lexeme is the forms of one lemma and part of speech. The longest common start of its forms is
/// its stem and the rest are the suffixes of a paradigm; lexemes with one paradigm, one lemma form
/// and the same tags share an inflection table. An analysis of a word form refers to a table and
/// a form. The result depends only on the added forms, not on their order
#[derive(Default)]
pub struct Builder {
    meta: String,
    /// Forms of a lexeme: the form and its normalized features
    lexemes: BTreeMap<(String, String), BTreeSet<(String, String)>>,
}

/// A form of a lexeme in its table: its word, its UD tag and whether it is only the lemma
struct Row<'a> {
    word: &'a str,
    ud: u16,
    lemma_only: bool,
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
            .insert((form.to_owned(), feats));
        Ok(())
    }

    /// Builds the dictionary in the binary format
    pub fn build(&self) -> Result<Vec<u8>, Error> {
        let mut tags = Interned::default();
        let mut suffixes = Interned::default();
        let mut paradigms: Interned<Vec<u32>> = Interned::default();
        let mut tables: Interned<(u32, Vec<u32>)> = Interned::default();
        let mut words: BTreeMap<&str, Vec<u32>> = BTreeMap::new();
        for ((lemma, pos), forms) in &self.lexemes {
            let mut rows = Vec::with_capacity(forms.len() + 1);
            for (form, feats) in forms {
                rows.push(Row {
                    word: form,
                    ud: tags.number((pos.clone(), feats.clone()), "UD tags", u16::MAX)? as u16,
                    lemma_only: false,
                });
            }
            let lemma_form = match rows.iter().position(|row| row.word == lemma) {
                Some(at) => at,
                None => {
                    // The tables have no row of the lemma itself: it stays a form only for analyses
                    rows.push(Row {
                        word: lemma,
                        ud: tags.number((pos.clone(), String::new()), "UD tags", u16::MAX)? as u16,
                        lemma_only: true,
                    });
                    rows.len() - 1
                }
            };
            let lemma_form = u16::try_from(lemma_form)
                .ok()
                .filter(|_| rows.len() <= 1 << 16);
            let Some(lemma_form) = lemma_form else {
                return Err(Error::Build(format!("lexeme {lemma:?} has too many forms")));
            };
            let stem = common_start(rows.iter().map(|row| row.word));
            let mut paradigm = Vec::with_capacity(rows.len());
            for row in &rows {
                let suffix = row.word[stem..].to_owned();
                paradigm.push(suffixes.number(suffix, "suffixes", u32::MAX)?);
            }
            let paradigm = paradigms.number(paradigm, "paradigms", u32::MAX)?;
            let cells = rows
                .iter()
                .map(|row| u32::from(lemma_form) << 16 | u32::from(row.ud))
                .collect();
            let table = tables.number((paradigm, cells), "inflection tables", u16::MAX)?;
            for (form, row) in rows.iter().enumerate() {
                if !row.lemma_only {
                    words
                        .entry(row.word)
                        .or_default()
                        .push(table << 16 | form as u32);
                }
            }
        }

        let alphabet = alphabet(words.keys().copied());
        let letters = Alphabet::new(&alphabet).map_err(|err| match err {
            Error::Format(msg) => Error::Build(msg),
            err => err,
        })?;

        // Equal lists of analyses of different word forms are kept once, spellings of one key together
        let mut records = Vec::new();
        let mut offsets: HashMap<&[u32], u32> = HashMap::new();
        let mut keys: BTreeMap<Vec<u8>, Vec<(u64, u32)>> = BTreeMap::new();
        for (word, analyses) in &words {
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
            let offset = *offsets.entry(analyses).or_insert_with(|| {
                let offset = records.len() as u32;
                records.push(analyses.len() as u32);
                records.extend_from_slice(analyses);
                offset
            });
            keys.entry(key.bytes().to_vec())
                .or_default()
                .push((key.marks, offset));
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
                [(marks, records)] => WordValue::Single { marks, records }.pack(),
                _ => None,
            };
            let value = match single {
                Some(value) => value,
                None => {
                    let offset = u32::try_from(variants.len())
                        .map_err(|_| Error::Build("too many spellings".into()))?;
                    variants.push(spellings.len() as u32);
                    for &(marks, records) in &spellings {
                        variants.extend([marks as u32, (marks >> 32) as u32, records]);
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
        }))
    }
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
