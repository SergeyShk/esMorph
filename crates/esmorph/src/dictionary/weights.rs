//! Weights of the models of the dictionary: a file of entries "name, kind, data" with the
//! signature of the model and a version, numbers little-endian; matrices and the kernels of LSTM
//! for the forward pass

use std::collections::HashMap;

use crate::Error;

/// Largest sizes from the file: a damaged file does not ask for gigabytes
pub(super) const MAX_DIM: usize = 4096;
const MAX_ROWS: usize = 1 << 22;
/// Largest weight: a trained model has weights below 10, and with larger ones the sums of a
/// crafted file would overflow to infinity and give probabilities NaN
const MAX_WEIGHT: f32 = 1e4;

/// A matrix of f32 by rows
#[derive(Debug)]
pub(super) struct Matrix {
    pub(super) rows: usize,
    pub(super) cols: usize,
    pub(super) data: Vec<f32>,
}

impl Matrix {
    pub(super) fn row(&self, row: usize) -> &[f32] {
        &self.data[row * self.cols..(row + 1) * self.cols]
    }

    /// `out = self · input`
    #[inline(always)]
    pub(super) fn apply(&self, input: &[f32], out: &mut [f32]) {
        for (value, row) in out.iter_mut().zip(self.data.chunks_exact(self.cols)) {
            *value = dot(row, input);
        }
    }
}

/// Rows of a tile of `Tiled`
const TILE: usize = 16;
/// Whether the products of the model fuse their additions without a check of the processor:
/// arm64 always has a fused multiply-add, a build for x86-64 may have it
pub(super) const FUSED: bool = cfg!(any(target_arch = "aarch64", target_feature = "fma"));
/// Vectors that a tile multiplies at once: `Tiled::apply_all` keeps this many sums
pub(super) const BATCH: usize = 4;
const _: () = assert!(BATCH == 4);

/// A matrix by tiles of `TILE` rows for the product with many vectors: in a tile the columns go one
/// after another, a column holds the numbers of `TILE` rows. The sums of a tile for `BATCH` vectors
/// stay in registers, and every number of the matrix is read once for `BATCH` vectors
#[derive(Debug)]
pub(super) struct Tiled {
    pub(super) rows: usize,
    pub(super) cols: usize,
    columns: Vec<[f32; TILE]>,
}

impl Tiled {
    pub(super) fn new(matrix: &Matrix) -> Self {
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
    #[inline(always)]
    pub(super) fn apply_add<const FMA: bool>(&self, input: &[f32], out: &mut [f32]) {
        for (tile, columns) in self.columns.chunks_exact(self.cols).enumerate() {
            let mut sums = [0.0f32; TILE];
            for (column, &value) in columns.iter().zip(input) {
                axpy::<FMA>(&mut sums, column, value);
            }
            let start = tile * TILE;
            let end = (start + TILE).min(self.rows);
            for (out, sum) in out[start..end].iter_mut().zip(sums) {
                *out += sum;
            }
        }
    }

    /// `out[t] = self · inputs[t] + bias` for all vectors of `inputs` in a row
    #[inline(always)]
    pub(super) fn apply_all<const FMA: bool>(&self, inputs: &[f32], bias: &[f32], out: &mut [f32]) {
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
            let out = &mut out[first * self.rows..(first + batch) * self.rows];
            self.batch::<FMA, false>(&padded, batch, bias, out);
        }
    }

    /// `out[b] = self · vectors[b] + bias` for `count` vectors by columns, or with `ADD`
    /// `out[b] += self · vectors[b]`: the sums of a vector are those of `apply_add` to the last bit
    #[inline(always)]
    pub(super) fn batch<const FMA: bool, const ADD: bool>(
        &self,
        columns: &[[f32; BATCH]],
        count: usize,
        bias: &[f32],
        out: &mut [f32],
    ) {
        for (tile, weights) in self.columns.chunks_exact(self.cols).enumerate() {
            // Four sums apart, not an array: so they stay in registers in any build profile
            let (mut s0, mut s1, mut s2, mut s3) =
                ([0.0f32; TILE], [0.0; TILE], [0.0; TILE], [0.0; TILE]);
            for (column, values) in weights.iter().zip(columns) {
                axpy::<FMA>(&mut s0, column, values[0]);
                axpy::<FMA>(&mut s1, column, values[1]);
                axpy::<FMA>(&mut s2, column, values[2]);
                axpy::<FMA>(&mut s3, column, values[3]);
            }
            let sums = [s0, s1, s2, s3];
            for (b, sums) in sums.iter().enumerate().take(count) {
                for (j, sum) in sums.iter().enumerate() {
                    let row = tile * TILE + j;
                    if row < self.rows {
                        let out = &mut out[b * self.rows + row];
                        *out = if ADD { *out + sum } else { sum + bias[row] };
                    }
                }
            }
        }
    }
}

/// `sums += value · column`, by a fused multiply-add if `FMA`: a quarter less time, and the last
/// bits differ from a processor without it
#[inline(always)]
fn axpy<const FMA: bool>(sums: &mut [f32; TILE], column: &[f32; TILE], value: f32) {
    for (sum, &weight) in sums.iter_mut().zip(column) {
        *sum = if FMA {
            value.mul_add(weight, *sum)
        } else {
            *sum + value * weight
        };
    }
}

/// Dot product by blocks of 8: so the loop is vectorized
#[inline(always)]
pub(super) fn dot(a: &[f32], b: &[f32]) -> f32 {
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

#[inline(always)]
pub(super) fn add(out: &mut [f32], values: &[f32]) {
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
pub(super) struct Lstm {
    pub(super) input: Tiled,
    pub(super) hidden: Tiled,
    pub(super) bias: Vec<f32>,
}

/// Value of an entry of the file of weights
pub(super) enum Value<'a> {
    Strings(Vec<&'a str>),
    Matrix(Matrix),
    Numbers(Vec<u32>),
}

/// Reading of the file of weights with checked bounds
struct Reader<'a> {
    data: &'a [u8],
    at: usize,
    what: &'static str,
}

impl<'a> Reader<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8], Error> {
        let end = (self.at.checked_add(count)).filter(|&end| end <= self.data.len());
        let end = end.ok_or_else(|| invalid(self.what, "the file of weights is truncated"))?;
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
            return Err(invalid(self.what, "an entry is larger than allowed"));
        }
        Ok(count)
    }

    fn f32s(&mut self, count: usize) -> Result<Vec<f32>, Error> {
        let size = count
            .checked_mul(4)
            .ok_or_else(|| invalid(self.what, "size"))?;
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
                        .map_err(|_| invalid(self.what, "a string is not UTF-8"))?;
                    strings.push(text);
                }
                Ok(Value::Strings(strings))
            }
            1 | 2 => {
                let (rows, cols) = (self.count(MAX_ROWS)?, self.count(MAX_DIM)?);
                let size = rows
                    .checked_mul(cols)
                    .ok_or_else(|| invalid(self.what, "size"))?;
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
                // NaN fails the comparison too
                if !data.iter().all(|weight| weight.abs() <= MAX_WEIGHT) {
                    return Err(invalid(self.what, "a weight is out of range"));
                }
                Ok(Value::Matrix(Matrix { rows, cols, data }))
            }
            3 => {
                let count = self.count(64)?;
                (0..count)
                    .map(|_| self.u32())
                    .collect::<Result<_, _>>()
                    .map(Value::Numbers)
            }
            _ => Err(invalid(self.what, &format!("unknown kind of entry {kind}"))),
        }
    }
}

/// An error of the file of weights of the model `what`
pub(super) fn invalid(what: &str, message: &str) -> Error {
    Error::Format(format!("{what}: {message}"))
}

/// Entries of the file of weights by name
pub(super) struct Entries<'a> {
    map: HashMap<&'a str, Value<'a>>,
    what: &'static str,
}

impl<'a> Entries<'a> {
    pub(super) fn take(&mut self, name: &str) -> Result<Value<'a>, Error> {
        (self.map.remove(name)).ok_or_else(|| invalid(self.what, &format!("no entry {name}")))
    }

    pub(super) fn matrix(&mut self, name: &str, rows: usize, cols: usize) -> Result<Matrix, Error> {
        match self.take(name)? {
            Value::Matrix(matrix) if matrix.rows == rows && matrix.cols == cols => Ok(matrix),
            Value::Matrix(matrix) => Err(invalid(
                self.what,
                &format!(
                    "{name}: size {}×{}, expected {rows}×{cols}",
                    matrix.rows, matrix.cols
                ),
            )),
            _ => Err(invalid(self.what, &format!("{name} is not a matrix"))),
        }
    }

    pub(super) fn vector(&mut self, name: &str, size: usize) -> Result<Vec<f32>, Error> {
        Ok(self.matrix(name, 1, size)?.data)
    }

    pub(super) fn strings(&mut self, name: &str) -> Result<Vec<&'a str>, Error> {
        match self.take(name)? {
            Value::Strings(strings) => Ok(strings),
            _ => Err(invalid(self.what, &format!("{name} are not strings"))),
        }
    }

    /// Numbers of the rows of a table by the strings of the entry `name` from the number `first`;
    /// a repeated string is an error: the number of a row would leave the table
    pub(super) fn index(&mut self, name: &str, first: u32) -> Result<HashMap<String, u32>, Error> {
        let strings = self.strings(name)?;
        let index: HashMap<String, u32> = (strings.iter().enumerate())
            .map(|(number, text)| ((*text).to_owned(), number as u32 + first))
            .collect();
        if index.len() != strings.len() {
            return Err(invalid(self.what, &format!("{name} repeat a string")));
        }
        Ok(index)
    }
}

/// Entries of a file of weights of the model `what`: the signature `magic`, the version `format`,
/// the number of entries and the entries
pub(super) fn read<'a>(
    data: &'a [u8],
    magic: &[u8; 8],
    format: u32,
    what: &'static str,
) -> Result<Entries<'a>, Error> {
    if data.get(..magic.len()) != Some(magic.as_slice()) {
        return Err(invalid(what, "no signature of a file of weights"));
    }
    let mut reader = Reader {
        data,
        at: magic.len(),
        what,
    };
    let version = reader.u32()?;
    if version != format {
        return Err(invalid(
            what,
            &format!("version of the file of weights {version}, supported {format}"),
        ));
    }
    let count = reader.count(256)?;
    let mut entries = HashMap::new();
    for _ in 0..count {
        let len = usize::from(u16::from_le_bytes([reader.take(1)?[0], reader.take(1)?[0]]));
        let name = std::str::from_utf8(reader.take(len)?)
            .map_err(|_| invalid(what, "the name of an entry is not UTF-8"))?;
        entries.insert(name, reader.value()?);
    }
    if reader.at != data.len() {
        return Err(invalid(what, "data after the entries"));
    }
    Ok(Entries { map: entries, what })
}

/// A layer of a BiLSTM over the inputs of at most `BATCH` sentences, `n × d` each, the longest
/// first: the output of a token is the states of the forward and the backward direction in a row.
/// Both directions go in one loop: their chains are independent, and the processor runs them at
/// once. On every step the recurrent matrix of a direction multiplies the states of all the
/// sentences, read once for them; the states of a sentence are those of it alone to the last bit
#[inline(always)]
pub(super) fn bilstm<const FMA: bool>(
    layer: &[Lstm; 2],
    inputs: &[Vec<f32>],
    d: usize,
) -> Vec<Vec<f32>> {
    let hidden = d / 2;
    let lens: Vec<usize> = inputs.iter().map(|input| input.len() / d).collect();
    debug_assert!(inputs.len() <= BATCH && lens.is_sorted_by(|a, b| a >= b));
    let mut outputs: Vec<Vec<f32>> = lens.iter().map(|&n| vec![0.0f32; n * d]).collect();
    // The input part of all gates for all tokens at once
    let mut gates_input = Vec::with_capacity(inputs.len());
    for (input, &n) in inputs.iter().zip(&lens) {
        let mut gates = [(); 2].map(|()| vec![0.0f32; n * 4 * hidden]);
        for (cell, gates) in layer.iter().zip(&mut gates) {
            cell.input.apply_all::<FMA>(input, &cell.bias, gates);
        }
        gates_input.push(gates);
    }
    // The states and the gates of the sentences in a row
    let count = inputs.len();
    let mut states = [(); 2].map(|()| (vec![0.0f32; count * hidden], vec![0.0f32; count * hidden]));
    let mut gates = [(); 2].map(|()| vec![0.0f32; count * 4 * hidden]);
    let mut columns = vec![[0.0f32; BATCH]; hidden];
    for step in 0..lens.first().copied().unwrap_or(0) {
        // The sentences that have this step: the longer ones, the first
        let active = lens.iter().take_while(|&&n| n > step).count();
        for direction in 0..2 {
            let (h, c) = &mut states[direction];
            let gates = &mut gates[direction][..active * 4 * hidden];
            for (s, gates) in gates.chunks_exact_mut(4 * hidden).enumerate() {
                let t = if direction == 0 {
                    step
                } else {
                    lens[s] - 1 - step
                };
                gates.copy_from_slice(
                    &gates_input[s][direction][t * 4 * hidden..(t + 1) * 4 * hidden],
                );
            }
            let cell = &layer[direction].hidden;
            if active == 1 {
                cell.apply_add::<FMA>(&h[..hidden], gates);
            } else {
                // The states by columns: a column holds a number of the state of every sentence
                for (col, values) in columns.iter_mut().enumerate() {
                    for (b, value) in values.iter_mut().enumerate().take(active) {
                        *value = h[b * hidden + col];
                    }
                }
                cell.batch::<FMA, true>(&columns, active, &[], gates);
            }
            for (s, gates) in gates.chunks_exact_mut(4 * hidden).enumerate() {
                let t = if direction == 0 {
                    step
                } else {
                    lens[s] - 1 - step
                };
                let (h, c) = (
                    &mut h[s * hidden..(s + 1) * hidden],
                    &mut c[s * hidden..(s + 1) * hidden],
                );
                cell_step(gates, h, c);
                outputs[s][t * d + direction * hidden..t * d + (direction + 1) * hidden]
                    .copy_from_slice(h);
            }
        }
    }
    outputs
}

/// A step of an LSTM cell: the new states `h` and `c` by the sums of its gates i, f, g, o; every
/// loop goes over an array without branches
#[inline(always)]
fn cell_step(gates: &mut [f32], h: &mut [f32], c: &mut [f32]) {
    let hidden = h.len();
    let (sigmoids, rest) = gates.split_at_mut(2 * hidden);
    let (candidates, outputs) = rest.split_at_mut(hidden);
    for value in sigmoids.iter_mut() {
        *value = sigmoid(*value);
    }
    for value in candidates.iter_mut() {
        *value = tanh(*value);
    }
    for value in outputs.iter_mut() {
        *value = sigmoid(*value);
    }
    let (inputs, forgets) = sigmoids.split_at(hidden);
    for i in 0..hidden {
        c[i] = forgets[i] * c[i] + inputs[i] * candidates[i];
    }
    for i in 0..hidden {
        h[i] = outputs[i] * tanh(c[i]);
    }
}

/// A layer of an LSTM of one direction over the input parts of the gates of its inputs, the
/// input matrix times the input plus the biases: the state after the last of them
#[inline(always)]
pub(super) fn lstm<const FMA: bool>(cell: &Lstm, gates_input: &mut [f32]) -> Vec<f32> {
    let hidden = cell.hidden.cols;
    let (mut h, mut c) = (vec![0.0f32; hidden], vec![0.0f32; hidden]);
    for gates in gates_input.chunks_exact_mut(4 * hidden) {
        cell.hidden.apply_add::<FMA>(&h, gates);
        cell_step(gates, &mut h, &mut c);
    }
    h
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn entry(out: &mut Vec<u8>, name: &str, kind: u8, body: &[u8]) {
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(name.as_bytes());
        out.push(kind);
        out.extend_from_slice(body);
    }

    pub(crate) fn matrix(rows: usize, cols: usize, values: &[f32]) -> Vec<u8> {
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

    pub(crate) fn strings(items: &[&str]) -> Vec<u8> {
        let mut body = (items.len() as u32).to_le_bytes().to_vec();
        for item in items {
            body.extend_from_slice(&(item.len() as u32).to_le_bytes());
            body.extend_from_slice(item.as_bytes());
        }
        body
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
        let output = bilstm::<FUSED>(&[cell(), cell()], &[vec![0.0; 6]], 2).remove(0);
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
        tiled_products_with::<false>();
        tiled_products_with::<true>();
    }

    fn tiled_products_with<const FMA: bool>() {
        // 18 rows: two tiles, the second one partly empty; 5 vectors: two batches. The values are
        // exact, so a fused product gives them too
        let (rows, cols) = (18, 3);
        let data: Vec<f32> = (0..rows * cols).map(|i| (i % 7) as f32 - 3.0).collect();
        let matrix = Matrix { rows, cols, data };
        let tiled = Tiled::new(&matrix);
        let inputs: Vec<f32> = (0..5 * cols).map(|i| i as f32 * 0.5).collect();
        let bias: Vec<f32> = (0..rows).map(|i| i as f32).collect();
        let mut out = vec![0.0; 5 * rows];
        tiled.apply_all::<FMA>(&inputs, &bias, &mut out);
        for t in 0..5 {
            let mut expected = vec![0.0; rows];
            matrix.apply(&inputs[t * cols..(t + 1) * cols], &mut expected);
            add(&mut expected, &bias);
            assert_eq!(out[t * rows..(t + 1) * rows], expected[..]);
            let mut added = bias.clone();
            tiled.apply_add::<FMA>(&inputs[t * cols..(t + 1) * cols], &mut added);
            assert_eq!(added, expected);
        }
        let columns: Vec<[f32; BATCH]> = (0..cols)
            .map(|col| [0, 1, 2, 3].map(|t| inputs[t * cols + col]))
            .collect();
        let mut added: Vec<f32> = (0..BATCH).flat_map(|_| bias.iter().copied()).collect();
        tiled.batch::<FMA, true>(&columns, BATCH, &[], &mut added);
        assert_eq!(added, out[..BATCH * rows]);
    }

    #[test]
    fn sentences_together_as_alone() {
        sentences_together_as_alone_with::<false>();
        sentences_together_as_alone_with::<true>();
    }

    fn sentences_together_as_alone_with<const FMA: bool>() {
        // A hidden state of 10: 40 rows of gates, three tiles. Four sentences, the longest first
        let (hidden, d) = (10, 20);
        let mut seed = 1u32;
        let mut random = |count: usize| -> Vec<f32> {
            (0..count)
                .map(|_| {
                    seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    (seed >> 8) as f32 / (1 << 24) as f32 - 0.5
                })
                .collect()
        };
        let mut cell = || Lstm {
            input: Tiled::new(&Matrix {
                rows: 4 * hidden,
                cols: d,
                data: random(4 * hidden * d),
            }),
            hidden: Tiled::new(&Matrix {
                rows: 4 * hidden,
                cols: hidden,
                data: random(4 * hidden * hidden),
            }),
            bias: random(4 * hidden),
        };
        let layer = [cell(), cell()];
        let inputs: Vec<Vec<f32>> = [7, 5, 2, 1].map(|n| random(n * d)).into();
        let together = bilstm::<FMA>(&layer, &inputs, d);
        for (input, output) in inputs.iter().zip(together) {
            let alone = bilstm::<FMA>(&layer, std::slice::from_ref(input), d);
            assert_eq!(alone[0], output);
        }
    }

    #[test]
    fn lstm_as_the_forward_direction() {
        // The state of one direction after the last input is the forward half of the output of
        // the BiLSTM at the last token, to the last bit
        let (hidden, d, n) = (10, 20, 6);
        let mut seed = 7u32;
        let mut random = |count: usize| -> Vec<f32> {
            (0..count)
                .map(|_| {
                    seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    (seed >> 8) as f32 / (1 << 24) as f32 - 0.5
                })
                .collect()
        };
        let mut cell = || Lstm {
            input: Tiled::new(&Matrix {
                rows: 4 * hidden,
                cols: d,
                data: random(4 * hidden * d),
            }),
            hidden: Tiled::new(&Matrix {
                rows: 4 * hidden,
                cols: hidden,
                data: random(4 * hidden * hidden),
            }),
            bias: random(4 * hidden),
        };
        let layer = [cell(), cell()];
        let input = random(n * d);
        let mut gates = vec![0.0; n * 4 * hidden];
        layer[0]
            .input
            .apply_all::<false>(&input, &layer[0].bias, &mut gates);
        let state = lstm::<false>(&layer[0], &mut gates);
        let output = bilstm::<false>(&layer, std::slice::from_ref(&input), 2 * hidden).remove(0);
        assert_eq!(
            state,
            output[(n - 1) * 2 * hidden..(n - 1) * 2 * hidden + hidden]
        );
    }

    #[test]
    fn dot_products() {
        let a: Vec<f32> = (0..19).map(|i| i as f32).collect();
        let b = vec![2.0f32; 19];
        assert_eq!(dot(&a, &b), 342.0);
    }

    #[test]
    fn weights_out_of_range() {
        // An int8 matrix: rows, columns, the scale of every row and its values
        let int8 = |scale: f32| {
            let mut data = vec![2];
            data.extend(1u32.to_le_bytes());
            data.extend(2u32.to_le_bytes());
            data.extend(scale.to_le_bytes());
            data.extend([100, 156]);
            Reader {
                data: &data,
                at: 0,
                what: "model",
            }
            .value()
            .map(|_| ())
        };
        assert!(int8(0.04).is_ok());
        for scale in [1e37, f32::NAN] {
            let err = int8(scale).unwrap_err();
            assert!(err.to_string().contains("out of range"), "{scale}: {err}");
        }
    }
}
