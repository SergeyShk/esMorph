//! Context model: the choice of the analysis of a token by the tokens of its sentence. The input of
//! a token is its shape, its word, its endings and the tags of its analyses; a BiLSTM runs over the
//! sentence, and the score of an analysis is a dot product with the vector of its token. The
//! weights are a section of the dictionary, the file of `scripts/train_context.py`: a signature,
//! a version and entries "name, kind, data", numbers little-endian

use std::collections::HashMap;
use std::fmt::Write;

use super::{Analysis, Source};
use crate::Error;
use crate::ud::UdTags;

const MAGIC: &[u8; 8] = b"ESMCTX\0\0";
const FORMAT: u32 = 1;
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
/// Largest sizes from the file: a damaged file does not ask for gigabytes
const MAX_DIM: usize = 4096;
const MAX_ROWS: usize = 1 << 22;

/// A matrix of f32 by rows
#[derive(Debug)]
struct Matrix {
    rows: usize,
    cols: usize,
    data: Vec<f32>,
}

impl Matrix {
    fn row(&self, row: usize) -> &[f32] {
        &self.data[row * self.cols..(row + 1) * self.cols]
    }

    /// `out = self · input`
    fn apply(&self, input: &[f32], out: &mut [f32]) {
        for (value, row) in out.iter_mut().zip(self.data.chunks_exact(self.cols)) {
            *value = dot(row, input);
        }
    }
}

/// Rows of a tile of `Tiled`
const TILE: usize = 16;
/// Vectors that a tile multiplies at once: `Tiled::apply_all` keeps this many sums
const BATCH: usize = 4;
const _: () = assert!(BATCH == 4);

/// A matrix by tiles of `TILE` rows for the product with many vectors: in a tile the columns go one
/// after another, a column holds the numbers of `TILE` rows. The sums of a tile for `BATCH` vectors
/// stay in registers, and every number of the matrix is read once for `BATCH` vectors
#[derive(Debug)]
struct Tiled {
    rows: usize,
    cols: usize,
    columns: Vec<[f32; TILE]>,
}

impl Tiled {
    fn new(matrix: &Matrix) -> Self {
        let tiles = matrix.rows.div_ceil(TILE);
        let mut columns = vec![[0.0; TILE]; tiles * matrix.cols];
        for row in 0..matrix.rows {
            let (tile, offset) = (row / TILE, row % TILE);
            for (col, &value) in matrix.row(row).iter().enumerate() {
                columns[tile * matrix.cols + col][offset] = value;
            }
        }
        Tiled {
            rows: matrix.rows,
            cols: matrix.cols,
            columns,
        }
    }

    /// `out += self · input` for one vector
    fn apply_add(&self, input: &[f32], out: &mut [f32]) {
        for (tile, columns) in self.columns.chunks_exact(self.cols).enumerate() {
            let mut sums = [0.0f32; TILE];
            for (column, &value) in columns.iter().zip(input) {
                axpy(&mut sums, column, value);
            }
            let start = tile * TILE;
            let end = (start + TILE).min(self.rows);
            for (out, sum) in out[start..end].iter_mut().zip(sums) {
                *out += sum;
            }
        }
    }

    /// `out[t] = self · inputs[t] + bias` for all vectors of `inputs` in a row
    fn apply_all(&self, inputs: &[f32], bias: &[f32], out: &mut [f32]) {
        let count = inputs.len() / self.cols;
        let mut padded = vec![[0.0f32; BATCH]; self.cols];
        for first in (0..count).step_by(BATCH) {
            let batch = (count - first).min(BATCH);
            // The vectors of a batch by columns: a column holds the numbers of `BATCH` vectors
            for (col, values) in padded.iter_mut().enumerate() {
                for (b, value) in values.iter_mut().enumerate() {
                    *value = if b < batch {
                        inputs[(first + b) * self.cols + col]
                    } else {
                        0.0
                    };
                }
            }
            for (tile, columns) in self.columns.chunks_exact(self.cols).enumerate() {
                // Four sums apart, not an array: so they stay in registers in any build profile
                let (mut s0, mut s1, mut s2, mut s3) =
                    ([0.0f32; TILE], [0.0; TILE], [0.0; TILE], [0.0; TILE]);
                for (column, values) in columns.iter().zip(&padded) {
                    axpy(&mut s0, column, values[0]);
                    axpy(&mut s1, column, values[1]);
                    axpy(&mut s2, column, values[2]);
                    axpy(&mut s3, column, values[3]);
                }
                let sums = [s0, s1, s2, s3];
                for (b, sums) in sums.iter().enumerate().take(batch) {
                    for (j, sum) in sums.iter().enumerate() {
                        let row = tile * TILE + j;
                        if row < self.rows {
                            out[(first + b) * self.rows + row] = sum + bias[row];
                        }
                    }
                }
            }
        }
    }
}

/// `sums += value · column`
#[inline(always)]
fn axpy(sums: &mut [f32; TILE], column: &[f32; TILE], value: f32) {
    for (sum, &weight) in sums.iter_mut().zip(column) {
        *sum += value * weight;
    }
}

/// Dot product by blocks of 8: so the loop is vectorized
fn dot(a: &[f32], b: &[f32]) -> f32 {
    let mut sums = [0.0f32; 8];
    let (left, right) = (a.as_chunks::<8>(), b.as_chunks::<8>());
    for (x, y) in left.0.iter().zip(right.0) {
        for i in 0..8 {
            sums[i] += x[i] * y[i];
        }
    }
    let tail: f32 = left.1.iter().zip(right.1).map(|(x, y)| x * y).sum();
    sums.iter().sum::<f32>() + tail
}

fn add(out: &mut [f32], values: &[f32]) {
    for (out, value) in out.iter_mut().zip(values) {
        *out += value;
    }
}

/// e^x with a relative error of about 2e-7, without libm and branches, so a loop over an array is
/// vectorized. Rounding adds 1.5·2²³: SSE2 has no vector rounding
#[inline(always)]
fn exp(x: f32) -> f32 {
    const ROUND: f32 = 12_582_912.0;
    let x = x.clamp(-87.0, 88.0);
    let n = (x * std::f32::consts::LOG2_E + ROUND) - ROUND;
    // ln 2 in two parts: the remainder is exact for large n too
    let r = x - n * 0.693_145_75 - n * 1.428_606_8e-6;
    let p = 1.0
        + r * (1.0
            + r * (0.5 + r * (1.0 / 6.0 + r * (1.0 / 24.0 + r * (1.0 / 120.0 + r / 720.0)))));
    p * f32::from_bits(((n as i32 + 127) as u32) << 23)
}

#[inline(always)]
fn sigmoid(x: f32) -> f32 {
    1.0 / (1.0 + exp(-x))
}

#[inline(always)]
fn tanh(x: f32) -> f32 {
    2.0 / (1.0 + exp(-2.0 * x)) - 1.0
}

/// An LSTM cell of one direction: the matrices of the gates in the order of PyTorch (i, f, g, o),
/// the biases summed
#[derive(Debug)]
struct Lstm {
    input: Tiled,
    hidden: Tiled,
    bias: Vec<f32>,
}

/// Value of an entry of the file of weights
enum Value<'a> {
    Strings(Vec<&'a str>),
    Matrix(Matrix),
    Numbers(Vec<u32>),
}

/// Reading of the file of weights with checked bounds
struct Reader<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8], Error> {
        let end = (self.at.checked_add(count)).filter(|&end| end <= self.data.len());
        let end = end.ok_or_else(|| invalid("the file of weights is truncated"))?;
        let bytes = &self.data[self.at..end];
        self.at = end;
        Ok(bytes)
    }

    fn u32(&mut self) -> Result<u32, Error> {
        let bytes = self.take(4)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn count(&mut self, limit: usize) -> Result<usize, Error> {
        let count = self.u32()? as usize;
        if count > limit {
            return Err(invalid("an entry is larger than allowed"));
        }
        Ok(count)
    }

    fn f32s(&mut self, count: usize) -> Result<Vec<f32>, Error> {
        let size = count.checked_mul(4).ok_or_else(|| invalid("size"))?;
        let bytes = self.take(size)?;
        Ok((bytes.as_chunks::<4>().0.iter())
            .map(|&chunk| f32::from_le_bytes(chunk))
            .collect())
    }

    fn value(&mut self) -> Result<Value<'a>, Error> {
        let kind = self.take(1)?[0];
        match kind {
            0 => {
                let count = self.count(MAX_ROWS)?;
                let mut strings = Vec::with_capacity(count.min(1 << 16));
                for _ in 0..count {
                    let len = self.count(1 << 16)?;
                    let text = std::str::from_utf8(self.take(len)?)
                        .map_err(|_| invalid("a string is not UTF-8"))?;
                    strings.push(text);
                }
                Ok(Value::Strings(strings))
            }
            1 | 2 => {
                let (rows, cols) = (self.count(MAX_ROWS)?, self.count(MAX_DIM)?);
                let size = rows.checked_mul(cols).ok_or_else(|| invalid("size"))?;
                let data = if kind == 1 {
                    self.f32s(size)?
                } else {
                    let scales = self.f32s(rows)?;
                    let values = self.take(size)?;
                    (values.chunks_exact(cols.max(1)).zip(&scales))
                        .flat_map(|(row, &scale)| {
                            row.iter().map(move |&q| f32::from(q as i8) * scale)
                        })
                        .collect()
                };
                Ok(Value::Matrix(Matrix { rows, cols, data }))
            }
            3 => {
                let count = self.count(64)?;
                (0..count)
                    .map(|_| self.u32())
                    .collect::<Result<_, _>>()
                    .map(Value::Numbers)
            }
            _ => Err(invalid(&format!("unknown kind of entry {kind}"))),
        }
    }
}

fn invalid(message: &str) -> Error {
    Error::Format(format!("context model: {message}"))
}

/// Entries of the file of weights by name
struct Entries<'a>(HashMap<&'a str, Value<'a>>);

impl<'a> Entries<'a> {
    fn take(&mut self, name: &str) -> Result<Value<'a>, Error> {
        (self.0.remove(name)).ok_or_else(|| invalid(&format!("no entry {name}")))
    }

    fn matrix(&mut self, name: &str, rows: usize, cols: usize) -> Result<Matrix, Error> {
        match self.take(name)? {
            Value::Matrix(matrix) if matrix.rows == rows && matrix.cols == cols => Ok(matrix),
            Value::Matrix(matrix) => Err(invalid(&format!(
                "{name}: size {}×{}, expected {rows}×{cols}",
                matrix.rows, matrix.cols
            ))),
            _ => Err(invalid(&format!("{name} is not a matrix"))),
        }
    }

    fn vector(&mut self, name: &str, size: usize) -> Result<Vec<f32>, Error> {
        Ok(self.matrix(name, 1, size)?.data)
    }

    fn strings(&mut self, name: &str) -> Result<Vec<&'a str>, Error> {
        match self.take(name)? {
            Value::Strings(strings) => Ok(strings),
            _ => Err(invalid(&format!("{name} are not strings"))),
        }
    }

    /// Numbers of the rows of a table by the strings of the entry `name` from the number `first`;
    /// a repeated string is an error: the number of a row would leave the table
    fn index(&mut self, name: &str, first: u32) -> Result<HashMap<String, u32>, Error> {
        let strings = self.strings(name)?;
        let index: HashMap<String, u32> = (strings.iter().enumerate())
            .map(|(number, text)| ((*text).to_owned(), number as u32 + first))
            .collect();
        if index.len() != strings.len() {
            return Err(invalid(&format!("{name} repeat a string")));
        }
        Ok(index)
    }
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

/// Entries of the file of weights: the signature, the version, the number of entries and the
/// entries
fn read(data: &[u8]) -> Result<Entries<'_>, Error> {
    if data.get(..MAGIC.len()) != Some(MAGIC.as_slice()) {
        return Err(invalid("no signature of a file of weights"));
    }
    let mut reader = Reader {
        data,
        at: MAGIC.len(),
    };
    let format = reader.u32()?;
    if format != FORMAT {
        return Err(invalid(&format!(
            "version of the file of weights {format}, supported {FORMAT}"
        )));
    }
    let count = reader.count(256)?;
    let mut entries = HashMap::new();
    for _ in 0..count {
        let len = usize::from(u16::from_le_bytes([reader.take(1)?[0], reader.take(1)?[0]]));
        let name = std::str::from_utf8(reader.take(len)?)
            .map_err(|_| invalid("the name of an entry is not UTF-8"))?;
        entries.insert(name, reader.value()?);
    }
    if reader.at != data.len() {
        return Err(invalid("data after the entries"));
    }
    Ok(Entries(entries))
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
        let at = usize::from(analysis.ud) * MAX_WORDS + words - 1;
        (at < self.tag_bias.len()).then_some(at)
    }

    fn sums<'a>(&self, table: &'a [f32], analysis: &Analysis) -> &'a [f32] {
        let dc = self.model.tag_in.cols;
        self.slot(analysis)
            .map_or(&[], |at| &table[at * dc..(at + 1) * dc])
    }

    /// Analyses of the tokens of a sentence by the score of the model, highest first; the
    /// probability is the share in the context. The model sees the first `kmax` analyses of a
    /// token, the others follow them with the probability 0. `words` are the tokens normalized as
    /// `analyze` reads them, `first` - whether they start the sentence
    pub fn rank(
        &self,
        words: &[&str],
        analyses: Vec<Vec<Analysis>>,
        first: bool,
    ) -> Vec<Vec<Analysis>> {
        let model = &self.model;
        let n = words.len();
        if n == 0 {
            return analyses;
        }
        let (d, dc) = (model.input.rows, model.tag_in.cols);
        let din = model.input.cols;
        let mut x = vec![0.0f32; n * din];
        for (t, (word, list)) in words.iter().zip(&analyses).enumerate() {
            // Input of a token: the shape, the mean of the tags of its analyses, the word and its
            // endings
            let row = &mut x[t * din..(t + 1) * din];
            let (shape_row, rest) = row.split_at_mut(model.shape.cols);
            shape_row.copy_from_slice(model.shape.row(shape(word, first && t == 0)));
            let (mean, rest) = rest.split_at_mut(dc);
            let seen = &list[..list.len().min(model.kmax)];
            for analysis in seen {
                add(mean, self.sums(&self.tag_in, analysis));
            }
            for value in mean.iter_mut() {
                *value /= seen.len().max(1) as f32;
            }
            let normal = word.to_lowercase();
            let (word_row, rest) = rest.split_at_mut(model.word.cols);
            let number = model.words.get(&normal).copied().unwrap_or(1);
            word_row.copy_from_slice(model.word.row(number as usize));
            let suffix_rows = rest.chunks_exact_mut(model.suffix.cols);
            for (key, out) in suffixes(&normal, model.suffix_max).zip(suffix_rows) {
                let number = model.suffixes.get(&key).copied().unwrap_or(1);
                out.copy_from_slice(model.suffix.row(number as usize));
            }
        }

        let mut hidden = vec![0.0f32; n * d];
        model.input.apply_all(&x, &model.input_bias, &mut hidden);
        for value in &mut hidden {
            *value = value.max(0.0);
        }
        for layer in &model.lstm {
            hidden = bilstm(layer, &hidden, n, d);
        }

        let mut query = vec![0.0f32; dc];
        let mut normed = vec![0.0f32; d];
        let mut result = Vec::with_capacity(n);
        for (t, list) in analyses.into_iter().enumerate() {
            let h = &hidden[t * d..(t + 1) * d];
            let mean = h.iter().sum::<f32>() / d as f32;
            let variance = h.iter().map(|v| (v - mean) * (v - mean)).sum::<f32>() / d as f32;
            let scale = 1.0 / (variance + 1e-5).sqrt();
            for (i, value) in normed.iter_mut().enumerate() {
                *value = (h[i] - mean) * scale * model.norm_weight[i] + model.norm_bias[i];
            }
            model.query.apply(&normed, &mut query);
            add(&mut query, &model.query_bias);
            let scores: Vec<f32> = (list.iter().take(model.kmax))
                .map(|analysis| self.score(&query, analysis))
                .collect();
            result.push(reorder(list, &scores));
        }
        result
    }

    /// Score of an analysis for the query vector of its token
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

/// Analyses by the scores `scores` of the first of them, highest first, with the probabilities of
/// softmax; the others follow them with the probability 0. Equal scores keep the dictionary order
fn reorder(list: Vec<Analysis>, scores: &[f32]) -> Vec<Analysis> {
    let top = scores.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let exps: Vec<f64> = scores.iter().map(|&s| f64::from(s - top).exp()).collect();
    let total: f64 = exps.iter().sum();
    let mut order: Vec<usize> = (0..scores.len()).collect();
    order.sort_by(|&a, &b| scores[b].total_cmp(&scores[a]));
    let mut slots: Vec<Option<Analysis>> = list.into_iter().map(Some).collect();
    let mut result = Vec::with_capacity(slots.len());
    for index in order {
        if let Some(mut analysis) = slots[index].take() {
            analysis.prob = exps[index] / total;
            result.push(analysis);
        }
    }
    for mut analysis in slots.into_iter().flatten() {
        analysis.prob = 0.0;
        result.push(analysis);
    }
    result
}

/// A layer of a BiLSTM: the output of a token is the states of the forward and the backward
/// direction in a row. Both directions go in one loop: their chains are independent, and the
/// processor runs them at once
fn bilstm(layer: &[Lstm; 2], input: &[f32], n: usize, d: usize) -> Vec<f32> {
    let hidden = d / 2;
    let mut output = vec![0.0f32; n * d];
    // The input part of all gates for all tokens at once
    let gates_input = layer.each_ref().map(|cell| {
        let mut gates = vec![0.0f32; n * 4 * hidden];
        cell.input.apply_all(input, &cell.bias, &mut gates);
        gates
    });
    let mut states = [(); 2].map(|()| (vec![0.0f32; hidden], vec![0.0f32; hidden]));
    let mut gates = [(); 2].map(|()| vec![0.0f32; 4 * hidden]);
    for step in 0..n {
        for direction in 0..2 {
            let t = if direction == 0 { step } else { n - 1 - step };
            let (h, c) = &mut states[direction];
            let gates = &mut gates[direction];
            gates.copy_from_slice(&gates_input[direction][t * 4 * hidden..(t + 1) * 4 * hidden]);
            layer[direction].hidden.apply_add(h, gates);
            // Gates i, f, g, o; every loop goes over an array without branches
            let (sigmoids, rest) = gates.split_at_mut(2 * hidden);
            let (candidates, outputs) = rest.split_at_mut(hidden);
            sigmoids
                .iter_mut()
                .for_each(|value| *value = sigmoid(*value));
            candidates
                .iter_mut()
                .for_each(|value| *value = tanh(*value));
            outputs
                .iter_mut()
                .for_each(|value| *value = sigmoid(*value));
            let (inputs, forgets) = sigmoids.split_at(hidden);
            for i in 0..hidden {
                c[i] = forgets[i] * c[i] + inputs[i] * candidates[i];
            }
            for i in 0..hidden {
                h[i] = outputs[i] * tanh(c[i]);
            }
            output[t * d + direction * hidden..t * d + (direction + 1) * hidden].copy_from_slice(h);
        }
    }
    output
}

/// Endings of a word of 1..=`max` letters with the start mark `^`: the key is the length and the
/// ending
fn suffixes(word: &str, max: usize) -> impl Iterator<Item = String> {
    let marked: Vec<char> = std::iter::once('^').chain(word.chars()).collect();
    (1..=max).map(move |n| {
        let tail: String = marked[marked.len().saturating_sub(n)..].iter().collect();
        format!("{n}{tail}")
    })
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
    let digits = token.chars().filter(char::is_ascii_digit).count();
    let letters: Vec<char> = token.chars().filter(|c| c.is_alphabetic()).collect();
    if digits == 0 && letters.is_empty() {
        0
    } else if digits == token.chars().count() {
        1
    } else if digits > 0 {
        2
    } else if letters.iter().all(|c| !c.is_uppercase()) {
        3
    } else if letters.iter().all(|c| !c.is_lowercase()) && letters.len() > 1 {
        4
    } else if letters[0].is_uppercase() && letters[1..].iter().all(|c| !c.is_uppercase()) {
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

    fn entry(out: &mut Vec<u8>, name: &str, kind: u8, body: &[u8]) {
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(name.as_bytes());
        out.push(kind);
        out.extend_from_slice(body);
    }

    fn matrix(rows: usize, cols: usize, values: &[f32]) -> Vec<u8> {
        let mut body = Vec::new();
        body.extend_from_slice(&(rows as u32).to_le_bytes());
        body.extend_from_slice(&(cols as u32).to_le_bytes());
        for row in 0..rows {
            for col in 0..cols {
                let value = values.get(row * cols + col).copied().unwrap_or(0.0);
                body.extend_from_slice(&value.to_le_bytes());
            }
        }
        body
    }

    fn strings(items: &[&str]) -> Vec<u8> {
        let mut body = (items.len() as u32).to_le_bytes().to_vec();
        for item in items {
            body.extend_from_slice(&(item.len() as u32).to_le_bytes());
            body.extend_from_slice(item.as_bytes());
        }
        body
    }

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
        Dictionary::from_bytes(&builder.build().unwrap()).unwrap()
    }

    fn pos<'d>(dictionary: &'d Dictionary, analysis: &Analysis) -> &'d str {
        dictionary.ud_tag(analysis.ud).unwrap().pos()
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
    fn combined_tags() {
        // The tag of the part of speech, features and words together outweighs the pronoun
        let article = "C=DET|Definite=Def|Gender=Fem|Number=Sing|PronType=Art|1";
        let dictionary = dictionary(Some(weights(&[("P=PRON", 2.0), (article, 3.0)])));
        let sentence = dictionary.analyze_sentence(&["la", "vio"]).unwrap();
        assert_eq!(pos(&dictionary, &sentence[0][0]), "DET");
        // A tag of two words does not fit a token of one
        let two = "C=DET|Definite=Def|Gender=Fem|Number=Sing|PronType=Art|2";
        let dictionary = super::tests::dictionary(Some(weights(&[("P=PRON", 2.0), (two, 3.0)])));
        let sentence = dictionary.analyze_sentence(&["la", "vio"]).unwrap();
        assert_eq!(pos(&dictionary, &sentence[0][0]), "PRON");
    }

    #[test]
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
    fn lstm_recurrence() {
        // A hidden state of 1: the gates are constant (only the biases), the state grows by steps
        let bias = vec![0.5f32, -0.3, 0.8, 0.2];
        let zeros = |cols: usize| {
            Tiled::new(&Matrix {
                rows: 4,
                cols,
                data: vec![0.0; 4 * cols],
            })
        };
        let cell = || Lstm {
            input: zeros(2),
            hidden: zeros(1),
            bias: bias.clone(),
        };
        let output = bilstm(&[cell(), cell()], &[0.0; 6], 3, 2);
        let (i, f, g, o) = (sigmoid(0.5), sigmoid(-0.3), 0.8f32.tanh(), sigmoid(0.2));
        let mut states = Vec::new();
        let mut c = 0.0;
        for _ in 0..3 {
            c = f * c + i * g;
            states.push(o * c.tanh());
        }
        for t in 0..3 {
            assert!((output[t * 2] - states[t]).abs() < 1e-6);
            assert!((output[t * 2 + 1] - states[2 - t]).abs() < 1e-6);
        }
    }

    #[test]
    fn tiled_products() {
        // 18 rows: two tiles, the second one partly empty; 5 vectors: two batches
        let (rows, cols) = (18, 3);
        let data: Vec<f32> = (0..rows * cols).map(|i| (i % 7) as f32 - 3.0).collect();
        let matrix = Matrix { rows, cols, data };
        let tiled = Tiled::new(&matrix);
        let inputs: Vec<f32> = (0..5 * cols).map(|i| i as f32 * 0.5).collect();
        let bias: Vec<f32> = (0..rows).map(|i| i as f32).collect();
        let mut out = vec![0.0; 5 * rows];
        tiled.apply_all(&inputs, &bias, &mut out);
        for t in 0..5 {
            let mut expected = vec![0.0; rows];
            matrix.apply(&inputs[t * cols..(t + 1) * cols], &mut expected);
            add(&mut expected, &bias);
            assert_eq!(out[t * rows..(t + 1) * rows], expected[..]);
            let mut added = bias.clone();
            tiled.apply_add(&inputs[t * cols..(t + 1) * cols], &mut added);
            assert_eq!(added, expected);
        }
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
        let keys: Vec<String> = suffixes("casa", 4).collect();
        assert_eq!(keys, ["1a", "2sa", "3asa", "4casa"]);
        let keys: Vec<String> = suffixes("él", 4).collect();
        assert_eq!(keys, ["1l", "2él", "3^él", "4^él"]);
    }

    #[test]
    fn dot_products() {
        let a: Vec<f32> = (0..19).map(|i| i as f32).collect();
        let b = vec![2.0f32; 19];
        assert_eq!(dot(&a, &b), 342.0);
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
