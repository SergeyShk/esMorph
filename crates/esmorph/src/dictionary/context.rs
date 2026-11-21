//! Context model: the choice of the analysis of a token by the tokens of its sentence. The input of
//! a token is its shape, its word, its endings and the tags of its analyses; a BiLSTM runs over the
//! sentence, and the score of an analysis is a dot product with the vector of its token. The
//! weights are a section of the dictionary, the file of `scripts/train_context.py`: a signature,
//! a version and entries "name, kind, data", numbers little-endian

use std::collections::HashMap;
use std::fmt::Write;

use super::weights::{
    self, BATCH, Entries, FUSED, Lstm, MAX_DIM, Matrix, Tiled, Value, add, bilstm, dot,
};
use super::{Analysis, Source};
use crate::Error;
use crate::clitics;
use crate::ud::UdTags;

const MAGIC: &[u8; 8] = b"ESMCTX\0\0";
const FORMAT: u32 = 1;
const WHAT: &str = "context model";
/// Rows of the table of token shapes (`shape`)
const SHAPES: usize = 64;
/// Sources of analyses in the order of the rows of the table `source`
const SOURCES: [Source; 4] = [
    Source::Dictionary,
    Source::Rule,
    Source::Guess,
    Source::Clitics,
];
/// The most words of a token that the tags of an analysis tell apart
const MAX_WORDS: usize = 4;

fn invalid(message: &str) -> Error {
    weights::invalid(WHAT, message)
}

/// Entries of the file of weights of the context model
fn read(data: &[u8]) -> Result<Entries<'_>, Error> {
    weights::read(data, MAGIC, FORMAT, WHAT)
}

/// Weights of the context model
#[derive(Debug)]
pub(crate) struct Model {
    kmax: usize,
    suffix_max: usize,
    words: HashMap<String, u32>,
    suffixes: HashMap<String, u32>,
    /// Number of a tag of an analysis from 1: `P=` part of speech, `F=` feature and value, `W=`
    /// words of a token, `C=` part of speech, features and words together
    tags: HashMap<String, u32>,
    /// Tags of the parts of speech and features of the dictionary the model was trained with
    #[cfg_attr(not(feature = "build"), allow(dead_code))]
    inventory: Vec<String>,
    word: Matrix,
    suffix: Matrix,
    shape: Matrix,
    tag_in: Matrix,
    tag_out: Matrix,
    tag_bias: Matrix,
    source: Matrix,
    input: Tiled,
    input_bias: Vec<f32>,
    lstm: Vec<[Lstm; 2]>,
    norm_weight: Vec<f32>,
    norm_bias: Vec<f32>,
    query: Matrix,
    query_bias: Vec<f32>,
}

/// The data the model was trained on: the entry `sources` of the file of weights, a JSON string
#[cfg(any(test, feature = "build"))]
pub(crate) fn sources(data: &[u8]) -> Result<String, Error> {
    match read(data)?.take("sources")? {
        Value::Strings(strings) if strings.len() == 1 => Ok(strings[0].to_owned()),
        _ => Err(invalid("sources are not one string")),
    }
}

impl Model {
    /// Weights from a file of `scripts/train_context.py`: the sizes of all matrices are checked
    pub fn parse(data: &[u8]) -> Result<Self, Error> {
        let mut entries = read(data)?;
        let Value::Numbers(config) = entries.take("config")? else {
            return Err(invalid("config are not numbers"));
        };
        let &[d, layers, dw, ds, dc, dshape, kmax, suffix_max] = config.as_slice() else {
            return Err(invalid("config are not 8 numbers"));
        };
        let [d, layers, dw, ds, dc, dshape, kmax, suffix_max] =
            [d, layers, dw, ds, dc, dshape, kmax, suffix_max].map(|value| value as usize);
        if [d, dw, ds, dc, dshape, kmax, suffix_max]
            .iter()
            .any(|&value| value == 0 || value > MAX_DIM)
            || layers == 0
            || layers > 8
            || d % 2 != 0
            || suffix_max > 16
        {
            return Err(invalid("wrong sizes in config"));
        }
        // Rows 0 and 1 of the tables of words and endings are the empty and the unknown one, row 0
        // of tags is the empty one
        let words = entries.index("words", 2)?;
        let suffixes = entries.index("suffixes", 2)?;
        let tags = entries.index("tags", 1)?;
        let inventory = (entries.strings("inventory")?.iter())
            .map(|text| (*text).to_owned())
            .collect();
        let hidden = d / 2;
        let din = dshape + dc + dw + suffix_max * ds;
        let mut lstm = Vec::with_capacity(layers);
        for layer in 0..layers {
            let mut cell = |direction: &str| -> Result<Lstm, Error> {
                let prefix = format!("lstm.{layer}.{direction}");
                Ok(Lstm {
                    input: Tiled::new(&entries.matrix(&format!("{prefix}.ih"), 4 * hidden, d)?),
                    hidden: Tiled::new(&entries.matrix(
                        &format!("{prefix}.hh"),
                        4 * hidden,
                        hidden,
                    )?),
                    bias: entries.vector(&format!("{prefix}.bias"), 4 * hidden)?,
                })
            };
            lstm.push([cell("forward")?, cell("backward")?]);
        }
        Ok(Model {
            kmax,
            suffix_max,
            word: entries.matrix("word", words.len() + 2, dw)?,
            suffix: entries.matrix("suffix", suffixes.len() + 2, ds)?,
            shape: entries.matrix("shape", SHAPES, dshape)?,
            tag_in: entries.matrix("tag_in", tags.len() + 1, dc)?,
            tag_out: entries.matrix("tag_out", tags.len() + 1, dc)?,
            tag_bias: entries.matrix("tag_bias", tags.len() + 1, 1)?,
            source: entries.matrix("source", SOURCES.len(), dc)?,
            input: Tiled::new(&entries.matrix("input.weight", d, din)?),
            input_bias: entries.vector("input.bias", d)?,
            lstm,
            norm_weight: entries.vector("norm.weight", d)?,
            norm_bias: entries.vector("norm.bias", d)?,
            query: entries.matrix("query.weight", dc, d)?,
            query_bias: entries.vector("query.bias", dc)?,
            words,
            suffixes,
            tags,
            inventory,
        })
    }
}

/// A sentence or a part of it for `Context::rank`: its tokens, their analyses and whether they
/// start the sentence
#[derive(Debug)]
pub(crate) struct Sentence<'a> {
    pub words: Vec<&'a str>,
    pub analyses: Vec<Vec<Analysis>>,
    pub first: bool,
}

/// The model and, for every UD tag of the dictionary and number of words of a token, the sums of
/// the rows of the tags of an analysis: input, output and bias
#[derive(Debug)]
pub(crate) struct Context {
    model: Model,
    tag_in: Vec<f32>,
    tag_out: Vec<f32>,
    tag_bias: Vec<f32>,
}

impl Context {
    pub fn new(model: Model, ud: &UdTags) -> Self {
        let dc = model.tag_in.cols;
        let size = ud.len() * MAX_WORDS;
        let mut context = Context {
            tag_in: vec![0.0; size * dc],
            tag_out: vec![0.0; size * dc],
            tag_bias: vec![0.0; size],
            model,
        };
        let mut feats: Vec<(&str, &str)> = Vec::new();
        let mut name = String::new();
        let mut rows = Vec::new();
        for (number, tag) in ud.iter().enumerate() {
            feats.clear();
            feats.extend(tag.feats());
            // As `sorted` of Python orders them in the training script: by the bytes of the name
            feats.sort_unstable();
            let joined = (feats.iter())
                .map(|(feat, value)| format!("{feat}={value}"))
                .collect::<Vec<_>>()
                .join("|");
            for words in 1..=MAX_WORDS {
                rows.clear();
                let mut find = |name: &str| rows.extend(context.model.tags.get(name).copied());
                name.clear();
                let _ = write!(name, "P={}", tag.pos());
                find(&name);
                for (feat, value) in &feats {
                    name.clear();
                    let _ = write!(name, "F={feat}={value}");
                    find(&name);
                }
                if words > 1 {
                    name.clear();
                    let _ = write!(name, "W={words}");
                    find(&name);
                }
                name.clear();
                let _ = write!(name, "C={}|{joined}|{words}", tag.pos());
                find(&name);
                let at = number * MAX_WORDS + words - 1;
                for &row in &rows {
                    let row = row as usize;
                    add(
                        &mut context.tag_in[at * dc..(at + 1) * dc],
                        context.model.tag_in.row(row),
                    );
                    add(
                        &mut context.tag_out[at * dc..(at + 1) * dc],
                        context.model.tag_out.row(row),
                    );
                    context.tag_bias[at] += context.model.tag_bias.row(row)[0];
                }
            }
        }
        context
    }

    /// Tags of the parts of speech and features of the dictionary `ud` that the dictionary of
    /// the training of the model had not
    #[cfg(feature = "build")]
    pub fn gaps(&self, ud: &UdTags) -> Vec<String> {
        let known: std::collections::HashSet<&str> =
            self.model.inventory.iter().map(String::as_str).collect();
        let mut gaps: Vec<String> = (ud.iter())
            .flat_map(|tag| {
                std::iter::once(format!("P={}", tag.pos()))
                    .chain(tag.feats().map(|(name, value)| format!("F={name}={value}")))
            })
            .filter(|name| !known.contains(name.as_str()))
            .collect();
        gaps.sort_unstable();
        gaps.dedup();
        gaps
    }

    /// Place of the sums of an analysis among the sums of the tags
    fn slot(&self, analysis: &Analysis) -> Option<usize> {
        let words = analysis.words.len().clamp(1, MAX_WORDS);
        // The model saw tokens of several words only of verbs with clitics and contractions: a
        // number with its sign (`60%`) and a unit with its number (`km.24`) are one word for it
        let several = match analysis.source {
            Source::Rule => false,
            Source::Dictionary => clitics::contraction(&analysis.word).is_some(),
            _ => true,
        };
        let words = if words > 1 && !several { 1 } else { words };
        let at = usize::from(analysis.ud) * MAX_WORDS + words - 1;
        (at < self.tag_bias.len()).then_some(at)
    }

    fn sums<'a>(&self, table: &'a [f32], analysis: &Analysis) -> &'a [f32] {
        let dc = self.model.tag_in.cols;
        self.slot(analysis)
            .map_or(&[], |at| &table[at * dc..(at + 1) * dc])
    }

    /// Analyses of the tokens of sentences by the score of the model, highest first; the
    /// probability is the share in the context. A sentence is its tokens, their analyses and
    /// whether they start the sentence (`first`): its start is the first token with a letter or
    /// a digit, after a dash of a dialogue, a quote or a bracket. The model sees the first `kmax`
    /// analyses of a token, the others follow them with the probability 0; the tokens are
    /// normalized as `analyze` reads them. The BiLSTM runs over four sentences at once, and the
    /// analyses of a sentence are those of it alone to the last bit
    pub fn rank(&self, sentences: Vec<Sentence<'_>>) -> Vec<Vec<Vec<Analysis>>> {
        // A build for x86-64 knows no fused multiply-add and no AVX2: a processor that has them
        // takes the copy of the model built for them, whose scores are those of arm64 to the last
        // bit
        #[cfg(target_arch = "x86_64")]
        if !FUSED && is_x86_feature_detected!("avx2") && is_x86_feature_detected!("fma") {
            // SAFETY: the processor has the features of the function
            return unsafe { self.rank_fused(sentences) };
        }
        self.rank_with::<FUSED>(sentences)
    }

    /// `rank` with the instructions of AVX2 and the fused multiply-add
    #[cfg(target_arch = "x86_64")]
    #[target_feature(enable = "avx2,fma")]
    fn rank_fused(&self, sentences: Vec<Sentence<'_>>) -> Vec<Vec<Vec<Analysis>>> {
        self.rank_with::<true>(sentences)
    }

    /// `rank` with the fused multiply-add if `FMA`; inlined into each copy with its instructions.
    /// The products go in loops, not in closures: a closure that is not inlined has none of the
    /// instructions of the copy
    #[inline(always)]
    fn rank_with<const FMA: bool>(&self, sentences: Vec<Sentence<'_>>) -> Vec<Vec<Vec<Analysis>>> {
        let mut hidden = Vec::with_capacity(sentences.len());
        for sentence in &sentences {
            hidden.push(self.input::<FMA>(sentence));
        }
        // Four sentences of close lengths go together: the longest first, so the steps of a batch
        // are mostly those of all four
        let mut order: Vec<usize> = (0..sentences.len()).collect();
        order.sort_by_key(|&at| std::cmp::Reverse(sentences[at].words.len()));
        let d = self.model.input.rows;
        for batch in order.chunks(BATCH) {
            let mut states: Vec<Vec<f32>> = (batch.iter())
                .map(|&at| std::mem::take(&mut hidden[at]))
                .collect();
            for layer in &self.model.lstm {
                states = bilstm::<FMA>(layer, &states, d);
            }
            for (&at, states) in batch.iter().zip(states) {
                hidden[at] = states;
            }
        }
        let mut result = Vec::with_capacity(sentences.len());
        for (sentence, hidden) in sentences.into_iter().zip(&hidden) {
            result.push(self.scores(sentence.analyses, hidden));
        }
        result
    }

    /// The input of the BiLSTM for the tokens of a sentence, `n × d`: the input of a token is its
    /// shape, the mean of the tags of its analyses, the word and its endings
    #[inline(always)]
    fn input<const FMA: bool>(&self, sentence: &Sentence<'_>) -> Vec<f32> {
        let model = &self.model;
        let (words, n) = (sentence.words.as_slice(), sentence.words.len());
        let (d, dc) = (model.input.rows, model.tag_in.cols);
        let din = model.input.cols;
        let mut x = vec![0.0f32; n * din];
        let lead = (words.iter().position(|word| kind(word) != 0)).filter(|_| sentence.first);
        let (mut normal, mut key) = (String::new(), String::new());
        for (t, (word, list)) in words.iter().zip(&sentence.analyses).enumerate() {
            let row = &mut x[t * din..(t + 1) * din];
            let (shape_row, rest) = row.split_at_mut(model.shape.cols);
            shape_row.copy_from_slice(model.shape.row(shape(word, lead == Some(t))));
            let (mean, rest) = rest.split_at_mut(dc);
            let seen = &list[..list.len().min(model.kmax)];
            for analysis in seen {
                add(mean, self.sums(&self.tag_in, analysis));
            }
            for value in mean.iter_mut() {
                *value /= seen.len().max(1) as f32;
            }
            lowercase(word, &mut normal);
            let (word_row, rest) = rest.split_at_mut(model.word.cols);
            let number = model.words.get(normal.as_str()).copied().unwrap_or(1);
            word_row.copy_from_slice(model.word.row(number as usize));
            let letters = normal.chars().count();
            for (n, out) in (1..=model.suffix_max).zip(rest.chunks_exact_mut(model.suffix.cols)) {
                suffix_key(&normal, letters, n, &mut key);
                let number = model.suffixes.get(key.as_str()).copied().unwrap_or(1);
                out.copy_from_slice(model.suffix.row(number as usize));
            }
        }
        let mut hidden = vec![0.0f32; n * d];
        model
            .input
            .apply_all::<FMA>(&x, &model.input_bias, &mut hidden);
        for value in &mut hidden {
            *value = value.max(0.0);
        }
        hidden
    }

    /// The analyses of the tokens of a sentence by their scores for the output of the BiLSTM
    #[inline(always)]
    fn scores(&self, analyses: Vec<Vec<Analysis>>, hidden: &[f32]) -> Vec<Vec<Analysis>> {
        let model = &self.model;
        let (d, dc) = (model.input.rows, model.tag_in.cols);
        let mut query = vec![0.0f32; dc];
        let mut normed = vec![0.0f32; d];
        let mut result = Vec::with_capacity(analyses.len());
        for (t, mut list) in analyses.into_iter().enumerate() {
            let h = &hidden[t * d..(t + 1) * d];
            let (mut sum, mut squares) = (0.0f32, 0.0f32);
            for value in h {
                sum += value;
            }
            let mean = sum / d as f32;
            for value in h {
                squares += (value - mean) * (value - mean);
            }
            let variance = squares / d as f32;
            let scale = 1.0 / (variance + 1e-5).sqrt();
            for (i, value) in normed.iter_mut().enumerate() {
                *value = (h[i] - mean) * scale * model.norm_weight[i] + model.norm_bias[i];
            }
            model.query.apply(&normed, &mut query);
            add(&mut query, &model.query_bias);
            let seen = list.len().min(model.kmax);
            for analysis in &mut list[..seen] {
                analysis.prob = f64::from(self.score(&query, analysis));
            }
            for analysis in &mut list[seen..] {
                analysis.prob = 0.0;
            }
            softmax(&mut list[..seen]);
            result.push(list);
        }
        result
    }

    /// Score of an analysis for the query vector of its token
    #[inline(always)]
    fn score(&self, query: &[f32], analysis: &Analysis) -> f32 {
        let source = (SOURCES.iter())
            .position(|&source| source == analysis.source)
            .unwrap_or(0);
        let bias = self.slot(analysis).map_or(0.0, |at| self.tag_bias[at]);
        dot(query, self.sums(&self.tag_out, analysis))
            + dot(query, self.model.source.row(source))
            + bias
    }
}

/// Analyses with their scores in `prob` by the score, highest first, with the probabilities of
/// softmax; equal scores keep the dictionary order
fn softmax(analyses: &mut [Analysis]) {
    analyses.sort_by(|a, b| b.prob.total_cmp(&a.prob));
    let Some(top) = analyses.first().map(|analysis| analysis.prob) else {
        return;
    };
    let mut total = 0.0;
    for analysis in analyses.iter_mut() {
        analysis.prob = (analysis.prob - top).exp();
        total += analysis.prob;
    }
    for analysis in analyses {
        analysis.prob /= total;
    }
}

/// A word in lower case, as `str::to_lowercase` gives it, into `out`
fn lowercase(word: &str, out: &mut String) {
    out.clear();
    // Only the final sigma depends on the letters around it
    if word.contains('Σ') {
        out.push_str(&word.to_lowercase());
    } else {
        out.extend(word.chars().flat_map(char::to_lowercase));
    }
}

/// Key of the ending of `n` letters of a word of `letters` letters with the start mark `^`: the
/// length and the ending
fn suffix_key(word: &str, letters: usize, n: usize, key: &mut String) {
    key.clear();
    let _ = write!(key, "{n}");
    if n > letters {
        key.push('^');
        key.push_str(word);
    } else {
        let start = (word.char_indices().nth(letters - n)).map_or(0, |(at, _)| at);
        key.push_str(&word[start..]);
    }
}

/// Shape of a token, as `shape` of `scripts/train_context.py`: no letters and digits, ASCII
/// digits only, with digits, lower case, upper case, a capital letter first or mixed; the start of
/// the sentence, a hyphen and a dot in a token longer than one character
fn shape(token: &str, first: bool) -> usize {
    kind(token) * 8
        + 4 * usize::from(first)
        + 2 * usize::from(token.contains('-'))
        + usize::from(token.contains('.') && token.chars().nth(1).is_some())
}

/// Kind of the letters and digits of a token for `shape`: 5 is a capital letter first
fn kind(token: &str) -> usize {
    let (mut chars, mut digits, mut letters, mut upper, mut lower) = (0, 0, 0, 0, 0);
    // Whether the first letter is a capital one and whether another one is
    let (mut first, mut later) = (false, false);
    for c in token.chars() {
        chars += 1;
        digits += usize::from(c.is_ascii_digit());
        if c.is_alphabetic() {
            let capital = c.is_uppercase();
            if letters == 0 {
                first = capital;
            } else {
                later |= capital;
            }
            letters += 1;
            upper += usize::from(capital);
            lower += usize::from(c.is_lowercase());
        }
    }
    if digits == 0 && letters == 0 {
        0
    } else if digits == chars {
        1
    } else if digits > 0 {
        2
    } else if upper == 0 {
        3
    } else if lower == 0 && letters > 1 {
        4
    } else if first && !later {
        5
    } else {
        6
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::Dictionary;
    use crate::builder::Builder;
    use crate::dictionary::weights::tests::{entry, matrix, strings};

    /// Weights of a small model (d = 2): all weights are zeros but the biases of the tags `bias`,
    /// so the tags order the analyses
    pub(crate) fn weights(bias: &[(&str, f32)]) -> Vec<u8> {
        let tags: Vec<&str> = bias.iter().map(|&(tag, _)| tag).collect();
        let n = tags.len() + 1;
        let mut entries = Vec::new();
        let mut count = 0u32;
        let mut add = |name: &str, kind: u8, body: Vec<u8>| {
            entry(&mut entries, name, kind, &body);
            count += 1;
        };
        let config = [2u32, 1, 1, 1, 1, 1, 32, 4];
        let mut body = (config.len() as u32).to_le_bytes().to_vec();
        config
            .iter()
            .for_each(|value| body.extend_from_slice(&value.to_le_bytes()));
        add("config", 3, body);
        add("sources", 0, strings(&[r#"{"release": "r2.18"}"#]));
        add("words", 0, strings(&["la"]));
        add("suffixes", 0, strings(&[]));
        add("tags", 0, strings(&tags));
        add("inventory", 0, strings(&["P=DET", "P=PRON", "P=VERB"]));
        let biases: Vec<f32> = std::iter::once(0.0)
            .chain(bias.iter().map(|&(_, b)| b))
            .collect();
        for (name, rows, cols, values) in [
            ("word", 3, 1, &[][..]),
            ("suffix", 2, 1, &[]),
            ("shape", SHAPES, 1, &[]),
            ("tag_in", n, 1, &[]),
            ("tag_out", n, 1, &[]),
            ("tag_bias", n, 1, &biases[..]),
            ("source", SOURCES.len(), 1, &[]),
            ("input.weight", 2, 7, &[]),
            ("input.bias", 1, 2, &[]),
            ("lstm.0.forward.ih", 4, 2, &[]),
            ("lstm.0.forward.hh", 4, 1, &[]),
            ("lstm.0.forward.bias", 1, 4, &[]),
            ("lstm.0.backward.ih", 4, 2, &[]),
            ("lstm.0.backward.hh", 4, 1, &[]),
            ("lstm.0.backward.bias", 1, 4, &[]),
            ("norm.weight", 1, 2, &[]),
            ("norm.bias", 1, 2, &[]),
            ("query.weight", 1, 2, &[]),
            ("query.bias", 1, 1, &[]),
        ] {
            add(name, 1, matrix(rows, cols, values));
        }
        let mut data = MAGIC.to_vec();
        data.extend_from_slice(&FORMAT.to_le_bytes());
        data.extend_from_slice(&count.to_le_bytes());
        data.extend_from_slice(&entries);
        data
    }

    /// A dictionary of `la` (an article and a pronoun) and `vio` with the model of `weights`
    pub(crate) fn dictionary(model: Option<Vec<u8>>) -> Dictionary {
        Dictionary::from_bytes(&dictionary_bytes(model)).unwrap()
    }

    fn dictionary_bytes(model: Option<Vec<u8>>) -> Vec<u8> {
        let mut builder = Builder::new();
        builder.meta("{}");
        let article = "Definite=Def|Gender=Fem|Number=Sing|PronType=Art";
        let pronoun = "Case=Acc|Gender=Fem|Number=Sing|Person=3|PronType=Prs";
        let verb = "Mood=Ind|Number=Sing|Person=3|Tense=Past|VerbForm=Fin";
        builder.add_form("el", "DET", "la", article).unwrap();
        builder.add_form("él", "PRON", "la", pronoun).unwrap();
        builder.add_form("ver", "VERB", "vio", verb).unwrap();
        if let Some(model) = model {
            builder.context(model).unwrap();
        }
        builder.build().unwrap()
    }

    fn pos<'d>(dictionary: &'d Dictionary, analysis: &Analysis) -> &'d str {
        dictionary.ud_tag(analysis.ud).unwrap().pos()
    }

    /// Analyses of a sentence in the order of the model alone, without the constraints
    fn ranked(dictionary: &Dictionary, words: &[&str]) -> Vec<Vec<Analysis>> {
        let analyses = words.iter().map(|word| dictionary.analyze(word)).collect();
        let sentence = Sentence {
            words: words.to_vec(),
            analyses,
            first: true,
        };
        let context = dictionary.context().unwrap();
        context.rank(vec![sentence]).pop().unwrap()
    }

    #[test]
    fn training_sources() {
        let data = weights(&[("P=PRON", 0.0)]);
        assert_eq!(sources(&data).unwrap(), r#"{"release": "r2.18"}"#);
        assert!(Model::parse(&data).is_ok());
        assert_eq!(
            Builder::context_sources(&data).unwrap(),
            r#"{"release": "r2.18"}"#
        );
    }

    #[test]
    fn orders_by_context_model() {
        let dictionary = dictionary(Some(weights(&[("P=PRON", 2.0)])));
        let sentence = dictionary.analyze_sentence(&["la", "vio"]).unwrap();
        let la = &sentence[0];
        assert_eq!(la.len(), 2);
        assert_eq!(pos(&dictionary, &la[0]), "PRON");
        let expected = 1.0 / (1.0 + (-2.0f64).exp());
        assert!((la[0].prob - expected).abs() < 1e-6, "{}", la[0].prob);
        assert!((la[0].prob + la[1].prob - 1.0).abs() < 1e-9);
        assert_eq!(sentence[1][0].prob, 1.0);
        // An empty sentence, a token without neighbors as `analyze`, a copy - the same model
        assert!(dictionary.analyze_sentence::<&str>(&[]).unwrap().is_empty());
        assert_eq!(
            dictionary.analyze_sentence(&["la"]).unwrap(),
            vec![dictionary.analyze("la")]
        );
        let copy = dictionary.clone();
        assert_eq!(
            copy.analyze_sentence(&["la", "vio"]).unwrap()[0][0].ud,
            la[0].ud
        );
        assert!(dictionary.has_context());
    }

    #[test]
    fn a_number_with_its_sign_is_one_word_for_the_model() {
        let dictionary = dictionary(Some(weights(&[("P=PRON", 0.0)])));
        let context = dictionary.context().unwrap();
        let la = dictionary.analyze("la").remove(0);
        let with = |source, word: &str, words: &[&str]| Analysis {
            source,
            word: word.to_owned(),
            words: words.iter().map(|&word| word.to_owned()).collect(),
            ..la.clone()
        };
        let one = context.slot(&la);
        assert!(one.is_some());
        assert_eq!(context.slot(&with(Source::Rule, "60%", &["60", "%"])), one);
        assert_eq!(
            context.slot(&with(Source::Dictionary, "km", &["km.", "24"])),
            one
        );
        let two = one.map(|at| at + 1);
        assert_eq!(
            context.slot(&with(Source::Clitics, "dala", &["da", "la"])),
            two
        );
        assert_eq!(
            context.slot(&with(Source::Dictionary, "del", &["de", "el"])),
            two
        );
    }

    #[test]
    fn combined_tags() {
        // The tag of the part of speech, features and words together outweighs the pronoun
        let article = "C=DET|Definite=Def|Gender=Fem|Number=Sing|PronType=Art|1";
        let dictionary = dictionary(Some(weights(&[("P=PRON", 2.0), (article, 3.0)])));
        let sentence = ranked(&dictionary, &["la", "vio"]);
        assert_eq!(pos(&dictionary, &sentence[0][0]), "DET");
        // A tag of two words does not fit a token of one
        let two = "C=DET|Definite=Def|Gender=Fem|Number=Sing|PronType=Art|2";
        let dictionary = super::tests::dictionary(Some(weights(&[("P=PRON", 2.0), (two, 3.0)])));
        let sentence = ranked(&dictionary, &["la", "vio"]);
        assert_eq!(pos(&dictionary, &sentence[0][0]), "PRON");
    }

    #[test]
    #[cfg_attr(
        miri,
        ignore = "Miri perturbs `exp` on purpose, so equal scores differ in the last bits"
    )]
    fn long_sentence_in_chunks() {
        let dictionary = dictionary(Some(weights(&[("P=PRON", 2.0)])));
        let words: Vec<&str> = ["la", "vio"]
            .into_iter()
            .cycle()
            .take(crate::SENTENCE_CHUNK + 3)
            .collect();
        let (head, tail) = words.split_at(crate::SENTENCE_CHUNK);
        let mut chunks = dictionary.analyze_sentence(head).unwrap();
        chunks.extend(dictionary.analyze_sentence_part(tail, false).unwrap());
        assert_eq!(dictionary.analyze_sentence(&words).unwrap(), chunks);
    }

    #[cfg(feature = "bundled")]
    #[test]
    fn parts_of_a_long_sentence() {
        // Only the first part starts the sentence: the tail as a sentence of its own differs
        let dictionary = Dictionary::bundled().unwrap();
        let words: Vec<&str> = ["la", "casa", "es", "grande"]
            .into_iter()
            .cycle()
            .take(2 * crate::SENTENCE_CHUNK + 4)
            .collect();
        let whole = dictionary.analyze_sentence(&words).unwrap();
        let (head, tail) = words.split_at(crate::SENTENCE_CHUNK);
        let mut parts = dictionary.analyze_sentence_part(head, true).unwrap();
        parts.extend(dictionary.analyze_sentence_part(tail, false).unwrap());
        assert_eq!(parts, whole);
        let alone = dictionary.analyze_sentence(tail).unwrap();
        assert_ne!(alone[0][0].prob, whole[crate::SENTENCE_CHUNK][0].prob);
    }

    #[test]
    fn verify_reads_the_model() {
        // A model damaged under a right checksum: loading takes it, `verify` does not
        let data = dictionary_bytes(Some(weights(&[("P=PRON", 1234.5)])));
        assert!(Dictionary::from_bytes(&data).unwrap().verify().is_ok());
        let damage = |pattern: &[u8], with: &[u8]| {
            let mut damaged = data.clone();
            let at = (damaged.windows(pattern.len()))
                .position(|window| window == pattern)
                .unwrap();
            damaged[at..at + with.len()].copy_from_slice(with);
            Dictionary::from_bytes(&crate::format::tests::with_checksum(damaged)).unwrap()
        };
        for weight in [f32::NAN, f32::INFINITY, 3e38] {
            let wrong = damage(&1234.5f32.to_le_bytes(), &weight.to_le_bytes());
            let err = wrong.verify().unwrap_err();
            assert!(err.to_string().contains("out of range"), "{weight}: {err}");
            let err = wrong.analyze_sentence(&["la", "vio"]).unwrap_err();
            assert!(matches!(err, Error::Format(_)), "{err}");
        }
        let unsigned = damage(MAGIC, b"X");
        let err = unsigned.verify().unwrap_err();
        assert!(err.to_string().contains("no signature"), "{err}");
    }

    #[cfg(feature = "bundled")]
    #[test]
    fn sentence_after_a_dash() {
        // A dash of a dialogue, a quote or a bracket before its first word: the word still starts
        // the sentence and is not taken for a name
        let dictionary = Dictionary::bundled().unwrap();
        for (sentence, wanted) in [
            (&["Mira", "lo", "que", "has", "hecho", "."][..], "VERB"),
            (&["Hombre", ",", "ven", "."], "NOUN"),
            (&["Pobre", "mujer", "."], "ADJ"),
        ] {
            for lead in ["—", "--", "«", "(", "¡", "\""] {
                let words: Vec<&str> = std::iter::once(lead)
                    .chain(sentence.iter().copied())
                    .collect();
                let analyses = dictionary.analyze_sentence(&words).unwrap();
                assert_eq!(
                    pos(&dictionary, &analyses[1][0]),
                    wanted,
                    "{lead} {}",
                    words[1]
                );
            }
        }
    }

    #[test]
    fn dictionary_without_model() {
        let dictionary = dictionary(None);
        assert!(!dictionary.has_context());
        let err = dictionary.analyze_sentence(&["la"]).unwrap_err();
        assert!(matches!(err, Error::Context(_)), "{err}");
        assert!(err.to_string().contains("no context model"), "{err}");
    }

    #[cfg(feature = "build")]
    #[test]
    fn tags_the_model_has_not() {
        let dictionary = dictionary(Some(weights(&[("P=PRON", 2.0)])));
        let gaps = dictionary.context_gaps().unwrap();
        assert!(gaps.contains(&"F=Case=Acc".to_owned()), "{gaps:?}");
        assert!(!gaps.contains(&"P=PRON".to_owned()), "{gaps:?}");
    }

    #[test]
    fn token_shapes() {
        assert_eq!(shape(",", false), 0);
        assert_eq!(shape("...", false), 1);
        assert_eq!(shape("1999", false), 8);
        assert_eq!(shape("1,5", false), 16);
        assert_eq!(shape("2-1", false), 18);
        assert_eq!(shape("casa", false), 24);
        assert_eq!(shape("ONU", false), 32);
        assert_eq!(shape("Universidad", true), 44);
        assert_eq!(shape("Sr.", false), 41);
        assert_eq!(shape("EE.UU.", false), 33);
        assert_eq!(shape("McDonald", false), 48);
        assert_eq!(shape("Castilla-La", false), 50);
        assert_eq!(shape("y", true), 28);
        assert_eq!(kind("Ñandú"), 5);
        assert_eq!(kind("É"), 5);
    }

    #[test]
    fn word_suffixes() {
        let keys = |word: &str| -> Vec<String> {
            let mut key = String::new();
            (1..=4)
                .map(|n| {
                    suffix_key(word, word.chars().count(), n, &mut key);
                    key.clone()
                })
                .collect()
        };
        assert_eq!(keys("casa"), ["1a", "2sa", "3asa", "4casa"]);
        assert_eq!(keys("él"), ["1l", "2él", "3^él", "4^él"]);
        assert_eq!(keys(""), ["1^", "2^", "3^", "4^"]);
    }

    #[test]
    fn lower_case() {
        let mut out = String::new();
        for word in ["CASA", "Ñandú", "İstanbul", "ΣΟΦΟΣ", "ΣΑ Σ", "ǅ"] {
            lowercase(word, &mut out);
            assert_eq!(out, word.to_lowercase());
        }
    }

    #[test]
    fn repeated_strings() {
        // A repeated tag: the number of the row of the second would leave the table of tags
        let data = weights(&[("P=PRON", 1.0), ("P=PRON", 2.0)]);
        let err = Model::parse(&data).unwrap_err().to_string();
        assert!(err.contains("tags repeat a string"), "{err}");
    }

    #[test]
    fn broken_weights() {
        assert!(Model::parse(b"").is_err());
        assert!(Model::parse(b"ESMCTX\0\0\x02\0\0\0\0\0\0\0").is_err());
        let mut data = MAGIC.to_vec();
        data.extend_from_slice(&FORMAT.to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes());
        let err = Model::parse(&data).unwrap_err().to_string();
        assert!(err.contains("no entry config"), "{err}");
        // Any cut of good weights is an error, not a panic
        let data = weights(&[("P=PRON", 2.0)]);
        for end in 0..data.len() {
            assert!(Model::parse(&data[..end]).is_err(), "{end}");
        }
        let mut longer = data.clone();
        longer.push(0);
        assert!(Model::parse(&longer).is_err());
        // A damaged model is an error of the format, not a missing model
        let mut builder = Builder::new();
        builder.add_form("el", "DET", "la", "").unwrap();
        assert!(builder.context(data[..data.len() - 1].to_vec()).is_err());
    }
}
