//! Own words: a word inflects as a word of the dictionary, its sample

use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};
use std::sync::Arc;

use super::{Analysis, Dictionary, Entry, Lexeme, analyses_at, normalized};
use crate::Error;
use crate::alphabet::{Key, MAX_KEY};
use crate::format::{LEMMA_ONLY, SPOKEN, VARIANT};

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

/// A form of an own word: the accents of its key, its spelling, the form of a table, and the share
/// and the place of that form among the analyses of the spelling of the sample
#[derive(Clone)]
pub(super) struct OwnForm {
    marks: u64,
    spelling: String,
    entry: Entry,
    share: f64,
    place: usize,
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
        // The analyses of the spellings of samples: words of one sample share them
        let mut samples: HashMap<String, Vec<Analysis>> = HashMap::new();
        for (word, sample) in words {
            let mut forms = Vec::new();
            for (spelling, entry, like) in self.sample_forms(word, sample.into())? {
                // The analyses of a spelling go in the order and by the shares of the sample: in
                // `blogueaba` the third person goes first
                let analyses = samples
                    .entry(like)
                    .or_insert_with_key(|like| self.spelled(like).0);
                let (share, place) = (analyses.iter().enumerate())
                    .find(|(_, analysis)| analysis.entry == Some(entry))
                    .map_or(
                        (1.0 / analyses.len().max(1) as f64, analyses.len()),
                        |(at, found)| (found.prob, at),
                    );
                forms.push((spelling, entry, share, place));
            }
            let own = Arc::make_mut(&mut self.own_words);
            for (spelling, entry, share, place) in forms {
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
                let forms = own.entry(key.bytes().into()).or_default();
                if !(forms.iter()).any(|known| known.entry == entry && known.spelling == spelling) {
                    forms.push(OwnForm {
                        marks: key.marks,
                        spelling,
                        entry,
                        share,
                        place,
                    });
                }
            }
        }
        Ok(self)
    }

    /// Forms of the word `word` like the sample: the spelling, the form of the table and the
    /// spelling of the sample of each form of the lexemes of the sample, without the spoken
    /// spellings and, but of a verb (`hablás` of `hablas`), without the variants: those of a noun
    /// are other spellings (`yoghurt` of `yogur`)
    fn sample_forms(
        &self,
        word: &str,
        sample: Sample<'_>,
    ) -> Result<Vec<(String, Entry, String)>, Error> {
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
        // The endings of the sample that the word needs, for the error
        let mut endings: Vec<&str> = Vec::new();
        for analysis in &analyses {
            let Some(entry) = analysis.entry else {
                continue;
            };
            let Some(sample_lexeme) = self.lexeme(entry, &analysis.word) else {
                continue;
            };
            if let Some(ending) = analysis.word.strip_prefix(sample_lexeme.stem)
                && !endings.contains(&ending)
            {
                endings.push(ending);
            }
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
            let skipped = match pos(analysis) {
                "VERB" | "AUX" => LEMMA_ONLY | SPOKEN,
                _ => LEMMA_ONLY | SPOKEN | VARIANT,
            };
            for form in 0..lexeme.cells.len() {
                if lexeme.cells[form] & skipped != 0 {
                    continue;
                }
                if let (Some(spelling), Some(like)) = (
                    lexeme.spell(&self.suffixes, form),
                    sample_lexeme.spell(&self.suffixes, form),
                ) {
                    let entry = Entry {
                        table: entry.table,
                        form: form as u16,
                    };
                    forms.push((spelling, entry, like));
                }
            }
        }
        if forms.is_empty() {
            let endings: Vec<String> = endings.iter().map(|ending| format!("{ending:?}")).collect();
            return Err(Error::Word(format!(
                "{word:?} does not inflect as {like:?}: the word needs the ending {} of the sample \
                 and a stem before it",
                endings.join(" or ")
            )));
        }
        Ok(forms)
    }

    /// The analyses of a word with its own words: an own word of the exact spelling, or of any
    /// spelling of the key if neither the dictionary nor the own words have the exact one, as the
    /// spellings of the dictionary; then with a lost diaeresis restored. The analyses of an own
    /// spelling take the order and the shares of the sample; together they take the mean share of
    /// the analyses of the dictionary each and go after them on equal shares; one that the
    /// dictionary has is not repeated
    pub(super) fn with_own(
        &self,
        word: &str,
        mut analyses: Vec<Analysis>,
        exact: bool,
    ) -> (Vec<Analysis>, bool) {
        let Some((key, forms, as_written)) = self.own_forms(word) else {
            return (analyses, exact);
        };
        // A lost diaeresis is not looked for in a word the dictionary has as written: `guiso` is
        // no own `güiso`
        if exact && !as_written {
            return (analyses, exact);
        }
        let own_exact = as_written && forms.iter().any(|own| own.marks == key.marks);
        // The analyses of the dictionary with other accents give way to the exact own word
        if own_exact && !exact {
            analyses.clear();
        }
        let exact = exact || own_exact;
        let mut chosen: Vec<&OwnForm> = (forms.iter())
            .filter(|own| !exact || own.marks == key.marks)
            .collect();
        chosen.sort_by_key(|own| own.place);
        let total: f64 = chosen.iter().map(|own| own.share).sum();
        let scale = chosen.len() as f64 / analyses.len().max(1) as f64;
        let known = analyses.len();
        for own in chosen {
            let share = if total > 0.0 {
                own.share / total * scale
            } else {
                scale
            };
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

    /// The key of a word, the own words of it and whether the key is as written: the key as written,
    /// then with a lost diaeresis restored
    fn own_forms(&self, word: &str) -> Option<(Key, &[OwnForm], bool)> {
        let key = self.alphabet.input_key(word)?;
        if let Some(forms) = self.own_words.get(key.bytes()) {
            return Some((key, forms, true));
        }
        (self.alphabet.diaeresis_keys(&key).into_iter()).find_map(|key| {
            let forms = self.own_words.get(key.bytes())?;
            Some((key, forms.as_slice(), false))
        })
    }

    /// Whether a form of a table is a form of its lexeme for `forms` and `inflect`: no lemma only and
    /// no spoken spelling; with own words, a variant only if the dictionary or the own words have
    /// it, as an own word that is no verb takes no variants of its sample (`yoghurt` of `yogur`)
    pub(super) fn takes_form(&self, lexeme: &Lexeme<'_>, form: usize) -> bool {
        let cell = lexeme.cells[form];
        if cell & (LEMMA_ONLY | SPOKEN) != 0 {
            return false;
        }
        if self.own_words.is_empty() || cell & VARIANT == 0 {
            return true;
        }
        let entry = Entry {
            table: lexeme.table,
            form: form as u16,
        };
        lexeme
            .spell(&self.suffixes, form)
            .is_some_and(|spelling| self.recorded(&spelling, entry))
    }

    /// Whether the dictionary or the own words have the form `entry` spelled exactly as `spelling`
    fn recorded(&self, spelling: &str, entry: Entry) -> bool {
        let packed = (u32::from(entry.table) << 16) | u32::from(entry.form);
        let in_automaton = self
            .lookup(spelling)
            .is_some_and(|(key, spellings, as_written)| {
                as_written
                    && spellings.iter().any(|(marks, records, _)| {
                        marks == key.marks
                            && analyses_at(self.records(), records)
                                .is_some_and(|(records, _)| records.contains(&packed))
                    })
            });
        in_automaton
            || self
                .own_forms(spelling)
                .is_some_and(|(key, forms, as_written)| {
                    as_written
                        && (forms.iter()).any(|own| own.marks == key.marks && own.entry == entry)
                })
    }

    /// Whether an own word is spelled as `word` with any accents or with a lost diaeresis
    pub(super) fn is_own(&self, word: &str) -> bool {
        !self.own_words.is_empty() && self.own_forms(word).is_some()
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
    fn an_own_word_follows_the_order_and_variants_of_its_sample() {
        let mut builder = Builder::new();
        builder.meta("{}");
        let past =
            |person: &str| format!("Mood=Ind|Number=Sing|Person={person}|Tense=Imp|VerbForm=Fin");
        let (first_person, third_person) = (past("1"), past("3"));
        let (sing, plur) = ("Gender=Masc|Number=Sing", "Gender=Masc|Number=Plur");
        let forms = [
            ("bloguear", "VERB", "bloguear", "VerbForm=Inf"),
            ("bloguear", "VERB", "blogueaba", first_person.as_str()),
            ("bloguear", "VERB", "blogueaba", third_person.as_str()),
            ("yogur", "NOUN", "yogur", sing),
            ("yogur", "NOUN", "yogures", plur),
            ("yogur", "NOUN", "yoghurt", sing),
            ("pingüino", "NOUN", "pingüino", sing),
            ("guiso", "NOUN", "guiso", sing),
        ];
        for (lemma, pos, form, feats) in forms {
            builder.add_form(lemma, pos, form, feats).unwrap();
        }
        builder
            .mark_variant("yogur", "NOUN", "yoghurt", sing)
            .unwrap();
        // The sample has the third person first
        builder
            .place("bloguear", "VERB", "blogueaba", &third_person, 0, Some(0.8))
            .unwrap();
        builder
            .place("bloguear", "VERB", "blogueaba", &first_person, 1, Some(0.2))
            .unwrap();
        let dictionary = Dictionary::from_bytes(&builder.build().unwrap()).unwrap();
        let dictionary = dictionary
            .with_words([
                ("tiktokear", "bloguear"),
                ("zogur", "yogur"),
                ("zingüino", "pingüino"),
            ])
            .unwrap();
        let analyses = dictionary.analyze("tiktokeaba");
        let tag = |analysis: &Analysis| dictionary.ud_tag(analysis.ud).unwrap();
        assert_eq!(tag(&analyses[0]).feat("Person"), Some("3"));
        assert!(
            (analyses[0].prob - 0.8).abs() < 1e-6,
            "{}",
            analyses[0].prob
        );
        // A noun takes no other spellings of its sample: as words, forms and in inflection
        assert!((dictionary.analyze("zoghurt").iter()).all(|a| a.source != Source::Dictionary));
        let forms: Vec<String> = (dictionary
            .forms(&dictionary.analyze("zogur")[0])
            .into_iter())
        .map(|form| form.word)
        .collect();
        assert_eq!(forms, ["zogur", "zogures"]);
        // The variants of the sample itself stay
        let yogur = dictionary.analyze("yogur").remove(0);
        assert_eq!(dictionary.forms(&yogur).len(), 3);
        let zogures = dictionary.analyze("zogures").remove(0);
        let singular = dictionary.inflect(&zogures, &[("Number", "Sing")]).unwrap();
        assert_eq!(singular.word, "zogur");
        // A lost diaeresis
        assert_eq!(
            first(&dictionary, "zinguino"),
            ("zingüino".into(), "zingüino".into())
        );
        assert!(dictionary.is_known("zinguino"));
        // but not in a word of the dictionary
        let dictionary = dictionary.with_words([("güiso", "pingüino")]).unwrap();
        assert_eq!(dictionary.analyze("guiso").len(), 1);
        assert_eq!(
            first(&dictionary, "güiso"),
            ("güiso".into(), "güiso".into())
        );
        // The error names the ending the word needs
        let error = (dictionary.with_words([("zogurt", "yogur")])).unwrap_err();
        assert!(
            error.to_string().contains("needs the ending \"ur\""),
            "{error}"
        );
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
