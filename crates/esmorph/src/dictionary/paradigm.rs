//! Paradigm model: the choice of the form of an inflection table for an unknown word among the
//! candidates of the lists of its endings. An LSTM reads the letters of the word from its end, and
//! the score of a candidate is a dot product of the vector of the word with the sum of the rows of
//! the features of the candidate: its UD tag, the suffixes of the lemma and of the form, the length
//! of the ending. The weights are a section of the dictionary, the file of
//! `scripts/train_paradigms.py`

use std::collections::HashMap;

use super::weights::{self, Entries, Lstm, MAX_DIM, Matrix, Tiled, add, dot, lstm};
use crate::Error;
use crate::format::{MAX_ENDING, Strings};
use crate::ud::UdTags;

const MAGIC: &[u8; 8] = b"ESMPAR\0\0";
const FORMAT: u32 = 1;
const WHAT: &str = "paradigm model";
/// Numeric features of a candidate: log(1 + number of word forms), its share in the list of the
/// ending, whether the ending is the longest of the word
const NUMERIC: usize = 3;

fn invalid(message: &str) -> Error {
    weights::invalid(WHAT, message)
}

/// Entries of the file of weights of the paradigm model
fn read(data: &[u8]) -> Result<Entries<'_>, Error> {
    weights::read(data, MAGIC, FORMAT, WHAT)
}

/// Weights of the paradigm model
#[derive(Debug)]
pub(crate) struct Model {
    max_chars: usize,
    /// Row of the table of letters by letter: 1 - an unknown one
    chars: HashMap<char, u32>,
    keys: HashMap<String, u32>,
    suffixes: HashMap<String, u32>,
    char: Matrix,
    lstm: Lstm,
    hidden: Matrix,
    hidden_bias: Vec<f32>,
    /// Rows of the features: the vector and the bias in the last column
    key: Matrix,
    lemma_suffix: Matrix,
    form_suffix: Matrix,
    length: Matrix,
    numeric: Vec<f32>,
}

impl Model {
    /// Weights from a file of `scripts/train_paradigms.py`: the sizes of all matrices are checked
    pub fn parse(data: &[u8]) -> Result<Self, Error> {
        let mut entries = read(data)?;
        let weights::Value::Numbers(config) = entries.take("config")? else {
            return Err(invalid("config are not numbers"));
        };
        let &[dim, hidden, dchar, max_chars] = config.as_slice() else {
            return Err(invalid("config are not 4 numbers"));
        };
        let [dim, hidden, dchar, max_chars] = [dim, hidden, dchar, max_chars].map(|v| v as usize);
        if [dim, hidden, dchar, max_chars]
            .iter()
            .any(|&value| value == 0 || value > MAX_DIM)
        {
            return Err(invalid("wrong sizes in config"));
        }
        // Rows 0 and 1 of the table of letters are the padding and the unknown letter, row 0 of
        // the features is the empty one
        let mut chars = HashMap::new();
        for (number, text) in entries.strings("chars")?.iter().enumerate() {
            let mut letters = text.chars();
            let (Some(letter), None) = (letters.next(), letters.next()) else {
                return Err(invalid("chars are not single letters"));
            };
            if chars.insert(letter, number as u32 + 2).is_some() {
                return Err(invalid("chars repeat a string"));
            }
        }
        let keys = entries.index("keys", 1)?;
        let suffixes = entries.index("suffixes", 1)?;
        let width = dim + 1;
        let lstm = Lstm {
            input: Tiled::new(&entries.matrix("lstm.ih", 4 * hidden, dchar)?),
            hidden: Tiled::new(&entries.matrix("lstm.hh", 4 * hidden, hidden)?),
            bias: entries.vector("lstm.bias", 4 * hidden)?,
        };
        Ok(Model {
            max_chars,
            char: entries.matrix("char", chars.len() + 2, dchar)?,
            lstm,
            hidden: entries.matrix("hidden.weight", dim, hidden)?,
            hidden_bias: entries.vector("hidden.bias", dim)?,
            key: entries.matrix("key", keys.len() + 1, width)?,
            lemma_suffix: entries.matrix("lemma_suffix", suffixes.len() + 1, width)?,
            form_suffix: entries.matrix("form_suffix", suffixes.len() + 1, width)?,
            length: entries.matrix("length", MAX_ENDING, width)?,
            numeric: entries.vector("numeric", NUMERIC)?,
            chars,
            keys,
            suffixes,
        })
    }
}

/// The data the model was trained on: the entry `sources` of the file of weights, a JSON string
#[cfg(any(test, feature = "build"))]
pub(crate) fn sources(data: &[u8]) -> Result<String, Error> {
    match read(data)?.take("sources")? {
        weights::Value::Strings(strings) if strings.len() == 1 => Ok(strings[0].to_owned()),
        _ => Err(invalid("sources are not one string")),
    }
}

/// Features of a candidate for its score
#[derive(Clone, Copy, Debug)]
pub(crate) struct Features {
    pub ud: u16,
    /// Numbers of the suffixes of the lemma and of the form in the suffixes of the dictionary
    pub lemma_suffix: usize,
    pub form_suffix: usize,
    pub length: usize,
    pub count: u32,
    pub share: f32,
    pub longest: bool,
}

/// A word for the scores of its candidates: its vector with a one for the biases and its products
/// with the rows of the lengths of an ending, the same for all candidates of the word
#[derive(Debug)]
pub(crate) struct Word {
    vector: Vec<f32>,
    lengths: [f32; MAX_ENDING],
}

/// The model and the rows of the features of the dictionary: the sum of the rows of the keys of
/// every UD tag and the row of every suffix
#[derive(Debug)]
pub(crate) struct Paradigms {
    model: Model,
    tags: Vec<f32>,
    /// Row of the tables of suffixes of the model by the number of a suffix of the dictionary: 0 -
    /// the training had no such suffix
    suffix_rows: Vec<u32>,
}

impl Paradigms {
    pub fn new(model: Model, ud: &UdTags, suffixes: &Strings) -> Self {
        let width = model.key.cols;
        let mut tags = vec![0.0; ud.len() * width];
        for (number, tag) in ud.iter().enumerate() {
            let row = &mut tags[number * width..(number + 1) * width];
            let keys = std::iter::once(format!("P={}", tag.pos()))
                .chain(tag.feats().map(|(name, value)| format!("F={name}={value}")));
            for key in keys {
                if let Some(&key) = model.keys.get(&key) {
                    add(row, model.key.row(key as usize));
                }
            }
        }
        let suffix_rows = (0..suffixes.len())
            .map(|number| {
                (suffixes.get(number))
                    .and_then(|suffix| model.suffixes.get(suffix))
                    .copied()
                    .unwrap_or(0)
            })
            .collect();
        Paradigms {
            model,
            tags,
            suffix_rows,
        }
    }

    /// A lowercase word for the scores of its candidates: the LSTM reads its last letters from the
    /// end. The products are not fused: the scores are the same on every processor
    pub fn word(&self, lower: &str) -> Word {
        let model = &self.model;
        let dchar = model.char.cols;
        let mut input = Vec::with_capacity(model.max_chars * dchar);
        for letter in lower.chars().rev().take(model.max_chars) {
            let row = model.chars.get(&letter).copied().unwrap_or(1);
            input.extend_from_slice(model.char.row(row as usize));
        }
        let state = lstm::<false>(&model.lstm, &input, input.len() / dchar);
        let mut vector = vec![0.0; model.hidden.rows + 1];
        model.hidden.apply(&state, &mut vector[..model.hidden.rows]);
        for (value, bias) in vector.iter_mut().zip(&model.hidden_bias) {
            *value = (*value + bias).tanh();
        }
        vector[model.hidden.rows] = 1.0;
        Word {
            lengths: std::array::from_fn(|length| dot(&vector, model.length.row(length))),
            vector,
        }
    }

    /// Score of a candidate of the word `word`
    pub fn score(&self, word: &Word, features: &Features) -> f32 {
        let model = &self.model;
        let width = model.key.cols;
        let ud = usize::from(features.ud);
        let suffix = |number: usize| self.suffix_rows.get(number).copied().unwrap_or(0) as usize;
        let tag = self.tags.get(ud * width..(ud + 1) * width).unwrap_or(&[]);
        let numeric = [
            (features.count as f32).ln_1p(),
            features.share,
            f32::from(u8::from(features.longest)),
        ];
        let length = features.length.clamp(1, MAX_ENDING) - 1;
        let vector = &word.vector;
        dot(vector, tag)
            + dot(
                vector,
                model.lemma_suffix.row(suffix(features.lemma_suffix)),
            )
            + dot(vector, model.form_suffix.row(suffix(features.form_suffix)))
            + word.lengths[length]
            + dot(&numeric, &model.numeric)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Dictionary;
    use crate::builder::Builder;
    use crate::dictionary::weights::tests::{entry, matrix, strings};

    /// Weights of a small model (dim = 1): all weights are zeros but the biases of the suffixes
    /// of the lemma `lemma_bias`, so they order the candidates
    fn weights(lemma_bias: &[(&str, f32)]) -> Vec<u8> {
        let suffixes: Vec<&str> = lemma_bias.iter().map(|&(suffix, _)| suffix).collect();
        let n = suffixes.len() + 1;
        let mut entries = Vec::new();
        let mut count = 0u32;
        let mut add = |name: &str, kind: u8, body: Vec<u8>| {
            entry(&mut entries, name, kind, &body);
            count += 1;
        };
        let mut config = 4u32.to_le_bytes().to_vec();
        for value in [1u32, 1, 1, 4] {
            config.extend_from_slice(&value.to_le_bytes());
        }
        add("config", 3, config);
        add("sources", 0, strings(&[r#"{"words": 7}"#]));
        add("chars", 0, strings(&["o", "s"]));
        add("keys", 0, strings(&["P=NOUN"]));
        add("suffixes", 0, strings(&suffixes));
        // The bias is the second column of the row of a suffix
        let biases: Vec<f32> = std::iter::once([0.0, 0.0])
            .chain(lemma_bias.iter().map(|&(_, bias)| [0.0, bias]))
            .flatten()
            .collect();
        for (name, rows, cols, values) in [
            ("char", 4, 1, &[][..]),
            ("lstm.ih", 4, 1, &[]),
            ("lstm.hh", 4, 1, &[]),
            ("lstm.bias", 1, 4, &[]),
            ("hidden.weight", 1, 1, &[]),
            ("hidden.bias", 1, 1, &[]),
            ("key", 2, 2, &[]),
            ("lemma_suffix", n, 2, &biases[..]),
            ("form_suffix", n, 2, &[]),
            ("length", MAX_ENDING, 2, &[]),
            ("numeric", 1, NUMERIC, &[]),
        ] {
            add(name, 1, matrix(rows, cols, values));
        }
        let mut data = MAGIC.to_vec();
        data.extend_from_slice(&FORMAT.to_le_bytes());
        data.extend_from_slice(&count.to_le_bytes());
        data.extend_from_slice(&entries);
        data
    }

    /// Three nouns in `-o`, `-os` and four verbs whose first person ends in `-os` too: the ending
    /// `-s` has the verb form with four word forms and the noun form with three
    fn builder() -> Builder {
        let mut builder = Builder::new();
        for noun in ["gato", "perro", "pato"] {
            let plural = format!("{noun}s");
            builder
                .add_form(noun, "NOUN", noun, "Gender=Masc|Number=Sing")
                .unwrap();
            builder
                .add_form(noun, "NOUN", &plural, "Gender=Masc|Number=Plur")
                .unwrap();
        }
        for verb in ["hablar", "cantar", "bailar", "andar"] {
            let form = format!("{}os", verb.strip_suffix("ar").unwrap());
            builder
                .add_form(verb, "VERB", verb, "VerbForm=Inf")
                .unwrap();
            builder
                .add_form(verb, "VERB", &form, "Mood=Ind|Person=1")
                .unwrap();
        }
        builder
    }

    fn lemmas(dictionary: &Dictionary, word: &str) -> Vec<String> {
        (dictionary.analyze(word).into_iter())
            .map(|analysis| analysis.lemma)
            .collect()
    }

    #[test]
    fn chooses_by_model() {
        // Without the model - by the numbers of word forms of the longest ending
        let dictionary = Dictionary::from_bytes(&builder().build().unwrap()).unwrap();
        assert!(!dictionary.has_paradigm_model());
        assert_eq!(lemmas(&dictionary, "zorros"), ["zorrar", "zorro"]);
        // The model prefers the lemma without a suffix after the stem, `zorro` of `zorros`; the
        // probabilities are the softmax of the scores
        let mut builder = builder();
        (builder.paradigm_model(weights(&[("", 3.0), ("ar", 0.0)]))).unwrap();
        let dictionary = Dictionary::from_bytes(&builder.build().unwrap()).unwrap();
        dictionary.verify().unwrap();
        assert!(dictionary.has_paradigm_model());
        let analyses = dictionary.analyze("zorros");
        assert_eq!(lemmas(&dictionary, "zorros"), ["zorro", "zorrar"]);
        let expected = 3.0f64.exp() / (3.0f64.exp() + 1.0);
        assert!((analyses[0].prob - expected).abs() < 1e-6, "{analyses:?}");
        // The words of the dictionary keep their analyses
        assert_eq!(lemmas(&dictionary, "gatos"), ["gato"]);
    }

    #[test]
    fn examples_without_own_word() {
        let dictionary = Dictionary::from_bytes(&builder().build().unwrap()).unwrap();
        let candidates = dictionary.guess_examples("gatos");
        let summary: Vec<(&str, &str, &str, u32, bool)> = (candidates.iter())
            .map(|candidate| {
                (
                    candidate.lemma.as_str(),
                    candidate.lemma_suffix.as_str(),
                    candidate.form_suffix.as_str(),
                    candidate.count,
                    candidate.gold,
                )
            })
            .collect();
        // The own word is taken from the list: its form has two word forms, not three
        assert_eq!(
            summary,
            [("gatar", "ar", "os", 4, false), ("gato", "", "s", 2, true)]
        );
        assert!((candidates[1].share - 2.0 / 6.0).abs() < 1e-6);
    }

    #[test]
    fn broken_weights() {
        assert!(Model::parse(b"").is_err());
        let mut data = MAGIC.to_vec();
        data.extend_from_slice(&FORMAT.to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes());
        let err = Model::parse(&data).unwrap_err().to_string();
        assert!(err.contains("paradigm model: no entry config"), "{err}");
        // The weights of the context model are no weights of the paradigm model
        let context = crate::dictionary::context::tests::weights(&[]);
        let err = Model::parse(&context).unwrap_err().to_string();
        assert!(err.contains("no signature"), "{err}");
        assert!(builder().paradigm_model(b"ESMPAR".to_vec()).is_err());
        let data = weights(&[("o", 1.0)]);
        assert!(Model::parse(&data).is_ok());
        assert_eq!(Builder::paradigm_sources(&data).unwrap(), r#"{"words": 7}"#);
        // Any cut of good weights is an error, not a panic
        for end in 0..data.len() {
            assert!(Model::parse(&data[..end]).is_err(), "{end}");
        }
    }
}
