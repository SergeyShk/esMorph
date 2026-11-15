//! Colloquial and dialectal spellings of standard words: a participle without its `d`
//! (`trabajao` is `trabajado`, `sío` is `sido`), a word without its final `d` (`caridá` is
//! `caridad`) and an aspirated `f` or `h` (`juera` is `fuera`, `juevo` is `huevo`)

/// Endings of such spellings and of their standard words
const ENDINGS: [(&str, &str); 7] = [
    ("aos", "ados"),
    ("ao", "ado"),
    ("íos", "idos"),
    ("ío", "ido"),
    ("á", "ad"),
    ("é", "ed"),
    ("ú", "ud"),
];

/// Starts of such spellings and of their standard words: `ju` is mostly `fu`, so it goes before
/// `hue` (`juera` is `fuera`, not `huera`)
const STARTS: [(&str, &str); 3] = [("ju", "fu"), ("jue", "hue"), ("aju", "afu")];

/// Standard spellings that a lowercase word may stand for, the likelier first; the word itself
/// is no spelling of a standard word unless the dictionary has one of them
pub(crate) fn standard(word: &str) -> impl Iterator<Item = String> + '_ {
    let endings = ENDINGS.iter().filter_map(move |&(ending, standard)| {
        let stem = word.strip_suffix(ending).filter(|stem| !stem.is_empty())?;
        Some(format!("{stem}{standard}"))
    });
    let starts = STARTS.iter().filter_map(move |&(start, standard)| {
        let rest = word.strip_prefix(start).filter(|rest| !rest.is_empty())?;
        Some(format!("{standard}{rest}"))
    });
    endings.chain(starts)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spellings(word: &str) -> Vec<String> {
        standard(word).collect()
    }

    #[test]
    fn spellings_of_standard_words() {
        assert_eq!(spellings("trabajao"), ["trabajado"]);
        assert_eq!(spellings("cansaos"), ["cansados"]);
        assert_eq!(spellings("sío"), ["sido"]);
        assert_eq!(spellings("teníos"), ["tenidos"]);
        assert_eq!(spellings("verdá"), ["verdad"]);
        assert_eq!(spellings("usté"), ["usted"]);
        assert_eq!(spellings("virtú"), ["virtud"]);
        assert_eq!(spellings("juerza"), ["fuerza", "huerza"]);
        assert_eq!(spellings("juera"), ["fuera", "huera"]);
        assert_eq!(spellings("ajuera"), ["afuera"]);
        assert_eq!(spellings("jugao"), ["jugado", "fugao"]);
    }

    #[test]
    fn a_spelling_keeps_a_stem() {
        assert!(spellings("ao").is_empty());
        assert!(spellings("á").is_empty());
        assert!(spellings("ju").is_empty());
        assert!(spellings("casa").is_empty());
    }
}
