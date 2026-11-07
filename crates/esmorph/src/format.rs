//! Binary format of the dictionary: a header (signature, format version, CRC32 of the rest of the
//! file) and sections "length u64 + data + zeros to a boundary of 8 bytes", numbers little-endian.
//! Sections of numbers are aligned, so they are read right from the file mapped into memory

use std::io::{ErrorKind, Read};
use std::ops::Range;

use crate::Error;

const MAGIC: &[u8; 8] = b"ESMORPH\0";
/// The most spellings of one key: the dictionary has no more than four
pub(crate) const MAX_SPELLINGS: usize = 8;
/// The longest list of analyses: the dictionary has lists of a dozen, while a damaged length would
/// make the analysis of a word quadratic
pub(crate) const MAX_LIST: usize = 255;
/// The bit of the length of a list of analyses: the list has probabilities
pub(crate) const HAS_PROBABILITIES: u32 = 1 << 8;
/// A probability is kept in millionths
pub(crate) const PROBABILITY_SCALE: u32 = 1_000_000;
const VERSION: u32 = 2;
const HEADER: usize = 16;
const ALIGN: usize = 8;
const SECTIONS: usize = 11;

/// Contents of a dictionary for writing into a file
#[cfg(any(test, feature = "build"))]
#[derive(Clone, Default)]
pub(crate) struct Sections {
    pub meta: String,
    /// Parts of speech, names and values of features and labels of UD tags
    pub ud_strings: Vec<String>,
    /// UD tags in a row as numbers of `ud_strings`: part of speech, number of features, pairs of
    /// "name, value", number of labels, labels
    pub ud_tags: Vec<u16>,
    /// Letters of word forms: the byte of a letter in the keys of the automaton is its number from one
    pub alphabet: String,
    /// Suffixes of forms; strings of the table do not repeat
    pub suffixes: Vec<String>,
    /// Start of every paradigm in `paradigms` and the common end
    pub paradigm_offsets: Vec<u32>,
    /// Paradigms in a row: the numbers of the suffixes of their forms
    pub paradigms: Vec<u32>,
    /// Inflection tables in a row: the number of the paradigm, then `(lemma form << 16) | UD tag`
    /// of each of its forms
    pub tables: Vec<u32>,
    /// Automaton "word form with plain vowels - `WordValue`"
    pub words: Vec<u8>,
    /// Lists of analyses in the order of their probability: the length with `HAS_PROBABILITIES` if
    /// the list has them, `(table << 16) | form` of every analysis, then their probabilities
    pub records: Vec<u32>,
    /// Spellings of keys that do not fit the value of the automaton: the number of spellings, then
    /// the accent places (two numbers), the start of the list of analyses and the share among the
    /// spellings, for a query that lost its accents, of each
    pub variants: Vec<u32>,
}

/// Value of a key in the automaton of word forms
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WordValue {
    /// One spelling: the places of its accents and the start of its list of analyses
    Single { marks: u64, records: u32 },
    /// The spellings are in the section `variants` from this place
    Many(u32),
}

/// One spelling fits the value if the list of analyses starts below 2²⁴ and the accents stand
/// among the first 39 letters
const RECORDS_BITS: u32 = 24;
#[cfg(any(test, feature = "build"))]
const MARKS_BITS: u32 = 39;
const MANY: u64 = 1 << 63;

impl WordValue {
    /// Value of the automaton; `None` if the spelling does not fit it
    #[cfg(any(test, feature = "build"))]
    pub fn pack(self) -> Option<u64> {
        let fits = |value: u64, bits: u32| (value >> bits == 0).then_some(value);
        match self {
            WordValue::Single { marks, records } => Some(
                fits(u64::from(records), RECORDS_BITS)? | fits(marks, MARKS_BITS)? << RECORDS_BITS,
            ),
            WordValue::Many(offset) => Some(MANY | u64::from(offset)),
        }
    }

    pub fn unpack(value: u64) -> Self {
        if value & MANY != 0 {
            return WordValue::Many(value as u32);
        }
        WordValue::Single {
            marks: value >> RECORDS_BITS,
            records: (value & ((1 << RECORDS_BITS) - 1)) as u32,
        }
    }
}

/// Bounds of the sections in the bytes of the file
#[derive(Clone, Debug, Default)]
pub(crate) struct Layout {
    pub meta: Range<usize>,
    pub ud_strings: Range<usize>,
    pub ud_tags: Range<usize>,
    pub alphabet: Range<usize>,
    pub suffixes: Range<usize>,
    pub paradigm_offsets: Range<usize>,
    pub paradigms: Range<usize>,
    pub tables: Range<usize>,
    pub words: Range<usize>,
    pub records: Range<usize>,
    pub variants: Range<usize>,
}

impl Layout {
    /// Sections of numbers read right from the file and the size of their numbers in bytes
    pub fn numbers(&self) -> [(&Range<usize>, usize); 5] {
        [
            (&self.paradigm_offsets, 4),
            (&self.paradigms, 4),
            (&self.tables, 4),
            (&self.records, 4),
            (&self.variants, 4),
        ]
    }
}

#[cfg(any(test, feature = "build"))]
pub(crate) fn write(sections: &Sections) -> Vec<u8> {
    let mut out = MAGIC.to_vec();
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    let mut section = |bytes: &[u8]| {
        out.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
        out.extend_from_slice(bytes);
        out.resize(out.len().next_multiple_of(ALIGN), 0);
    };
    section(sections.meta.as_bytes());
    section(&strings_bytes(&sections.ud_strings));
    section(&u16_bytes(&sections.ud_tags));
    section(sections.alphabet.as_bytes());
    section(&strings_bytes(&sections.suffixes));
    section(&u32_bytes(&sections.paradigm_offsets));
    section(&u32_bytes(&sections.paradigms));
    section(&u32_bytes(&sections.tables));
    section(&sections.words);
    section(&u32_bytes(&sections.records));
    section(&u32_bytes(&sections.variants));
    let checksum = crc32fast::hash(&out[HEADER..]);
    out[12..HEADER].copy_from_slice(&checksum.to_le_bytes());
    out
}

/// Checks the signature and the format version at the start of the file
fn check_header(data: &[u8]) -> Result<(), Error> {
    if data.get(..MAGIC.len()) != Some(MAGIC.as_slice()) {
        return Err(Error::Format(
            "no signature of an esMorph dictionary".into(),
        ));
    }
    let Some(header) = data.get(..HEADER) else {
        return Err(truncated());
    };
    let version = u32::from_le_bytes([header[8], header[9], header[10], header[11]]);
    if version != VERSION {
        return Err(Error::Format(format!(
            "format version {version}, supported {VERSION}"
        )));
    }
    Ok(())
}

/// Bounds of the sections of a file: the signature, the version, the lengths of the sections and
/// the checksum are checked
pub(crate) fn layout(data: &[u8]) -> Result<Layout, Error> {
    check_header(data)?;
    let mut at = HEADER;
    let mut section = |width: usize| -> Result<Range<usize>, Error> {
        let len = data.get(at..at + 8).ok_or_else(truncated)?;
        let len = u64::from_le_bytes(len.try_into().map_err(|_| truncated())?);
        let start = at + 8;
        let end = usize::try_from(len)
            .ok()
            .and_then(|len| start.checked_add(len))
            .filter(|&end| end <= data.len())
            .ok_or_else(truncated)?;
        if !(end - start).is_multiple_of(width) {
            return Err(Error::Format(format!(
                "section length is not a multiple of {width}"
            )));
        }
        at = end.next_multiple_of(ALIGN);
        if at > data.len() {
            return Err(truncated());
        }
        Ok(start..end)
    };
    let layout = Layout {
        meta: section(1)?,
        ud_strings: section(1)?,
        ud_tags: section(2)?,
        alphabet: section(1)?,
        suffixes: section(1)?,
        paradigm_offsets: section(4)?,
        paradigms: section(4)?,
        tables: section(4)?,
        words: section(1)?,
        records: section(4)?,
        variants: section(4)?,
    };
    if at != data.len() {
        return Err(extra_data());
    }
    // The checksum reads the whole file, so it comes after the quick checks
    let checksum = u32::from_le_bytes(data[12..HEADER].try_into().map_err(|_| truncated())?);
    if crc32fast::hash(&data[HEADER..]) != checksum {
        return Err(Error::Format(
            "checksum mismatch: the file is damaged".into(),
        ));
    }
    Ok(layout)
}

/// Reads a dictionary from a stream: the signature and the version are checked before the rest is
/// read, and sections are read as they come, so an endless stream is not read whole
pub(crate) fn read(mut input: impl Read) -> Result<Storage, Error> {
    let mut buffer = Buffer::default();
    buffer.fill(&mut input, HEADER as u64)?;
    check_header(buffer.bytes())?;
    for _ in 0..SECTIONS {
        if buffer.fill(&mut input, 8)? < 8 {
            return Err(truncated());
        }
        let bytes = buffer.bytes();
        let len = u64::from_le_bytes(
            bytes[bytes.len() - 8..]
                .try_into()
                .map_err(|_| truncated())?,
        );
        let padded = len
            .checked_next_multiple_of(ALIGN as u64)
            .ok_or_else(truncated)?;
        if buffer.fill(&mut input, padded)? < padded {
            return Err(truncated());
        }
    }
    if buffer.fill(&mut input, 1)? > 0 {
        return Err(extra_data());
    }
    buffer.words.truncate(buffer.len.div_ceil(ALIGN));
    Ok(Storage::Owned(buffer.words, buffer.len))
}

/// Bytes of a stream aligned to 8: the dictionary is built from them without a second copy
#[derive(Default)]
struct Buffer {
    words: Vec<u64>,
    len: usize,
}

impl Buffer {
    fn bytes(&self) -> &[u8] {
        &bytemuck::cast_slice(&self.words)[..self.len]
    }

    /// Reads `count` more bytes; fewer only at the end of the stream
    fn fill(&mut self, input: &mut impl Read, count: u64) -> Result<u64, Error> {
        const CHUNK: u64 = 1 << 16;
        let mut done = 0;
        while done < count {
            let want = (count - done).min(CHUNK) as usize;
            self.words.resize((self.len + want).div_ceil(ALIGN), 0);
            let space = &mut bytemuck::cast_slice_mut::<u64, u8>(&mut self.words)
                [self.len..self.len + want];
            match input.read(space) {
                Ok(0) => break,
                Ok(read) => {
                    self.len += read;
                    done += read as u64;
                }
                Err(err) if err.kind() == ErrorKind::Interrupted => {}
                Err(err) => return Err(err.into()),
            }
        }
        Ok(done)
    }
}

fn truncated() -> Error {
    Error::Format("the file is truncated".into())
}

fn extra_data() -> Error {
    Error::Format("extra data after the last section".into())
}

/// Bytes of a dictionary: a file mapped into memory, data of the program or a copy aligned to 8
pub(crate) enum Storage {
    Owned(Vec<u64>, usize),
    Mapped(memmap2::Mmap),
    Static(&'static [u8]),
}

impl Storage {
    pub fn copy(data: &[u8]) -> Self {
        let mut words = vec![0u64; data.len().div_ceil(ALIGN)];
        bytemuck::cast_slice_mut::<u64, u8>(&mut words)[..data.len()].copy_from_slice(data);
        Storage::Owned(words, data.len())
    }

    /// Data of the program is read without a copy if it is aligned to 8 bytes
    pub fn of_static(data: &'static [u8]) -> Self {
        if (data.as_ptr() as usize).is_multiple_of(ALIGN) {
            Storage::Static(data)
        } else {
            Storage::copy(data)
        }
    }

    pub fn bytes(&self) -> &[u8] {
        match self {
            Storage::Owned(words, len) => &bytemuck::cast_slice(words)[..*len],
            Storage::Mapped(map) => map,
            Storage::Static(data) => data,
        }
    }

    /// Dictionary with the numbers of the sections of `layout` in the byte order of the platform:
    /// a copy on big-endian
    pub fn native(self, layout: &Layout) -> Self {
        if cfg!(target_endian = "little") {
            self
        } else {
            self.swapped(layout)
        }
    }

    /// Copy with the reverse byte order in the numbers of the sections of `layout`
    fn swapped(&self, layout: &Layout) -> Self {
        let mut copy = Storage::copy(self.bytes());
        if let Storage::Owned(words, _) = &mut copy {
            let bytes = bytemuck::cast_slice_mut::<u64, u8>(words);
            for (range, width) in layout.numbers() {
                for number in bytes[range.clone()].chunks_exact_mut(width) {
                    number.reverse();
                }
            }
        }
        copy
    }
}

/// Bytes of a section of a dictionary for an automaton: the automaton holds the dictionary while it
/// is needed
#[derive(Clone)]
pub(crate) struct Slice {
    pub storage: std::sync::Arc<Storage>,
    pub range: Range<usize>,
}

impl AsRef<[u8]> for Slice {
    fn as_ref(&self) -> &[u8] {
        &self.storage.bytes()[self.range.clone()]
    }
}

/// Numbers of a section aligned on loading; a section that is not aligned is empty
pub(crate) fn numbers<T: bytemuck::Pod>(bytes: &[u8]) -> &[T] {
    bytemuck::try_cast_slice(bytes).unwrap_or_default()
}

/// Table of strings: the number of strings, the ends of the strings, then the strings in a row
#[derive(Clone, Debug, Default)]
pub(crate) struct Strings {
    text: String,
    ends: Vec<u32>,
}

impl Strings {
    pub fn read(bytes: &[u8]) -> Result<Self, Error> {
        let invalid = |msg: &str| Error::Format(msg.into());
        let number = |at: usize| -> Option<u32> {
            Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
        };
        let count = number(0).ok_or_else(|| invalid("the table of strings is truncated"))? as usize;
        // The end of each string takes 4 bytes: otherwise the count in a file could ask for any allocation
        let start = count
            .checked_mul(4)
            .and_then(|len| len.checked_add(4))
            .filter(|&start| start <= bytes.len())
            .ok_or_else(|| invalid("more strings than the size of the table"))?;
        let text = std::str::from_utf8(&bytes[start..])
            .map_err(|_| invalid("a string is not in UTF-8"))?
            .to_owned();
        let ends: Vec<u32> = (0..count)
            .map(|i| number(4 + 4 * i).unwrap_or(u32::MAX))
            .collect();
        let mut previous = 0;
        for &end in &ends {
            let end = end as usize;
            if end < previous || !text.is_char_boundary(end) {
                return Err(invalid("wrong bounds of strings in the table"));
            }
            previous = end;
        }
        if previous != text.len() {
            return Err(invalid("extra data in the table of strings"));
        }
        Ok(Strings { text, ends })
    }

    pub fn len(&self) -> usize {
        self.ends.len()
    }

    pub fn get(&self, index: usize) -> Option<&str> {
        let end = *self.ends.get(index)? as usize;
        let start = index.checked_sub(1).map_or(0, |i| self.ends[i] as usize);
        self.text.get(start..end)
    }

    /// A string that is surely in the table: the number is checked on loading
    pub fn at(&self, index: usize) -> &str {
        self.get(index).unwrap_or_default()
    }

    pub fn iter(&self) -> impl ExactSizeIterator<Item = &str> {
        (0..self.len()).map(|index| self.at(index))
    }
}

#[cfg(any(test, feature = "build"))]
fn strings_bytes(values: &[String]) -> Vec<u8> {
    let mut out = (values.len() as u32).to_le_bytes().to_vec();
    let mut end = 0;
    for value in values {
        end += value.len() as u32;
        out.extend_from_slice(&end.to_le_bytes());
    }
    for value in values {
        out.extend_from_slice(value.as_bytes());
    }
    out
}

#[cfg(any(test, feature = "build"))]
fn u16_bytes(values: &[u16]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

#[cfg(any(test, feature = "build"))]
fn u32_bytes(values: &[u32]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A file with the checksum written anew: damage the checksum does not catch
    pub(crate) fn with_checksum(mut data: Vec<u8>) -> Vec<u8> {
        if data.len() >= HEADER {
            let checksum = crc32fast::hash(&data[HEADER..]);
            data[12..HEADER].copy_from_slice(&checksum.to_le_bytes());
        }
        data
    }

    fn sample() -> Sections {
        Sections {
            meta: "{}".into(),
            ud_strings: vec!["NOUN".into(), "Gender".into(), "Fem".into()],
            ud_tags: vec![0, 1, 1, 2, 0],
            alphabet: "ab".into(),
            suffixes: vec![String::new(), "a".into()],
            paradigm_offsets: vec![0, 1],
            paradigms: vec![0],
            tables: vec![0, 0],
            words: vec![1, 2, 3],
            records: vec![1, 0],
            ..Sections::default()
        }
    }

    #[test]
    fn word_values() {
        let values = [
            WordValue::Single {
                marks: 0,
                records: 0,
            },
            WordValue::Single {
                marks: (1 << MARKS_BITS) - 1,
                records: (1 << RECORDS_BITS) - 1,
            },
            WordValue::Many(0),
            WordValue::Many(u32::MAX),
        ];
        for value in values {
            assert_eq!(WordValue::unpack(value.pack().unwrap()), value);
        }
        for value in [
            WordValue::Single {
                marks: 1 << MARKS_BITS,
                records: 0,
            },
            WordValue::Single {
                marks: 0,
                records: 1 << RECORDS_BITS,
            },
        ] {
            assert_eq!(value.pack(), None);
        }
    }

    #[test]
    fn sections_are_aligned() {
        let data = write(&sample());
        let layout = layout(&data).unwrap();
        for (range, _) in layout.numbers() {
            assert_eq!(range.start % ALIGN, 0);
        }
        assert_eq!(data.len() % ALIGN, 0);
        let storage = Storage::copy(&data);
        assert_eq!(storage.bytes(), data);
        assert_eq!(storage.bytes().as_ptr() as usize % ALIGN, 0);
        let storage = storage.native(&layout);
        assert_eq!(numbers::<u32>(&storage.bytes()[layout.records]), [1, 0]);
    }

    #[test]
    fn swaps_numbers_for_big_endian() {
        let mut sections = sample();
        sections.paradigms = vec![0x0102];
        let data = write(&sections);
        let layout = layout(&data).unwrap();
        let storage = Storage::copy(&data);
        let swapped = storage.swapped(&layout);
        // Numbers of the file are little-endian: after the swap they read as big-endian
        assert_eq!(
            numbers::<u32>(&swapped.bytes()[layout.paradigms.clone()]),
            [0x0102u32].map(|number| u32::from_ne_bytes(number.to_be_bytes()))
        );
        // Strings and automata do not change
        assert_eq!(
            swapped.bytes()[layout.words.clone()],
            storage.bytes()[layout.words.clone()]
        );
        let native = storage.native(&layout);
        assert_eq!(
            numbers::<u32>(&native.bytes()[layout.paradigms.clone()]),
            [0x0102]
        );
    }

    #[test]
    fn rejects_damaged_files() {
        let data = write(&sample());
        // The checksum or the check of the section lengths catches any damaged byte after the header
        for at in HEADER..data.len() {
            let mut damaged = data.clone();
            damaged[at] ^= 0x10;
            assert!(layout(&damaged).is_err(), "{at}");
        }
        let mut damaged = data.clone();
        damaged[HEADER + 8] ^= 0x10;
        let err = layout(&damaged).err().unwrap();
        assert!(err.to_string().contains("checksum"), "{err}");
        for broken in [
            &b"ESMORPH"[..],
            &data[..8],
            &data[..HEADER],
            &data[..data.len() - 1],
        ] {
            assert!(layout(broken).is_err());
        }
        let mut other = data.clone();
        other[8] = 9;
        assert!(
            layout(&other)
                .err()
                .unwrap()
                .to_string()
                .contains("version")
        );
        let mut foreign = data.clone();
        foreign[0] = b'X';
        assert!(
            layout(&foreign)
                .err()
                .unwrap()
                .to_string()
                .contains("signature")
        );
        let mut longer = data.clone();
        longer.extend_from_slice(&[0; 8]);
        assert!(layout(&with_checksum(longer)).is_err());
    }

    #[test]
    fn reads_streams() {
        let data = write(&sample());
        let storage = read(&data[..]).unwrap();
        assert_eq!(storage.bytes(), data);
        assert!(read(&data[..data.len() - 1]).is_err());
        let mut longer = data.clone();
        longer.push(0);
        assert!(read(&longer[..]).is_err());
        assert!(read(&b"not a dictionary, long enough"[..]).is_err());
        // An endless stream with a huge length of a section ends at its end, not after a huge allocation
        let mut header = data[..HEADER].to_vec();
        header.extend_from_slice(&u64::MAX.to_le_bytes());
        assert!(read(&header[..]).is_err());
    }

    #[test]
    fn tables_of_strings() {
        let table =
            Strings::read(&strings_bytes(&["a".into(), String::new(), "ñ".into()])).unwrap();
        assert_eq!(table.len(), 3);
        assert_eq!(table.iter().collect::<Vec<_>>(), ["a", "", "ñ"]);
        assert_eq!(table.get(2), Some("ñ"));
        assert_eq!(table.get(3), None);
        assert_eq!(table.at(3), "");
        for broken in [
            &[][..],
            &[9, 0, 0, 0],
            // The end of the only string splits a letter
            &[1, 0, 0, 0, 1, 0, 0, 0, 0xC3, 0xB1],
            // Text after the end of the last string
            &[1, 0, 0, 0, 1, 0, 0, 0, b'a', b'b'],
            &[1, 0, 0, 0, 2, 0, 0, 0, 0xFF, 0xFF],
        ] {
            assert!(Strings::read(broken).is_err(), "{broken:?}");
        }
    }
}
