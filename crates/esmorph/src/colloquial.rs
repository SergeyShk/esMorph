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
/// `hue` (`juera` is `fuera`, not `huera`), and `hu` last (`juyendo` is `huyendo`)
const STARTS: [(&str, &str); 4] = [("ju", "fu"), ("jue", "hue"), ("aju", "afu"), ("ju", "hu")];

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

/// Whether a word has an ending or a start of such spellings, without spelling its standard words
pub(crate) fn may_be_spelling(word: &str) -> bool {
    let stem = |rest: Option<&str>| rest.is_some_and(|rest| !rest.is_empty());
    (ENDINGS.iter()).any(|&(ending, _)| stem(word.strip_suffix(ending)))
        || (STARTS.iter()).any(|&(start, _)| stem(word.strip_prefix(start)))
}

/// Whether a word ends as a participle without its `d` that has an accent: `perdío`, `metíos`
pub(crate) fn is_participle(word: &str) -> bool {
    ["ío", "íos"].iter().any(|ending| {
        word.strip_suffix(ending)
            .is_some_and(|stem| !stem.is_empty())
    })
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
        assert_eq!(spellings("juerza"), ["fuerza", "huerza", "huerza"]);
        assert_eq!(spellings("juera"), ["fuera", "huera", "huera"]);
        assert_eq!(spellings("juyendo"), ["fuyendo", "huyendo"]);
        assert_eq!(spellings("ajuera"), ["afuera"]);
        assert_eq!(spellings("jugao"), ["jugado", "fugao", "hugao"]);
    }

    #[test]
    fn participles_with_an_accent() {
        assert!(is_participle("perdío") && is_participle("metíos"));
        assert!(!is_participle("ío") && !is_participle("trabajao") && !is_participle("perdió"));
    }

    #[test]
    fn a_word_may_be_a_spelling_as_it_has_standard_spellings() {
        for word in [
            "trabajao",
            "verdá",
            "juera",
            "ao",
            "á",
            "ju",
            "casa",
            "erisipela",
        ] {
            assert_eq!(may_be_spelling(word), !spellings(word).is_empty(), "{word}");
        }
    }

    #[test]
    fn a_spelling_keeps_a_stem() {
        assert!(spellings("ao").is_empty());
        assert!(spellings("á").is_empty());
        assert!(spellings("ju").is_empty());
        assert!(spellings("casa").is_empty());
    }
}
