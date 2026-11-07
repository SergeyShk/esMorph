//! Spanish dictionary for the esMorph morphological analyzer: the Spanish entries of the English
//! Wiktionary in the format of esMorph, licensed under CC BY-SA 4.0
//!
//! The crate `esmorph` loads the dictionary: `esmorph::Dictionary::bundled()`

/// Bytes aligned to 8: so the dictionary is read right from the memory of the program, without a copy
#[repr(C, align(8))]
struct Aligned<T: ?Sized>(T);

static ALIGNED: &Aligned<[u8]> = &Aligned(*include_bytes!(concat!(env!("OUT_DIR"), "/es.dict")));

/// The dictionary file; empty if the crate is built without a dictionary
pub static DICTIONARY: &[u8] = &ALIGNED.0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aligned() {
        assert_eq!(DICTIONARY.as_ptr() as usize % 8, 0);
        assert!(DICTIONARY.is_empty() || DICTIONARY.starts_with(b"ESMORPH\0"));
    }
}
