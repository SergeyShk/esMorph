//! Own words: a word inflects as a word of the dictionary, its sample

use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};
use std::sync::Arc;

use super::{Analysis, Dictionary, Entry, normalized};
use crate::Error;
use crate::alphabet::MAX_KEY;
use crate::format::{LEMMA_ONLY, SPOKEN};

/// Sample of an own word: a word of the dictionary in the same form as the own word (mostly the
/// lemma) and the UD part of speech if the sample has lexemes of several (`bajo` is ADJ, ADP, ADV,
/// NOUN and VERB)
///
/// Made from a word (`"bloguear".into()`) or a pair of a word and a part of speech
/// (`("bajo", "NOUN").into()`)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Sample<'a> {
    /// The word of the dictionary
    pub word: &'a str,
    /// The UD part of speech (`"VERB"`): needed if the sample has lexemes of several
    pub pos: Option<&'a str>,
}

impl<'a> From<&'a str> for Sample<'a> {
    fn from(word: &'a str) -> Self {
        Sample { word, pos: None }
    }
}

impl<'a> From<(&'a str, &'a str)> for Sample<'a> {
    fn from((word, pos): (&'a str, &'a str)) -> Self {
        Sample {
            word,
            pos: Some(pos),
        }
    }
}

/// Own words by the bytes of their keys
pub(super) type OwnWords = HashMap<Box<[u8]>, Vec<OwnForm>, BuildHasherDefault<KeyHasher>>;

/// A form of an own word: the accents of its key, its spelling and the form of a table
#[derive(Clone, PartialEq)]
pub(super) struct OwnForm {
    marks: u64,
    spelling: String,
    entry: Entry,
}

/// Hash of the keys of own words: they are looked for on every token, so it is faster than
/// SipHash; the owner of the dictionary sets the words, so crafted keys are no threat
#[derive(Clone, Default)]
pub(super) struct KeyHasher(u64);

impl Hasher for KeyHasher {
    fn write(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.0 = (self.0.rotate_left(5) ^ u64::from(byte)).wrapping_mul(0x517c_c1b7_2722_0a95);
        }
    }

    fn finish(&self) -> u64 {
        self.0
    }
}

impl Dictionary {
    /// The dictionary with own words: a word takes the forms of the lexemes of its sample with its
    /// own stem (`tiktokear` like `bloguear` gives `tiktokeo`, `tiktokeó`, `tiktokeando`) and
    /// analyses as a word of the dictionary. The word and the sample are taken as `analyze` takes a
    /// word. A sample that is a lemma gives its lexemes of that lemma; the lexemes of one part of
    /// speech are all taken. A sample may be an own word added before. A dictionary with and
    /// without own words: `dictionary.clone().with_words(...)`, the copy shares the mapped file
    ///
    /// ```
    /// # #[cfg(feature = "bundled")] {
    /// use esmorph::Dictionary;
    ///
    /// let dictionary = Dictionary::bundled()?.with_words([("tiktokear", "bloguear")])?;
    /// assert_eq!(dictionary.analyze("tiktokeamos")[0].lemma, "tiktokear");
    /// # }
    /// # Ok::<(), esmorph::Error>(())
    /// ```
    ///
    /// # Errors
    ///
    /// [`Error::Word`] if the sample is not in the dictionary, has lexemes of several parts of
    /// speech and none is given, the word does not inflect as the sample (the sample has another
    /// ending or the word has no stem), or the word is no token: it has a space or a control
    /// character
    pub fn with_words<'a, S: Into<Sample<'a>>>(
        mut self,
        words: impl IntoIterator<Item = (&'a str, S)>,
    ) -> Result<Self, Error> {
        for (word, sample) in words {
            let forms = self.sample_forms(word, sample.into())?;
            let own = Arc::make_mut(&mut self.own_words);
            for (spelling, entry) in forms {
                // Letters that the dictionary has not take free bytes of the alphabet of the copy
                if !self.alphabet.extend(&spelling) {
                    return Err(Error::Word(format!(
                        "the word {spelling:?} has too many letters that the dictionary has not"
                    )));
                }
                let Some(key) = self.alphabet.input_key(&spelling) else {
                    return Err(Error::Word(format!(
                        "the word {spelling:?} is longer than {MAX_KEY} letters"
                    )));
                };
                let form = OwnForm {
                    marks: key.marks,
                    spelling,
                    entry,
                };
                let forms = own.entry(key.bytes().into()).or_default();
                if !forms.contains(&form) {
                    forms.push(form);
                }
            }
        }
        Ok(self)
    }

    /// Forms of the word `word` like the sample: the spelling and the form of the table of each
    /// form of the lexemes of the sample, without the spoken spellings
    fn sample_forms(&self, word: &str, sample: Sample<'_>) -> Result<Vec<(String, Entry)>, Error> {
        let normal = |text: &str| normalized(text).to_lowercase();
        let (word, like) = (normal(word), normal(sample.word));
        if word.is_empty() || word.chars().any(|c| c.is_whitespace() || c.is_control()) {
            return Err(Error::Word(format!(
                "the own word {word:?} is empty or has a space or a control character: no token \
                 of a text is such"
            )));
        }
        let pos = |analysis: &Analysis| self.ud.get(analysis.ud).map_or("", |tag| tag.pos());
        let mut analyses: Vec<Analysis> = (self.found(&like).into_iter())
            .filter(|analysis| analysis.entry.is_some())
            .collect();
        if analyses.is_empty() {
            return Err(Error::Word(format!(
                "the sample {like:?} is not in the dictionary"
            )));
        }
        // A sample that is a lemma gives its lexemes of that lemma: `bajo` of `bajar` is no sample
        // of a noun
        if analyses.iter().any(|analysis| analysis.lemma == like) {
            analyses.retain(|analysis| analysis.lemma == like);
        }
        let mut found: Vec<&str> = analyses.iter().map(pos).collect();
        found.sort_unstable();
        found.dedup();
        match sample.pos {
            Some(wanted) => {
                analyses.retain(|analysis| pos(analysis) == wanted);
                if analyses.is_empty() {
                    return Err(Error::Word(format!(
                        "the sample {like:?} has no lexemes of {wanted}, only of {}",
                        found.join(", ")
                    )));
                }
            }
            None if found.len() > 1 => {
                return Err(Error::Word(format!(
                    "the sample {like:?} has lexemes of several parts of speech ({}): give the \
                     part of speech",
                    found.join(", ")
                )));
            }
            None => {}
        }
        let mut forms = Vec::new();
        let mut lexemes: Vec<(u16, &str)> = Vec::new();
        for analysis in &analyses {
            let Some(entry) = analysis.entry else {
                continue;
            };
            let Some(lexeme) = self
                .lexeme(entry, &word)
                .filter(|lexeme| !lexeme.stem.is_empty())
            else {
                continue;
            };
            if lexemes.contains(&(entry.table, lexeme.stem)) {
                continue;
            }
            lexemes.push((entry.table, lexeme.stem));
            for form in 0..lexeme.cells.len() {
                if lexeme.cells[form] & (LEMMA_ONLY | SPOKEN) != 0 {
                    continue;
                }
                if let Some(spelling) = lexeme.spell(&self.suffixes, form) {
                    let entry = Entry {
                        table: entry.table,
                        form: form as u16,
                    };
                    forms.push((spelling, entry));
                }
            }
        }
        if forms.is_empty() {
            return Err(Error::Word(format!(
                "{word:?} does not inflect as {like:?}: the word and the sample need the same form \
                 and ending, and the word a stem"
            )));
        }
        Ok(forms)
    }

    /// The analyses of a word with its own words: an own word of the exact spelling, or of any
    /// spelling of the key if neither the dictionary nor the own words have the exact one, as the
    /// spellings of the dictionary. An own analysis takes the mean share of the analyses of the
    /// dictionary and goes after them on equal shares; one that the dictionary has is not repeated
    pub(super) fn with_own(
        &self,
        word: &str,
        mut analyses: Vec<Analysis>,
        exact: bool,
    ) -> (Vec<Analysis>, bool) {
        let Some(key) = self.alphabet.input_key(word) else {
            return (analyses, exact);
        };
        let Some(forms) = self.own_words.get(key.bytes()) else {
            return (analyses, exact);
        };
        let own_exact = forms.iter().any(|own| own.marks == key.marks);
        // The analyses of the dictionary with other accents give way to the exact own word
        if own_exact && !exact {
            analyses.clear();
        }
        let exact = exact || own_exact;
        let share = 1.0 / analyses.len().max(1) as f64;
        let known = analyses.len();
        for own in forms.iter().filter(|own| !exact || own.marks == key.marks) {
            let Some(analysis) = self.analysis(own.spelling.clone(), own.entry, share) else {
                continue;
            };
            let repeated = analyses[..known].iter().any(|other| {
                other.word == analysis.word
                    && other.lemma == analysis.lemma
                    && other.ud == analysis.ud
            });
            if !repeated {
                analyses.push(analysis);
            }
        }
        let sum: f64 = analyses.iter().map(|analysis| analysis.prob).sum();
        if sum > 0.0 {
            for analysis in &mut analyses {
                analysis.prob /= sum;
            }
        }
        analyses.sort_by(|a, b| b.prob.total_cmp(&a.prob));
        (analyses, exact)
    }

    /// Whether an own word is spelled as `word` with any accents
    pub(super) fn is_own(&self, word: &str) -> bool {
        !self.own_words.is_empty()
            && (self.alphabet.input_key(word))
                .is_some_and(|key| self.own_words.contains_key(key.bytes()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Source;
    use crate::builder::Builder;

    fn dictionary() -> Dictionary {
        let mut builder = Builder::new();
        builder.meta("{}");
        let forms = [
            ("bloguear", "VERB", "bloguear", "VerbForm=Inf"),
            (
                "bloguear",
                "VERB",
                "blogueo",
                "Mood=Ind|Number=Sing|Person=1|Tense=Pres|VerbForm=Fin",
            ),
            (
                "bloguear",
                "VERB",
                "blogueó",
                "Mood=Ind|Number=Sing|Person=3|Tense=Past|VerbForm=Fin",
            ),
            ("bajo", "ADJ", "bajo", "Gender=Masc|Number=Sing"),
            ("bajo", "ADJ", "bajos", "Gender=Masc|Number=Plur"),
            ("bajo", "NOUN", "bajo", "Gender=Masc|Number=Sing"),
            ("bajo", "NOUN", "bajos", "Gender=Masc|Number=Plur"),
            ("casa", "NOUN", "casa", "Gender=Fem|Number=Sing"),
            ("casa", "NOUN", "casas", "Gender=Fem|Number=Plur"),
        ];
        for (lemma, pos, form, feats) in forms {
            builder.add_form(lemma, pos, form, feats).unwrap();
        }
        Dictionary::from_bytes(&builder.build().unwrap()).unwrap()
    }

    fn first(dictionary: &Dictionary, word: &str) -> (String, String) {
        let analysis = dictionary.analyze(word).remove(0);
        (analysis.word, analysis.lemma)
    }

    #[test]
    fn an_own_word_inflects_as_its_sample() {
        let base = dictionary();
        let dictionary = base.clone().with_words([("tuitear", "bloguear")]).unwrap();
        assert_eq!(
            first(&dictionary, "tuiteó"),
            ("tuiteó".into(), "tuitear".into())
        );
        assert_eq!(
            first(&dictionary, "TUITEO"),
            ("tuiteo".into(), "tuitear".into())
        );
        let analysis = dictionary.analyze("tuiteo").remove(0);
        assert_eq!(analysis.source, Source::Dictionary);
        let forms: Vec<String> = (dictionary.forms(&analysis).into_iter())
            .map(|form| form.word)
            .collect();
        assert_eq!(forms, ["tuitear", "tuiteo", "tuiteó"]);
        let past = dictionary.inflect(&analysis, &[("Tense", "Past")]).unwrap();
        assert_eq!(past.word, "tuiteó");
        // A verb with clitics and a word without its accent, as words of the dictionary
        assert_eq!(dictionary.analyze("tuitearlo")[0].words, ["tuitear", "lo"]);
        assert_eq!(
            first(&dictionary, "tuiteo"),
            ("tuiteo".into(), "tuitear".into())
        );
        assert!(dictionary.is_known("tuitearlo") && dictionary.is_known("tuiteó"));
        // The copy without own words does not know them
        assert!(!base.is_known("tuiteó"));
        assert!(
            (base.analyze("tuiteó").iter()).all(|analysis| analysis.source != Source::Dictionary)
        );
    }

    #[test]
    fn an_own_word_with_letters_out_of_the_dictionary() {
        let dictionary = dictionary().with_words([("wikear", "bloguear")]).unwrap();
        assert_eq!(
            first(&dictionary, "wikeó"),
            ("wikeó".into(), "wikear".into())
        );
        // A sample may be an own word
        let dictionary = dictionary.with_words([("zapear", "wikear")]).unwrap();
        assert_eq!(
            first(&dictionary, "zapeo"),
            ("zapeo".into(), "zapear".into())
        );
    }

    #[test]
    fn an_own_word_next_to_a_word_of_the_dictionary() {
        let dictionary = dictionary()
            .with_words([("casa", ("bajo", "ADJ"))])
            .unwrap();
        let analyses = dictionary.analyze("casa");
        assert_eq!(analyses.len(), 2);
        assert_eq!(analyses[0].lemma, "casa");
        assert_eq!(analyses[0].prob, analyses[1].prob);
        // The same analysis is not repeated
        let again = dictionary.with_words([("casa", ("bajo", "ADJ"))]).unwrap();
        assert_eq!(again.analyze("casa").len(), 2);
    }

    #[test]
    fn wrong_own_words() {
        let dictionary = dictionary();
        for (word, sample, wanted) in [
            ("tuitear", Sample::from("qzx"), "not in the dictionary"),
            ("bajo", Sample::from("bajo"), "several parts of speech"),
            ("cajo", Sample::from(("bajo", "VERB")), "no lexemes of VERB"),
            ("tuiteo", Sample::from("bloguear"), "does not inflect"),
            ("ar", Sample::from("bloguear"), "does not inflect"),
            ("tui tear", Sample::from("bloguear"), "no token"),
            ("", Sample::from("bloguear"), "no token"),
        ] {
            let Err(error) = dictionary.clone().with_words([(word, sample)]) else {
                panic!("{word:?} like {sample:?} is no error");
            };
            assert!(matches!(error, Error::Word(_)), "{error}");
            assert!(error.to_string().contains(wanted), "{error}");
        }
        let cajo = dictionary.with_words([("cajo", ("bajo", "NOUN"))]).unwrap();
        assert_eq!(first(&cajo, "cajos"), ("cajos".into(), "cajo".into()));
    }
}
