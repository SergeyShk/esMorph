//! Guessing of lowercase words that the dictionary does not have: a word with a hyphen by its last
//! part (`palestino-israelíes`), a word with a known prefix by the word of the dictionary without
//! it (`antimundialización`), a verb with clitics by its guessed infinitive or gerund
//! (`autogenerarse`) and any other by the forms of the dictionary words with the same ending and
//! their inflection tables. The lemma and the forms are those of the whole word

use super::{Analysis, Dictionary, Entry, Host, Source, guess_list, with_host_source};
use crate::clitics;
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
/// Parts of speech that the last part of a word with a hyphen keeps: `sur-este` is no determiner
const HYPHENATED: [&str; 5] = ["NOUN", "ADJ", "VERB", "ADV", "NUM"];

/// The shortest word guessed by its ending and the shortest word after a prefix, in letters: an
/// ending of a shorter word tells nothing
const MIN_GUESSED: usize = 3;

/// A longer word is not guessed: its ending tells nothing, and copies of it are costly
const MAX_GUESSED: usize = 64;

/// The shortest word after a prefix of three letters: shorter rests are mostly other words,
/// `destos` is no `des` + `tos`, `presona` no `pre` + `sona`
const MIN_AFTER_SHORT_PREFIX: usize = 4;

/// Endings of an adjective of the absolute degree: `esbeltísima` is one, though the nouns and
/// adverbs of the ending are more frequent words
const ABSOLUTE: [&str; 4] = ["ísimo", "ísima", "ísimos", "ísimas"];

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
        let with_clitics = self.with_guessed_host(word);
        if !with_clitics.is_empty() {
            return with_clitics;
        }
        let analyses = self.by_ending(word);
        if ABSOLUTE.iter().any(|ending| word.ends_with(ending)) {
            let absolute: Vec<Analysis> = (analyses.iter())
                .filter(|analysis| {
                    self.ud
                        .get(analysis.ud)
                        .is_some_and(|tag| tag.pos() == "ADJ" && tag.feat("Degree") == Some("Abs"))
                })
                .cloned()
                .collect();
            if !absolute.is_empty() {
                return normalized(absolute);
            }
        }
        analyses
    }

    /// A verb with clitics whose host the dictionary does not have: the guessed analyses of the
    /// host that are a gerund or an infinitive before `se` or a clitic of the third person. A
    /// guessed imperative would be too often another word (`zumbete`), as an infinitive before
    /// other clitics: `compañeros` is no `compañer` + `os`, `imparte` no `impar` + `te`
    fn with_guessed_host(&self, word: &str) -> Vec<Analysis> {
        for split in clitics::splits(word) {
            let infinitive = !matches!(split.clitics[0], "os" | "te" | "me" | "nos");
            for host in clitics::hosts(split.host, split.clitics[0]) {
                // Only a gerund takes these clitics here: another host is not guessed at all
                if !infinitive && !host.ends_with("ndo") {
                    continue;
                }
                let mut guessed = self.prefixed(&host);
                if guessed.is_empty() {
                    guessed = self.by_ending(&host);
                }
                let hosts: Vec<Analysis> = (guessed.into_iter())
                    .filter(|analysis| {
                        let form = self
                            .ud
                            .get(analysis.ud)
                            .and_then(|tag| tag.feat("VerbForm"));
                        self.hosts.get(usize::from(analysis.ud)) == Some(&Host::NonFinite)
                            && (infinitive || form == Some("Ger"))
                    })
                    .collect();
                if !hosts.is_empty() {
                    return with_host_source(word, &split, hosts, Source::Guess);
                }
            }
        }
        Vec::new()
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
            .filter(|analysis| {
                self.ud
                    .get(analysis.ud)
                    .is_some_and(|tag| HYPHENATED.contains(&tag.pos()))
            })
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
            let shortest = if prefix.chars().count() == 3 {
                MIN_AFTER_SHORT_PREFIX
            } else {
                MIN_GUESSED
            };
            if rest.chars().count() < shortest {
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
            let mut found: Vec<(Analysis, u64)> = Vec::new();
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
                    Some((_, total)) => *total += u64::from(count),
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
                        u64::from(count),
                    )),
                }
            }
            if found.is_empty() {
                continue;
            }
            let total: f64 = found.iter().map(|&(_, count)| count as f64).sum();
            let mut analyses: Vec<Analysis> = found
                .into_iter()
                .map(|(analysis, count)| Analysis {
                    prob: count as f64 / total,
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
                .add_tag_frequency("NOUN", "Gender=Masc|Number=Plur", frequency)
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
    fn the_forms_of_a_guessed_word_are_guessed() {
        let dictionary = dictionary(Some(10));
        let zorros = &dictionary.analyze("zorros")[0];
        let forms = dictionary.forms(zorros);
        assert!(forms.iter().all(|form| form.source == Source::Guess));
        assert_eq!(dictionary.inflect(&forms[0], &[]).unwrap().word, "zorro");
    }

    #[test]
    fn a_verb_with_clitics_is_guessed_by_its_infinitive() {
        let dictionary = dictionary(None);
        let analyses = dictionary.analyze("zorrarlo");
        assert_eq!(analyses.len(), 1);
        let analysis = &analyses[0];
        assert_eq!(
            (analysis.lemma.as_str(), analysis.source),
            ("zorrar", Source::Guess)
        );
        assert_eq!(analysis.words, ["zorrar", "lo"]);
        let forms: Vec<String> = (dictionary.forms(analysis).into_iter())
            .map(|form| form.word)
            .collect();
        assert_eq!(forms, ["zorrar", "zorros"]);
    }

    #[test]
    fn heavy_weights_neither_overflow_nor_lose_their_order() {
        let dictionary = dictionary(Some(u32::MAX));
        let analyses = dictionary.analyze("zorros");
        assert_eq!(analyses[0].lemma, "zorro");
        let total: f64 = analyses.iter().map(|analysis| analysis.prob).sum();
        assert!((total - 1.0).abs() < 1e-9, "{total}");
    }

    #[test]
    fn damaged_guesses_never_panic() {
        let mut builder = Builder::new();
        for noun in ["gato", "perro", "pato"] {
            let stem = noun.strip_suffix('o').unwrap();
            builder.add_form(noun, "NOUN", noun, "Number=Sing").unwrap();
            builder
                .add_form(noun, "NOUN", &format!("{stem}os"), "Number=Plur")
                .unwrap();
        }
        let clean = builder.build().unwrap();
        let layout = crate::format::layout(&clean).unwrap();
        // The automata are left out: only the checksum catches damage in them
        let automata = |at: &usize| layout.words.contains(at) || layout.guesses.contains(at);
        for at in (16..clean.len()).filter(|at| !automata(at)) {
            for flip in [0x01, 0x10, 0xFF] {
                let mut damaged = clean.clone();
                damaged[at] ^= flip;
                let damaged = crate::format::tests::with_checksum(damaged);
                let Ok(dictionary) = Dictionary::from_bytes(&damaged) else {
                    continue;
                };
                for word in ["zorros", "supergatos", "gato-perros", "zorro"] {
                    for analysis in dictionary.analyze(word) {
                        let _ = dictionary.forms(&analysis);
                        let _ = dictionary.inflect(&analysis, &[("Number", "Sing")]);
                    }
                }
            }
        }
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
        // The last part of a word with a hyphen is no word of a closed class
        let mut builder = Builder::new();
        builder.add_form("este", "DET", "este", "").unwrap();
        let dictionary = Dictionary::from_bytes(&builder.build().unwrap()).unwrap();
        assert!(dictionary.analyze("sur-este").is_empty());
    }
}
