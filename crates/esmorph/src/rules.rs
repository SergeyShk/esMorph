//! Analyses of tokens that the dictionary does not have: punctuation, symbols, numbers in digits,
//! Roman numerals and words with a capital letter (names, acronyms, initials). Parts of speech,
//! features and lemmas follow the UD guidelines and the treebanks UD Spanish AnCora and GSD; the
//! shares of ambiguous analyses come from their training parts

use crate::ud::UdTags;

/// Signs of punctuation with the same `PunctType` and `PunctSide`
const SIGNS: [(&str, &str, &str); 13] = [
    (",", "Comm", ""),
    (".", "Peri", ""),
    (":/", "Colo", ""),
    (";", "Semi", ""),
    ("…", "Elip", ""),
    ("-‐‑‒–—―", "Dash", ""),
    ("\"'«»“”„‘’‚‹›`´", "Quot", ""),
    ("([{", "Brck", "Ini"),
    (")]}", "Brck", "Fin"),
    ("¿", "Qest", "Ini"),
    ("?", "Qest", "Fin"),
    ("¡", "Excl", "Ini"),
    ("!", "Excl", "Fin"),
];

/// Symbols out of the symbol blocks of Unicode that are `SYM`, not `PUNCT`; the superscripts and
/// the ordinal indicators are a letter and digits for Unicode but signs for the treebanks
/// (`m²`, `25 º`)
const SYMBOLS: &[char] = &[
    '%', '‰', '$', '£', '¥', '¢', '°', '+', '=', '<', '>', '±', '×', '÷', '§', '©', '®', '&', '@',
    '#', '*', '~', '^', '|', '\\', '¶', '¬', '¹', '²', '³', 'º', 'ª',
];

/// Shares of the analyses of a Roman numeral the dictionary does not have, of several letters and
/// of one: a number or a name, by AnCora and GSD train
const ROMAN: [(f64, f64); 2] = [(363.0, 24.0), (32.0, 27.0)];
/// The longest Roman numeral below 4000: `MMMDCCCLXXXVIII`
const LONGEST_ROMAN: usize = 15;

/// Shares of a name among the analyses of a token with a capital letter that splits as a verb with
/// clitics, by AnCora and GSD train: of an imperative without an accent (`Mandela`, `Portela`: 45 of
/// 48) and of an infinitive, a gerund or an imperative with an accent (`Hacerlo`, `Déjame`: 4 of
/// 36), taken for a finite form too (`Levantóse`), which the treebanks of news hardly have
pub(crate) const CLITICS_NAME: [f64; 2] = [45.0 / 48.0, 4.0 / 36.0];

/// Share of a name among the analyses of a word with a capital letter and without accents that the
/// dictionary has only with them (`Angel`, `Perez`), by AnCora and GSD train (882 of 933)
pub(crate) const RESTORED_NAME: f64 = 882.0 / 933.0;

/// Share of a name among the analyses of a capital letter with a period that the dictionary knows
/// (`D.`, `V.`): an initial mostly, by AnCora and GSD train (48 of 63)
pub(crate) const INITIAL_NAME: f64 = 48.0 / 63.0;

/// Shares of a number and of a name among the analyses of a Roman numeral of one letter that the
/// dictionary knows (`I`, `X`), by AnCora and GSD train: 62 and 14 of 108
pub(crate) const ROMAN_LETTER: [f64; 2] = [62.0 / 108.0, 14.0 / 108.0];

/// Weight of the names among the analyses of a lowercase token: such a token is a name in 142 of
/// 21 442 in AnCora and GSD train, while the counts of the dictionary do not tell its case
pub(crate) const LOWER_NAME: f64 = 142.0 / 21442.0;

/// Shares of a name among the analyses of a word in capitals that the dictionary knows without
/// one, by AnCora and GSD train: a word of up to `SHORT_CAPS` letters is mostly an acronym (`ETA`,
/// `PP`), a longer one and a word of a closed class mostly a word of a headline (`CAMIONES`, `EL`)
pub(crate) const CAPS_NAME: [f64; 3] = [890.0 / 1079.0, 81.0 / 295.0, 39.0 / 252.0];
pub(crate) const SHORT_CAPS: usize = 4;

/// UD tags of the analyses by the rules, by their numbers in the tags of the dictionary
#[derive(Clone, Debug)]
pub(crate) struct RuleTags {
    punct: u16,
    signs: [u16; SIGNS.len()],
    symbol: u16,
    cardinal: u16,
    roman: u16,
    pub name: u16,
}

/// The kind of an analysis by a rule
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Rule {
    Punct,
    /// A sign of `SIGNS`, by its place there
    Sign(usize),
    Symbol,
    Cardinal,
    Roman,
    Name,
}

impl RuleTags {
    /// Finds the tags in `tags` and adds the missing ones; `None` if the table of tags is full
    pub fn add(tags: &mut UdTags) -> Option<Self> {
        let mut signs = [0; SIGNS.len()];
        for (number, &(_, kind, side)) in signs.iter_mut().zip(&SIGNS) {
            let feats: &[(&str, &str)] = if side.is_empty() {
                &[("PunctType", kind)]
            } else {
                &[("PunctSide", side), ("PunctType", kind)]
            };
            *number = tags.find_or_add("PUNCT", feats)?;
        }
        Some(RuleTags {
            punct: tags.find_or_add("PUNCT", &[])?,
            signs,
            symbol: tags.find_or_add("SYM", &[])?,
            cardinal: tags.find_or_add("NUM", &[("NumForm", "Digit"), ("NumType", "Card")])?,
            roman: tags.find_or_add("NUM", &[("NumForm", "Roman"), ("NumType", "Card")])?,
            name: tags.find_or_add("PROPN", &[])?,
        })
    }

    pub fn number(&self, rule: Rule) -> u16 {
        match rule {
            Rule::Punct => self.punct,
            Rule::Sign(at) => self.signs[at],
            Rule::Symbol => self.symbol,
            Rule::Cardinal => self.cardinal,
            Rule::Roman => self.roman,
            Rule::Name => self.name,
        }
    }
}

/// Analyses of a token by the rules: the lemma, the rule and the share; empty if no rule fits
pub(crate) fn rules(token: &str) -> Vec<(String, Rule, f64)> {
    // A replacement character is a lost character of the text, not a sign
    if token.is_empty()
        || (token.chars()).any(|c| c.is_whitespace() || c.is_control() || c == '\u{FFFD}')
    {
        return Vec::new();
    }
    if token.chars().all(|c| !c.is_alphanumeric() || symbol(c)) {
        let rule = if token.chars().any(symbol) {
            Rule::Symbol
        } else {
            punctuation(token)
        };
        return vec![(token.to_owned(), rule, 1.0)];
    }
    if let Some(lemma) = number(token) {
        return vec![(lemma, Rule::Cardinal, 1.0)];
    }
    if roman(token) {
        let (number, name) = roman_shares(token);
        return vec![
            (token.to_owned(), Rule::Roman, number),
            (token.to_owned(), Rule::Name, name),
        ];
    }
    if token.chars().any(char::is_uppercase) {
        return vec![(token.to_owned(), Rule::Name, 1.0)];
    }
    Vec::new()
}

/// Shares of a Roman numeral and of a name for a Roman numeral
pub(crate) fn roman_shares(token: &str) -> (f64, f64) {
    let (number, name) = ROMAN[usize::from(token.len() == 1)];
    (number / (number + name), name / (number + name))
}

/// Whether a token is a word in capitals: two letters at least and no lowercase one (`ETA`, `PP`,
/// `TV3`)
pub(crate) fn caps(token: &str) -> bool {
    !token.chars().any(char::is_lowercase)
        && token.chars().filter(|c| c.is_alphabetic()).count() >= 2
}

/// The kind of punctuation of a token of signs: the sign of `SIGNS` that all its characters are,
/// an ellipsis for several periods
fn punctuation(token: &str) -> Rule {
    if token.len() > 1 && token.bytes().all(|b| b == b'.') {
        return Rule::Sign(
            SIGNS
                .iter()
                .position(|&(_, kind, _)| kind == "Elip")
                .unwrap_or(0),
        );
    }
    let sign = |c: char| SIGNS.iter().position(|&(signs, ..)| signs.contains(c));
    match token.chars().next().and_then(sign) {
        Some(at) if token.chars().all(|c| sign(c) == Some(at)) => Rule::Sign(at),
        _ => Rule::Punct,
    }
}

/// A symbol: the signs of `SYMBOLS` and the symbol blocks of Unicode - currencies, letterlike
/// symbols, arrows, mathematical operators, technical and geometric signs, emoji
fn symbol(c: char) -> bool {
    SYMBOLS.contains(&c)
        || matches!(
            c,
            '\u{2070}'..='\u{209F}'
                | '\u{20A0}'..='\u{20CF}'
                | '\u{2100}'..='\u{214F}'
                | '\u{2190}'..='\u{23FF}'
                | '\u{25A0}'..='\u{27BF}'
                | '\u{2B00}'..='\u{2BFF}'
                | '\u{1F000}'..='\u{1FAFF}'
        )
}

/// The lemma of a number in digits: groups of digits joined by `.`, `,`, `:`, `/` or a dash
/// (`3,14`, `15:00`, `1990/92`, `2-1`); the periods between thousands go away (`10.000` is
/// `10000`), the rest stays as written
fn number(token: &str) -> Option<String> {
    if !(token.split(['.', ',', ':', '/', '-', '–']))
        .all(|group| !group.is_empty() && group.bytes().all(|b| b.is_ascii_digit()))
    {
        return None;
    }
    let (whole, fraction) = token.split_once(',').unwrap_or((token, ""));
    let mut parts = whole.split('.');
    let first = parts.next().unwrap_or_default();
    let thousands = whole.contains('.')
        && (1..=3).contains(&first.len())
        && first.bytes().all(|b| b.is_ascii_digit())
        && parts.all(|group| group.len() == 3 && group.bytes().all(|b| b.is_ascii_digit()));
    if !thousands {
        return Some(token.to_owned());
    }
    let mut lemma = whole.replace('.', "");
    if token.contains(',') {
        lemma.push(',');
        lemma.push_str(fraction);
    }
    Some(lemma)
}

/// A Roman numeral in capitals in its canonical spelling, below 4000: `XIV`, `MMXXVI`, not `IIII`
pub(crate) fn roman(token: &str) -> bool {
    const DIGITS: [(&str, u32); 13] = [
        ("M", 1000),
        ("CM", 900),
        ("D", 500),
        ("CD", 400),
        ("C", 100),
        ("XC", 90),
        ("L", 50),
        ("XL", 40),
        ("X", 10),
        ("IX", 9),
        ("V", 5),
        ("IV", 4),
        ("I", 1),
    ];
    if token.is_empty()
        || token.len() > LONGEST_ROMAN
        || !token.chars().all(|c| "MDCLXVI".contains(c))
    {
        return false;
    }
    let mut value = 0;
    let mut rest = token;
    while let Some(&(digit, amount)) = DIGITS.iter().find(|(digit, _)| rest.starts_with(digit)) {
        value += amount;
        rest = &rest[digit.len()..];
    }
    if !rest.is_empty() || value == 0 || value >= 4000 {
        return false;
    }
    let mut canonical = String::new();
    let mut left = value;
    for (digit, amount) in DIGITS {
        while left >= amount {
            canonical.push_str(digit);
            left -= amount;
        }
    }
    canonical == token
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(token: &str) -> Vec<(String, Rule)> {
        rules(token)
            .into_iter()
            .map(|(lemma, rule, _)| (lemma, rule))
            .collect()
    }

    fn sign(kind: &str, side: &str) -> Rule {
        Rule::Sign(
            SIGNS
                .iter()
                .position(|&(_, k, s)| k == kind && s == side)
                .unwrap(),
        )
    }

    #[test]
    fn punctuation_and_symbols() {
        assert_eq!(kinds(","), [(",".into(), sign("Comm", ""))]);
        assert_eq!(kinds("¿"), [("¿".into(), sign("Qest", "Ini"))]);
        assert_eq!(kinds(")"), [(")".into(), sign("Brck", "Fin"))]);
        assert_eq!(kinds("«"), [("«".into(), sign("Quot", ""))]);
        assert_eq!(kinds("..."), [("...".into(), sign("Elip", ""))]);
        assert_eq!(kinds("—"), [("—".into(), sign("Dash", ""))]);
        assert_eq!(kinds("--"), [("--".into(), sign("Dash", ""))]);
        // Signs of different kinds and signs out of the table have no features
        assert_eq!(kinds("?!"), [("?!".into(), Rule::Punct)]);
        assert_eq!(kinds("/"), [("/".into(), sign("Colo", ""))]);
        assert_eq!(kinds("…/"), [("…/".into(), Rule::Punct)]);
        for symbol in ["%", "€", "+", "&", "→", "😀", "²", "º", "ª", "⁴"] {
            assert_eq!(kinds(symbol), [(symbol.into(), Rule::Symbol)], "{symbol}");
        }
    }

    #[test]
    fn numbers() {
        assert_eq!(kinds("1999"), [("1999".into(), Rule::Cardinal)]);
        assert_eq!(kinds("10.000"), [("10000".into(), Rule::Cardinal)]);
        assert_eq!(kinds("1.234.567"), [("1234567".into(), Rule::Cardinal)]);
        assert_eq!(kinds("1.234,5"), [("1234,5".into(), Rule::Cardinal)]);
        // Not a group of thousands: a decimal point stays
        assert_eq!(kinds("3.14"), [("3.14".into(), Rule::Cardinal)]);
        assert_eq!(kinds("3,14"), [("3,14".into(), Rule::Cardinal)]);
        assert_eq!(kinds("15:00"), [("15:00".into(), Rule::Cardinal)]);
        assert_eq!(kinds("2-1"), [("2-1".into(), Rule::Cardinal)]);
        assert_eq!(kinds("1990/92"), [("1990/92".into(), Rule::Cardinal)]);
        for word in ["1.", "3,", ".5", "1..2", "1a"] {
            assert!(
                !kinds(word).iter().any(|(_, rule)| *rule == Rule::Cardinal),
                "{word}"
            );
        }
    }

    #[test]
    fn roman_numerals() {
        for word in ["II", "XIV", "XXI", "MMXXVI", "CD", "I"] {
            assert!(roman(word), "{word}");
        }
        for word in ["IIII", "IC", "VX", "XXXX", "ii", "", "MMMM"] {
            assert!(!roman(word), "{word}");
        }
        assert!(roman("MMMDCCCLXXXVIII"));
        // A long token is no numeral and is not summed
        assert!(!roman(&"M".repeat(5_000_000)));
        let analyses = rules("XXI");
        assert_eq!(analyses[0].1, Rule::Roman);
        assert_eq!(analyses[1].1, Rule::Name);
        assert!(analyses[0].2 > 0.9);
        let letter = rules("V");
        assert!(letter[0].2 > letter[1].2 && letter[1].2 > 0.4);
    }

    #[test]
    fn names() {
        for word in [
            "Josep",
            "CiU",
            "OTAN",
            "W.",
            "TV-3",
            "A21",
            "d'Esquadra",
            "Macapagal-Arroyo",
        ] {
            assert_eq!(kinds(word), [(word.into(), Rule::Name)], "{word}");
        }
        assert!(rules("qzxw").is_empty());
        assert!(rules("a b").is_empty());
        assert!(rules("").is_empty());
        assert!(caps("ETA") && caps("TV3") && caps("EE.UU."));
        assert!(!caps("Eta") && !caps("A") && !caps("F1") && !caps("1999"));
    }

    #[test]
    fn tags_of_the_rules() {
        let mut tags = UdTags::default();
        let rules = RuleTags::add(&mut tags).unwrap();
        let tag = tags.get(rules.number(sign("Qest", "Ini"))).unwrap();
        assert_eq!(tag.pos(), "PUNCT");
        assert_eq!(tag.feats_string(), "PunctSide=Ini|PunctType=Qest");
        let number = tags.get(rules.number(Rule::Cardinal)).unwrap();
        assert_eq!(number.feats_string(), "NumForm=Digit|NumType=Card");
        // Adding again finds the same tags
        let count = tags.len();
        RuleTags::add(&mut tags).unwrap();
        assert_eq!(tags.len(), count);
    }
}
