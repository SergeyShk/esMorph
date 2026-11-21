//! Guessing of lowercase words that the dictionary does not have: a word with a hyphen by its last
//! part (`palestino-israelíes`), a word with a known prefix by the word of the dictionary without
//! it (`antimundialización`), a verb with clitics by its guessed infinitive or gerund
//! (`autogenerarse`) and any other by the forms of the dictionary words with the same ending and
//! their inflection tables. The lemma and the forms are those of the whole word

use super::paradigm::{Features, Paradigms};
use super::{Analysis, Dictionary, Entry, Host, Source, guess_list, with_host_source};
use crate::clitics;
use crate::format::{MAX_ENDING, MIN_ENDING_COUNT};

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

/// How many forms of the list of the longest ending guess a word without the paradigm model
const MAX_COUNTED: usize = 10;

/// The most analyses of a word that the paradigm model gives, and the least weight of one against
/// the first
const MAX_MODEL_GUESSES: usize = 10;
const MIN_MODEL_SHARE: f64 = 0.01;

/// A form of a table from the lists of the endings of an unknown word
#[derive(Clone, Copy, Debug, PartialEq)]
struct Candidate {
    entry: Entry,
    /// Length of the longest ending of the word whose list has the form
    length: usize,
    /// Number of word forms of the dictionary with this ending and form
    count: u32,
    /// Share of the form in the list of this ending
    share: f32,
}

/// The UD tag, the lemma and the numbers of the suffixes of the lemma and the form of a candidate
#[derive(Clone, Debug)]
struct Cell {
    ud: u16,
    lemma: String,
    lemma_suffix: usize,
    form_suffix: usize,
}

/// A candidate of an example of the training of the paradigm model: its lemma and UD tag, its
/// features and whether it is right
#[doc(hidden)]
#[derive(Clone, Debug)]
pub struct GuessCandidate {
    pub lemma: String,
    pub ud: u16,
    pub lemma_suffix: String,
    pub form_suffix: String,
    pub length: usize,
    pub count: u32,
    pub share: f32,
    pub gold: bool,
}

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

    /// Analyses by the endings of the word: with the paradigm model the forms of the lists of all
    /// its endings chosen by the model, without it the forms of the longest ending that the index
    /// has forms for, by their numbers of word forms and how often texts have their tags
    fn by_ending(&self, word: &str) -> Vec<Analysis> {
        if word.chars().count() < MIN_GUESSED {
            return Vec::new();
        }
        let found = match self.paradigm_model() {
            Ok(Some(model)) => self.by_model(model, word),
            // The model was checked when the dictionary was read
            _ => self.by_counts(word),
        };
        // Forms of one lemma and tag in several tables are one analysis
        let mut analyses: Vec<Analysis> = Vec::with_capacity(found.len());
        for (cell, entry, weight) in found {
            match (analyses.iter_mut())
                .find(|known| known.lemma == cell.lemma && known.ud == cell.ud)
            {
                Some(known) => known.prob += weight,
                None => analyses.push(Analysis {
                    word: word.to_owned(),
                    lemma: cell.lemma,
                    ud: cell.ud,
                    prob: weight,
                    source: Source::Guess,
                    entry: Some(entry),
                    words: Vec::new(),
                }),
            }
        }
        let total: f64 = analyses.iter().map(|analysis| analysis.prob).sum();
        for analysis in &mut analyses {
            analysis.prob /= total;
        }
        analyses.sort_by(|a, b| b.prob.total_cmp(&a.prob));
        analyses
    }

    /// The forms of the longest ending of the word that has forms for it, with their weights: the
    /// number of word forms times how often texts have the tag
    fn by_counts(&self, word: &str) -> Vec<(Cell, Entry, f64)> {
        let starts: Vec<usize> = word.char_indices().map(|(at, _)| at).collect();
        let weights = self.tag_weights();
        for length in (1..=MAX_ENDING.min(starts.len() - 1)).rev() {
            let ending = &word[starts[starts.len() - length]..];
            let Some(forms) = (self.guesses.get(ending.as_bytes()))
                .and_then(|offset| guess_list(self.guess_records(), offset))
            else {
                continue;
            };
            let found: Vec<(Cell, Entry, f64)> = (forms.iter().take(MAX_COUNTED))
                .filter_map(|&[record, count]| {
                    let entry = unpack(record);
                    let cell = self.guess_cell(entry, word)?;
                    let weight = weights.get(usize::from(cell.ud)).copied().unwrap_or(1);
                    Some((cell, entry, f64::from(count) * f64::from(weight)))
                })
                .collect();
            if !found.is_empty() {
                return found;
            }
        }
        Vec::new()
    }

    /// The forms of the lists of all endings of the word chosen by the paradigm model: no more
    /// than `MAX_MODEL_GUESSES` of the most probable, with their weights
    fn by_model(&self, model: &Paradigms, word: &str) -> Vec<(Cell, Entry, f64)> {
        let found: Vec<(Candidate, Cell)> = (self.guess_candidates(word, &[]).into_iter())
            .filter_map(|candidate| Some((candidate, self.guess_cell(candidate.entry, word)?)))
            .collect();
        let Some(longest) = found.iter().map(|(candidate, _)| candidate.length).max() else {
            return Vec::new();
        };
        let vector = model.word(word);
        let scores: Vec<f64> = (found.iter())
            .map(|(candidate, cell)| {
                let features = Features {
                    ud: cell.ud,
                    lemma_suffix: cell.lemma_suffix,
                    form_suffix: cell.form_suffix,
                    length: candidate.length,
                    count: candidate.count,
                    share: candidate.share,
                    longest: candidate.length == longest,
                };
                f64::from(model.score(&vector, &features))
            })
            .collect();
        let top = scores.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        // Forms of one table with one suffix are one lexeme with one stem. The model chooses the
        // lexeme, and its equal forms (`verde` masculine or feminine) it tells apart at random: in
        // training all of them are right. The weight of a lexeme goes to its forms by their numbers
        // of word forms, as without the model
        let lexeme = |at: usize| (found[at].0.entry.table, found[at].1.form_suffix);
        let mut order: Vec<usize> = (0..found.len()).collect();
        order.sort_by_key(|&at| lexeme(at));
        let mut weights = vec![0.0; found.len()];
        for group in order.chunk_by(|&a, &b| lexeme(a) == lexeme(b)) {
            let mass: f64 = group.iter().map(|&at| (scores[at] - top).exp()).sum();
            let count: f64 = group.iter().map(|&at| f64::from(found[at].0.count)).sum();
            for &at in group {
                weights[at] = mass * f64::from(found[at].0.count) / count;
            }
        }
        // Equal weights keep the order of the lists
        order.sort_unstable();
        order.sort_by(|&a, &b| weights[b].total_cmp(&weights[a]));
        let least = weights[order[0]] * MIN_MODEL_SHARE;
        let mut found: Vec<Option<(Candidate, Cell)>> = found.into_iter().map(Some).collect();
        (order.into_iter())
            .take(MAX_MODEL_GUESSES)
            .take_while(|&at| weights[at] >= least)
            .filter_map(|at| {
                let (candidate, cell) = found[at].take()?;
                Some((cell, candidate.entry, weights[at]))
            })
            .collect()
    }

    /// The UD tag, the lemma and the suffixes of the form `entry` for the unknown word `word`;
    /// `None` if the word does not end with the suffix of the form or its stem has no vowel: such
    /// a stem is a coincidence, not a sample
    fn guess_cell(&self, entry: Entry, word: &str) -> Option<Cell> {
        let lexeme = self.lexeme(entry, word)?;
        if !lexeme.stem.chars().any(vowel) {
            return None;
        }
        Some(Cell {
            ud: lexeme.ud(usize::from(entry.form))?,
            lemma: lexeme.spell(&self.suffixes, 0)?,
            lemma_suffix: *lexeme.paradigm.first()? as usize,
            form_suffix: *lexeme.paradigm.get(usize::from(entry.form))? as usize,
        })
    }

    /// The forms of tables of the lists of all endings of the word, from the longest ending; a
    /// form comes from the longest ending whose list has it. The forms `exclude` are the analyses
    /// of a word of the dictionary: the word form added one to its form at each of its endings, and
    /// without these ones the lists are as if the dictionary had not the word
    fn guess_candidates(&self, word: &str, exclude: &[Entry]) -> Vec<Candidate> {
        let starts: Vec<usize> = word.char_indices().map(|(at, _)| at).collect();
        let mut candidates: Vec<Candidate> = Vec::new();
        let mut list: Vec<(Entry, u32)> = Vec::new();
        for length in (1..=MAX_ENDING.min(starts.len().saturating_sub(1))).rev() {
            let ending = &word[starts[starts.len() - length]..];
            let Some(forms) = (self.guesses.get(ending.as_bytes()))
                .and_then(|offset| guess_list(self.guess_records(), offset))
            else {
                continue;
            };
            list.clear();
            list.extend(forms.iter().filter_map(|&[record, count]| {
                let entry = unpack(record);
                let count = count - u32::from(exclude.contains(&entry)).min(count);
                (count > 0).then_some((entry, count))
            }));
            let total: u32 = list.iter().map(|&(_, count)| count).sum();
            if total < MIN_ENDING_COUNT {
                continue;
            }
            for &(entry, count) in &list {
                if !candidates.iter().any(|known| known.entry == entry) {
                    candidates.push(Candidate {
                        entry,
                        length,
                        count,
                        share: count as f32 / total as f32,
                    });
                }
            }
        }
        candidates
    }

    /// Examples of the training of the paradigm model: the candidates of a lowercase word of the
    /// dictionary as if the dictionary had not the word, and whether each is right - the form of
    /// a table of an analysis of the dictionary, or its lemma and UD tag
    #[doc(hidden)]
    pub fn guess_examples(&self, word: &str) -> Vec<GuessCandidate> {
        let own = self.found(word);
        let entries: Vec<Entry> = own.iter().filter_map(|analysis| analysis.entry).collect();
        (self.guess_candidates(word, &entries).into_iter())
            .filter_map(|candidate| {
                let cell = self.guess_cell(candidate.entry, word)?;
                let gold = entries.contains(&candidate.entry)
                    || (own.iter()).any(|known| known.ud == cell.ud && known.lemma == cell.lemma);
                Some(GuessCandidate {
                    lemma_suffix: self.suffixes.get(cell.lemma_suffix)?.to_owned(),
                    form_suffix: self.suffixes.get(cell.form_suffix)?.to_owned(),
                    lemma: cell.lemma,
                    ud: cell.ud,
                    length: candidate.length,
                    count: candidate.count,
                    share: candidate.share,
                    gold,
                })
            })
            .collect()
    }
}

/// The form of a table of a record of a list of guesses, `(table << 16) | form`
fn unpack(record: u32) -> Entry {
    Entry {
        table: (record >> 16) as u16,
        form: record as u16,
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
        let layout = crate::format::layout(&clean, true).unwrap();
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
