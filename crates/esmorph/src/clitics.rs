//! Words of a token of several: a verb with enclitic pronouns (`dámelo` is `da` + `me` + `lo`) and
//! the contractions `del` and `al`

/// Clitics and their places: a clitic stands after the clitics of lower places, `se` first and
/// the third person last (`dáselo`, `cómetelo`, `démonos`)
const CLITICS: [(&str, u8); 11] = [
    ("se", 0),
    ("te", 1),
    ("os", 1),
    ("me", 2),
    ("nos", 2),
    ("lo", 3),
    ("la", 3),
    ("le", 3),
    ("los", 3),
    ("las", 3),
    ("les", 3),
];
/// Spanish puts no more than three clitics after a verb
const MAX_CLITICS: usize = 3;
/// Letters of the shortest host: `di`, `ve`, `id`
const MIN_HOST: usize = 2;

/// A split of a token: the host as written and its clitics
pub(crate) struct Split<'a> {
    pub host: &'a str,
    pub clitics: Vec<&'static str>,
}

/// Splits of a token in lower case into a host and clitics, the most clitics first: `vérselas` is
/// `ver` + `se` + `las`, not the imperative `verse` of `versar` + `las`
pub(crate) fn splits(token: &str) -> Vec<Split<'_>> {
    let mut found = Vec::new();
    let mut clitics = Vec::new();
    collect(token, &mut clitics, u8::MAX, &mut found);
    found.sort_by_key(|split| std::cmp::Reverse(split.clitics.len()));
    found
}

fn collect<'a>(
    rest: &'a str,
    clitics: &mut Vec<&'static str>,
    before: u8,
    found: &mut Vec<Split<'a>>,
) {
    if clitics.len() == MAX_CLITICS {
        return;
    }
    for &(clitic, place) in &CLITICS {
        let Some(host) = rest.strip_suffix(clitic) else {
            continue;
        };
        if place >= before || host.chars().count() < MIN_HOST {
            continue;
        }
        clitics.insert(0, clitic);
        found.push(Split {
            host,
            clitics: clitics.clone(),
        });
        collect(host, clitics, place, found);
        clitics.remove(0);
    }
}

/// Spellings of a host to look for: without the accent the clitics put on it (`dá` is `da`), as
/// written (`dé`), with the `s` that `nos` and `se` take away (`vámonos` is `vamos` + `nos`) and with
/// the `d` that `os` takes away (`sentaos` is `sentad` + `os`)
pub(crate) fn hosts(host: &str, first: &str) -> Vec<String> {
    let plain: String = host.chars().map(without_acute).collect();
    let mut spellings = Vec::with_capacity(3);
    if (first == "nos" || first == "se") && plain.ends_with("mo") {
        spellings.push(format!("{plain}s"));
    }
    if first == "os" && plain.ends_with(['a', 'e', 'i']) {
        spellings.push(format!("{plain}d"));
    }
    if plain != host {
        spellings.push(plain);
        spellings.push(host.to_owned());
    } else {
        spellings.push(plain);
    }
    spellings
}

fn without_acute(c: char) -> char {
    match c {
        'á' => 'a',
        'é' => 'e',
        'í' => 'i',
        'ó' => 'o',
        'ú' => 'u',
        _ => c,
    }
}

/// Words of a contraction: `del` is `de` + `el`
pub(crate) fn contraction(token: &str) -> Option<[&'static str; 2]> {
    if token.eq_ignore_ascii_case("del") {
        Some(["de", "el"])
    } else if token.eq_ignore_ascii_case("al") {
        Some(["a", "el"])
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn split_list(token: &str) -> Vec<(String, String)> {
        splits(token)
            .into_iter()
            .map(|split| (split.host.to_owned(), split.clitics.join("+")))
            .collect()
    }

    #[test]
    fn clitics_keep_their_order() {
        let found = split_list("dámelo");
        assert!(found.contains(&("dá".into(), "me+lo".into())));
        assert!(found.contains(&("dáme".into(), "lo".into())));
        assert!(split_list("dáselo").contains(&("dá".into(), "se+lo".into())));
        // `lo` never stands before `me`
        assert!(!split_list("dalome").iter().any(|(_, c)| c == "lo+me"));
        assert!(!split_list("damese").iter().any(|(_, c)| c == "me+se"));
    }

    #[test]
    fn most_clitics_first() {
        let found = splits("vérselas");
        assert!(
            found
                .windows(2)
                .all(|pair| pair[0].clitics.len() >= pair[1].clitics.len())
        );
        assert_eq!(found[0].host, "vér");
    }

    #[test]
    fn hosts_have_two_letters_at_least() {
        assert!(split_list("me").is_empty());
        assert!(split_list("ase").is_empty());
        assert_eq!(split_list("idos"), [("id".into(), "os".into())]);
    }

    #[test]
    fn three_clitics_at_most() {
        assert!(
            split_list("dásetemelo")
                .iter()
                .all(|(_, c)| c.split('+').count() <= 3)
        );
    }

    #[test]
    fn spellings_of_a_host() {
        assert_eq!(hosts("dá", "me"), ["da", "dá"]);
        assert_eq!(hosts("hacer", "lo"), ["hacer"]);
        assert_eq!(hosts("vámo", "nos"), ["vamos", "vamo", "vámo"]);
        assert_eq!(hosts("digámo", "se"), ["digamos", "digamo", "digámo"]);
        assert_eq!(hosts("senta", "os"), ["sentad", "senta"]);
        assert_eq!(hosts("vestí", "os"), ["vestid", "vesti", "vestí"]);
        assert_eq!(hosts("pingüino", "lo"), ["pingüino"]);
    }

    #[test]
    fn contractions() {
        assert_eq!(contraction("del"), Some(["de", "el"]));
        assert_eq!(contraction("Al"), Some(["a", "el"]));
        assert_eq!(contraction("el"), None);
    }
}
