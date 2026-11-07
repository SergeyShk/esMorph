//! One-byte spelling of words in the keys of the automaton: the byte of a letter is its number in
//! the alphabet of the dictionary. A key holds an accented vowel as the plain one, and the places
//! of the accents are kept apart as a mask. The diaeresis is no accent: `ü` is a letter of its own,
//! because `ambigúes` and `paragüas` leave no way to tell it from `ú` by the neighbours

#[cfg(any(test, feature = "build"))]
use std::cmp::Ordering;

use crate::Error;

/// The longest key in letters: the dictionary has no longer word forms
pub(crate) const MAX_KEY: usize = 64;

/// The plain vowel of an accented one
fn plain(letter: char) -> Option<char> {
    match letter {
        'á' => Some('a'),
        'é' => Some('e'),
        'í' => Some('i'),
        'ó' => Some('o'),
        'ú' => Some('u'),
        _ => None,
    }
}

/// The accented vowel of a plain one
fn accented(letter: char) -> Option<char> {
    match letter {
        'a' => Some('á'),
        'e' => Some('é'),
        'i' => Some('í'),
        'o' => Some('ó'),
        'u' => Some('ú'),
        _ => None,
    }
}

/// Letters of the dictionary and their bytes
#[derive(Clone)]
pub(crate) struct Alphabet {
    /// The letter of each byte; `'\0'` for a byte without a letter
    letters: [char; 256],
    ascii: [u8; 128],
    /// Bytes of the other letters in the order of the letters
    other: Vec<(char, u8)>,
}

impl Alphabet {
    /// Alphabet of letters in order: the byte of a letter is its number from one
    pub fn new(letters: &str) -> Result<Self, Error> {
        let mut alphabet = Alphabet {
            letters: ['\0'; 256],
            ascii: [0; 128],
            other: Vec::new(),
        };
        for (index, letter) in letters.chars().enumerate() {
            let code = u8::try_from(index + 1)
                .map_err(|_| Error::Format("more than 255 letters in the alphabet".into()))?;
            if letter == '\0' {
                return Err(Error::Format("U+0000 in the alphabet".into()));
            }
            if plain(letter).is_some() {
                return Err(Error::Format("an accented vowel in the alphabet".into()));
            }
            if letter.is_ascii() && alphabet.ascii[letter as usize] != 0 {
                return Err(Error::Format("a letter of the alphabet repeats".into()));
            }
            alphabet.letters[usize::from(code)] = letter;
            if letter.is_ascii() {
                alphabet.ascii[letter as usize] = code;
            } else {
                alphabet.other.push((letter, code));
            }
        }
        alphabet.other.sort_unstable();
        if alphabet.other.windows(2).any(|pair| pair[0].0 == pair[1].0) {
            return Err(Error::Format("a letter of the alphabet repeats".into()));
        }
        Ok(alphabet)
    }

    fn code(&self, letter: char) -> Option<u8> {
        let code = if letter.is_ascii() {
            self.ascii[letter as usize]
        } else {
            match self
                .other
                .binary_search_by(|&(known, _)| known.cmp(&letter))
            {
                Ok(at) => self.other[at].1,
                Err(_) => 0,
            }
        };
        (code != 0).then_some(code)
    }

    /// Key of a word of a text: letters in lower case, accents in the mask; `None` if the
    /// dictionary cannot have such a word form
    pub fn input_key(&self, word: &str) -> Option<Key> {
        let mut key = Key::new();
        for letter in word.chars() {
            if letter.is_ascii() {
                key.push(self.code(letter.to_ascii_lowercase())?)?;
            } else {
                for lower in letter.to_lowercase() {
                    self.push(&mut key, lower)?;
                }
            }
        }
        Some(key)
    }

    /// Key of a word form as the dictionary spells it: the case does not change
    #[cfg(any(test, feature = "build"))]
    pub fn spelling_key(&self, spelling: &str) -> Option<Key> {
        let mut key = Key::new();
        for letter in spelling.chars() {
            self.push(&mut key, letter)?;
        }
        Some(key)
    }

    fn push(&self, key: &mut Key, letter: char) -> Option<()> {
        match plain(letter) {
            Some(vowel) => {
                let at = key.len;
                key.push(self.code(vowel)?)?;
                key.marks |= 1 << at;
                Some(())
            }
            None => key.push(self.code(letter)?),
        }
    }

    /// Keys of `key` with a lost diaeresis restored: `gu` before `e` or `i` as `gü`, one place at
    /// a time and then all of them. Spanish writes `ü` only there
    pub fn diaeresis_keys(&self, key: &Key) -> Vec<Key> {
        let (Some(g), Some(u), Some(diaeresis)) = (self.code('g'), self.code('u'), self.code('ü'))
        else {
            return Vec::new();
        };
        let (e, i) = (self.code('e'), self.code('i'));
        let bytes = key.bytes();
        let places: Vec<usize> = (1..bytes.len().saturating_sub(1))
            .filter(|&at| {
                bytes[at] == u
                    && bytes[at - 1] == g
                    && (Some(bytes[at + 1]) == e || Some(bytes[at + 1]) == i)
            })
            .collect();
        let mut keys: Vec<Key> = places
            .iter()
            .map(|&at| key.with_byte(at, diaeresis))
            .collect();
        if places.len() > 1 {
            let mut all = key.clone();
            for &at in &places {
                all.bytes[at] = diaeresis;
            }
            keys.push(all);
        }
        keys
    }

    /// Spelling of a key with accents on the places of `marks`
    pub fn spelling(&self, key: &Key, marks: u64) -> String {
        self.decode(key.bytes(), marks)
    }

    /// Spelling of the bytes of a key with accents on the places of `marks`
    pub fn decode(&self, bytes: &[u8], marks: u64) -> String {
        let mut spelling = String::with_capacity(2 * bytes.len());
        for (at, &code) in bytes.iter().enumerate() {
            let letter = self.letters[usize::from(code)];
            let marked = at < 64 && marks >> at & 1 == 1;
            spelling.push(accented(letter).filter(|_| marked).unwrap_or(letter));
        }
        spelling
    }
}

/// Key of the automaton: bytes of the letters and places of the accents
#[derive(Clone)]
pub(crate) struct Key {
    bytes: [u8; MAX_KEY],
    len: usize,
    /// Places of the accented vowels: the bit of the number of the letter
    pub marks: u64,
}

impl Key {
    fn new() -> Self {
        Key {
            bytes: [0; MAX_KEY],
            len: 0,
            marks: 0,
        }
    }

    fn push(&mut self, byte: u8) -> Option<()> {
        *self.bytes.get_mut(self.len)? = byte;
        self.len += 1;
        Some(())
    }

    fn with_byte(&self, at: usize, byte: u8) -> Self {
        let mut key = self.clone();
        key.bytes[at] = byte;
        key
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}

/// Order of the spellings of one key by the places of the accents from left to right: first
/// without accents, then with an accent on the leftmost place and so on
#[cfg(any(test, feature = "build"))]
pub(crate) fn mark_order(a: u64, b: u64) -> Ordering {
    let (mut a, mut b) = (a, b);
    loop {
        match (a, b) {
            (0, 0) => return Ordering::Equal,
            (0, _) => return Ordering::Less,
            (_, 0) => return Ordering::Greater,
            _ => {}
        }
        let (left, right) = (a.trailing_zeros(), b.trailing_zeros());
        if left != right {
            return left.cmp(&right);
        }
        a &= a - 1;
        b &= b - 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alphabet() -> Alphabet {
        Alphabet::new("-.aceginoqrstuñü").unwrap()
    }

    #[test]
    fn keys_of_words() {
        let alphabet = alphabet();
        assert!(
            alphabet.input_key("CAMIÓN").is_none(),
            "m is not in the alphabet"
        );
        let key = alphabet.input_key("Canción").unwrap();
        assert_eq!(key.marks, 1 << 5);
        assert_eq!(alphabet.spelling(&key, key.marks), "canción");
        assert_eq!(alphabet.spelling(&key, 0), "cancion");
        assert_eq!(alphabet.input_key("CANCION").unwrap().bytes(), key.bytes());
        assert_eq!(alphabet.input_key("CANCIÓN").unwrap().marks, key.marks);
        assert!(alphabet.input_key("canción!").is_none());
        assert!(alphabet.input_key(&"a".repeat(MAX_KEY + 1)).is_none());
        assert_eq!(
            alphabet.input_key(&"a".repeat(MAX_KEY)).unwrap().len,
            MAX_KEY
        );
        let last = format!("{}á", "a".repeat(MAX_KEY - 1));
        let key = alphabet.input_key(&last).unwrap();
        assert_eq!(key.marks, 1 << (MAX_KEY - 1));
        assert_eq!(alphabet.spelling(&key, key.marks), last);
    }

    #[test]
    fn spelling_keys_keep_case() {
        let alphabet = alphabet();
        assert!(alphabet.spelling_key("Canción").is_none());
        assert!(alphabet.spelling_key("canción").is_some());
    }

    #[test]
    fn n_with_tilde_is_a_letter() {
        let alphabet = Alphabet::new("anoñ").unwrap();
        let (year, other) = (
            alphabet.input_key("año").unwrap(),
            alphabet.input_key("ano").unwrap(),
        );
        assert_ne!(year.bytes(), other.bytes());
        assert_eq!(year.marks, 0);
        assert_eq!(alphabet.spelling(&year, 0), "año");
    }

    #[test]
    fn restores_the_diaeresis() {
        let alphabet = Alphabet::new("aeginopstuü").unwrap();
        let spelled = |word: &str| -> Vec<String> {
            let key = alphabet.input_key(word).unwrap();
            alphabet
                .diaeresis_keys(&key)
                .iter()
                .map(|key| alphabet.spelling(key, key.marks))
                .collect()
        };
        assert_eq!(spelled("pinguino"), ["pingüino"]);
        assert_eq!(spelled("guiguei"), ["güiguei", "guigüei", "güigüei"]);
        assert!(spelled("agua").is_empty());
        assert!(spelled("gu").is_empty());
        let key = alphabet.input_key("pingúino").unwrap();
        let restored = alphabet.diaeresis_keys(&key);
        assert_eq!(restored.len(), 1);
        assert_eq!(restored[0].marks, key.marks);
        let without = Alphabet::new("gueino").unwrap();
        let key = without.input_key("guei").unwrap();
        assert!(without.diaeresis_keys(&key).is_empty());
    }

    #[test]
    fn rejects_broken_alphabets() {
        assert!(Alphabet::new("aba").is_err());
        assert!(Alphabet::new("ñañ").is_err());
        assert!(Alphabet::new("a\0").is_err());
        assert!(Alphabet::new("aá").is_err());
        let letters: String = ('\u{100}'..).take(256).collect();
        assert!(Alphabet::new(&letters).is_err());
        assert!(Alphabet::new(&letters[..letters.len() - 2]).is_ok());
    }

    #[test]
    fn order_of_marks() {
        let mut masks = vec![0b110, 0b100, 0b010, 0b000, 0b011];
        masks.sort_by(|&a, &b| mark_order(a, b));
        assert_eq!(masks, [0b000, 0b011, 0b010, 0b110, 0b100]);
    }
}
