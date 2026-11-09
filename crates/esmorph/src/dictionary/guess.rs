//! Guessing of lowercase words that the dictionary does not have: a word with a hyphen by its last
//! part (`palestino-israelíes`), a word with a known prefix by the word of the dictionary without
//! it (`antimundialización`) and any other by the forms of the dictionary words with the same
//! ending and their inflection tables. The lemma and the forms are those of the whole word

use super::{Analysis, Dictionary, Entry, Source, guess_list};
use crate::format::MAX_ENDING;

/// Prefixes after which an unknown word is analyzed as the dictionary word without them, the
/// longest first; prefixes of two letters (`re`, `co`, `ex`) start too many other words
const PREFIXES: &[&str] = &[
    "electro", "contra", "pseudo", "archi", "ciber", "extra", "hiper", "infra", "inter", "intra",
    "macro", "micro", "multi", "pluri", "retro", "sobre", "super", "supra", "trans", "ultra",
    "anti", "auto", "euro", "foto", "hipo", "mega", "mini", "mono", "poli", "post", "semi", "tele",
    "tras", "vice", "bio", "des", "eco", "geo", "neo", "pos", "pre", "pro", "sub", "tri",
];

/// Parts of speech that a word after a prefix keeps: the closed classes take no prefixes
const OPEN: [&str; 4] = ["NOUN", "ADJ", "VERB", "ADV"];

/// The shortest word guessed by its ending and the shortest word after a prefix, in letters: an
/// ending of a shorter word tells nothing
const MIN_GUESSED: usize = 3;

/// A longer word is not guessed: its ending tells nothing, and copies of it are costly
const MAX_GUESSED: usize = 64;

impl Dictionary {
    /// Analyses of an unknown lowercase word, the most probable first; empty if nothing fits
    pub(super) fn guess(&self, word: &str) -> Vec<Analysis> {
        if !word
            .chars()
            .all(|c| (c.is_alphabetic() && !c.is_uppercase()) || c == '-')
            || word.chars().count() > MAX_GUESSED
        {
            return Vec::new();
        }
        if word.contains('-') {
            return self.hyphenated(word);
        }
        let prefixed = self.prefixed(word);
        if !prefixed.is_empty() {
            return prefixed;
        }
        self.by_ending(word)
    }

    /// A word with a hyphen: the analyses of its last part, whose stem takes the first part
    fn hyphenated(&self, word: &str) -> Vec<Analysis> {
        let Some((head, last)) = word.rsplit_once('-') else {
            return Vec::new();
        };
        if head.is_empty() || head.starts_with('-') || head.ends_with('-') || last.is_empty() {
            return Vec::new();
        }
        let analyses = self
            .analyze(last)
            .into_iter()
            .filter(|analysis| analysis.entry.is_some() && analysis.words.is_empty())
            .map(|analysis| Analysis {
                word: format!("{head}-{}", analysis.word),
                lemma: format!("{head}-{}", analysis.lemma),
                source: Source::Guess,
                ..analysis
            })
            .collect();
        normalized(analyses)
    }

    /// A word with a known prefix: the analyses of an open class of the dictionary word without it,
    /// whose stem takes the prefix
    fn prefixed(&self, word: &str) -> Vec<Analysis> {
        for prefix in PREFIXES {
            let Some(rest) = word.strip_prefix(prefix) else {
                continue;
            };
            if rest.chars().count() < MIN_GUESSED {
                continue;
            }
            let analyses: Vec<Analysis> = self
                .found(rest)
                .into_iter()
                .filter(|analysis| {
                    self.ud
                        .get(analysis.ud)
                        .is_some_and(|tag| OPEN.contains(&tag.pos()))
                })
                .map(|analysis| Analysis {
                    word: format!("{prefix}{}", analysis.word),
                    lemma: format!("{prefix}{}", analysis.lemma),
                    source: Source::Guess,
                    ..analysis
                })
                .collect();
            if !analyses.is_empty() {
                return normalized(analyses);
            }
        }
        Vec::new()
    }

    /// Analyses by the longest ending of the word that the index has forms for: the forms of the
    /// tables of dictionary words with this ending, by their numbers of word forms
    fn by_ending(&self, word: &str) -> Vec<Analysis> {
        let starts: Vec<usize> = word.char_indices().map(|(at, _)| at).collect();
        if starts.len() < MIN_GUESSED {
            return Vec::new();
        }
        for length in (1..=MAX_ENDING.min(starts.len() - 1)).rev() {
            let ending = &word[starts[starts.len() - length]..];
            let Some(forms) = (self.guesses.get(ending.as_bytes()))
                .and_then(|offset| guess_list(self.guess_records(), offset))
            else {
                continue;
            };
            let mut found: Vec<(Analysis, u32)> = Vec::new();
            for &[record, count] in forms {
                let entry = Entry {
                    table: (record >> 16) as u16,
                    form: record as u16,
                };
                let Some(lexeme) = self.lexeme(entry, word) else {
                    continue;
                };
                let (Some(lemma), Some(ud)) = (
                    lexeme.spell(&self.suffixes, 0),
                    lexeme.ud(usize::from(entry.form)),
                ) else {
                    continue;
                };
                // A stem without a vowel is a coincidence, not a sample
                if !lexeme.stem.chars().any(vowel) {
                    continue;
                }
                match found
                    .iter_mut()
                    .find(|(known, _)| known.lemma == lemma && known.ud == ud)
                {
                    Some((_, total)) => *total += count,
                    None => found.push((
                        Analysis {
                            word: word.to_owned(),
                            lemma,
                            ud,
                            prob: 0.0,
                            source: Source::Guess,
                            entry: Some(entry),
                            words: Vec::new(),
                        },
                        count,
                    )),
                }
            }
            if found.is_empty() {
                continue;
            }
            let total: f64 = found.iter().map(|&(_, count)| f64::from(count)).sum();
            let mut analyses: Vec<Analysis> = found
                .into_iter()
                .map(|(analysis, count)| Analysis {
                    prob: f64::from(count) / total,
                    ..analysis
                })
                .collect();
            analyses.sort_by(|a, b| b.prob.total_cmp(&a.prob));
            return analyses;
        }
        Vec::new()
    }
}

fn vowel(c: char) -> bool {
    "aeiouáéíóúü".contains(c)
}

/// Analyses with probabilities that sum to one, the most probable first
fn normalized(mut analyses: Vec<Analysis>) -> Vec<Analysis> {
    let total: f64 = analyses.iter().map(|analysis| analysis.prob).sum();
    let count = analyses.len() as f64;
    for analysis in &mut analyses {
        analysis.prob = if total > 0.0 {
            analysis.prob / total
        } else {
            1.0 / count
        };
    }
    analyses.sort_by(|a, b| b.prob.total_cmp(&a.prob));
    analyses
}

#[cfg(test)]
mod tests {
    use crate::builder::Builder;
    use crate::{Dictionary, Source};

    fn dictionary(tag_frequency: Option<u32>) -> Dictionary {
        let mut builder = Builder::new();
        for noun in ["gato", "perro", "pato"] {
            let stem = noun.strip_suffix('o').unwrap();
            builder
                .add_form(noun, "NOUN", noun, "Gender=Masc|Number=Sing")
                .unwrap();
            builder
                .add_form(
                    noun,
                    "NOUN",
                    &format!("{stem}os"),
                    "Gender=Masc|Number=Plur",
                )
                .unwrap();
        }
        // A table of one lexeme is no sample
        builder
            .add_form("caos", "NOUN", "caos", "Number=Sing")
            .unwrap();
        for verb in ["hablar", "cantar", "bailar", "andar"] {
            let stem = verb.strip_suffix("ar").unwrap();
            builder
                .add_form(verb, "VERB", verb, "VerbForm=Inf")
                .unwrap();
            builder
                .add_form(verb, "VERB", &format!("{stem}os"), "Mood=Ind|Person=1")
                .unwrap();
        }
        if let Some(frequency) = tag_frequency {
            builder
                .set_tag_frequency("NOUN", "Gender=Masc|Number=Plur", frequency)
                .unwrap();
        }
        Dictionary::from_bytes(&builder.build().unwrap()).unwrap()
    }

    fn summary(dictionary: &Dictionary, word: &str) -> Vec<(String, String, String)> {
        dictionary
            .analyze(word)
            .into_iter()
            .map(|analysis| {
                assert_eq!(analysis.source, Source::Guess);
                let tag = dictionary.ud_tag(analysis.ud).unwrap();
                (analysis.word, analysis.lemma, tag.pos().to_owned())
            })
            .collect()
    }

    #[test]
    fn guesses_by_the_ending_and_its_tables() {
        let dictionary = dictionary(None);
        // Four verbs against three nouns
        let lemmas: Vec<String> = (summary(&dictionary, "zorros").into_iter())
            .map(|(_, lemma, _)| lemma)
            .collect();
        assert_eq!(lemmas, ["zorrar", "zorro"]);
        let zorro = &dictionary.analyze("zorros")[1];
        assert!((zorro.prob - 3.0 / 7.0).abs() < 1e-9);
        let forms: Vec<String> = (dictionary.forms(zorro).into_iter())
            .map(|form| form.word)
            .collect();
        assert_eq!(forms, ["zorro", "zorros"]);
        // Texts have nouns more often: the frequency of a tag weighs its forms
        let dictionary = self::dictionary(Some(10));
        assert_eq!(summary(&dictionary, "zorros")[0].1, "zorro");
    }

    #[test]
    fn guesses_by_parts() {
        let dictionary = dictionary(None);
        assert_eq!(
            summary(&dictionary, "supergatos"),
            [("supergatos".into(), "supergato".into(), "NOUN".into())]
        );
        assert_eq!(
            summary(&dictionary, "gato-perros")[0],
            ("gato-perros".into(), "gato-perro".into(), "NOUN".into())
        );
    }

    #[test]
    fn what_is_not_guessed() {
        let dictionary = dictionary(None);
        // A table of one lexeme gives no guess: `zaos` is not `caos`
        let rare = |analysis: &crate::Analysis| analysis.lemma == "zaos";
        assert!(!dictionary.analyze("zaos").iter().any(rare));
        // A stem without a vowel, a word shorter than three letters, other tokens
        for word in ["trs", "os", "-os", "os-", "a1os"] {
            assert!(
                dictionary
                    .analyze(word)
                    .iter()
                    .all(|analysis| analysis.source != Source::Guess),
                "{word}"
            );
        }
        let long = "a".repeat(70) + "os";
        assert!(dictionary.analyze(&long).is_empty());
    }
}
