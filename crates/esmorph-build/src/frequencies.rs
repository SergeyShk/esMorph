//! Order and probabilities of the analyses of a word form by the counts of treebanks: the probability
//! is P(part of speech | form) P(lemma | form, part of speech) P(features | form, part of speech,
//! lemma). What the treebanks did not see of a form falls back on its last letters and on the
//! features of its part of speech

use std::collections::{HashMap, HashSet};

use esmorph::Error;

/// How many letters of the end of a form tell its part of speech if the form is unseen
const SUFFIX: usize = 3;
/// Weight of what the treebanks did not see against the counts of a form, as that many words
const PRIOR: f64 = 1.0;

/// An analysis of a form
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Row<'a> {
    pub lemma: &'a str,
    pub pos: &'a str,
    pub feats: &'a str,
}

/// Analyses of a form, the most probable first
#[derive(Debug, PartialEq)]
pub struct Ranked<'a> {
    pub rows: Vec<Row<'a>>,
    /// Probabilities of the rows; `None` if the treebanks did not see the form
    pub probabilities: Option<Vec<f64>>,
}

/// Counts of the treebanks
#[derive(Default)]
pub struct Counts {
    /// Form and part of speech: all treebanks
    parts: HashMap<(String, String), u32>,
    /// Form, part of speech and lemma
    lemmas: HashMap<(String, String, String), u32>,
    /// Form, part of speech, lemma and features
    analyses: HashMap<(String, String, String, String), u32>,
    /// End of a form and part of speech
    suffixes: HashMap<(String, String), u32>,
    /// Part of speech and features
    tags: HashMap<(String, String), u32>,
}

fn number(line: &str, text: &str) -> Result<u32, Error> {
    text.parse()
        .map_err(|_| Error::Build(format!("wrong count in {line:?}")))
}

/// The last letters of a form
fn suffix(form: &str) -> &str {
    let start = form
        .char_indices()
        .rev()
        .nth(SUFFIX - 1)
        .map_or(0, |(at, _)| at);
    &form[start..]
}

impl Counts {
    /// Counts from the text of `analyses.tsv` (form, part of speech, lemma, features, count) and
    /// `pos.tsv` (form, part of speech, count)
    pub fn read(analyses: &str, parts: &str) -> Result<Self, Error> {
        let mut counts = Counts::default();
        for line in analyses.lines() {
            let &[form, pos, lemma, feats, count] = line.split('\t').collect::<Vec<_>>().as_slice()
            else {
                return Err(Error::Build(format!("wrong line of analyses {line:?}")));
            };
            let count = number(line, count)?;
            *counts
                .lemmas
                .entry((form.to_owned(), pos.to_owned(), lemma.to_owned()))
                .or_default() += count;
            *counts
                .tags
                .entry((pos.to_owned(), feats.to_owned()))
                .or_default() += count;
            counts.analyses.insert(
                (
                    form.to_owned(),
                    pos.to_owned(),
                    lemma.to_owned(),
                    feats.to_owned(),
                ),
                count,
            );
        }
        for line in parts.lines() {
            let &[form, pos, count] = line.split('\t').collect::<Vec<_>>().as_slice() else {
                return Err(Error::Build(format!("wrong line of parts {line:?}")));
            };
            let count = number(line, count)?;
            counts
                .parts
                .insert((form.to_owned(), pos.to_owned()), count);
            *counts
                .suffixes
                .entry((suffix(form).to_owned(), pos.to_owned()))
                .or_default() += count;
        }
        Ok(counts)
    }

    fn part(&self, form: &str, pos: &str) -> f64 {
        let key = (form.to_owned(), pos.to_owned());
        f64::from(self.parts.get(&key).copied().unwrap_or(0))
    }

    fn lemma(&self, form: &str, pos: &str, lemma: &str) -> f64 {
        let key = (form.to_owned(), pos.to_owned(), lemma.to_owned());
        f64::from(self.lemmas.get(&key).copied().unwrap_or(0))
    }

    fn analysis(&self, form: &str, row: Row) -> f64 {
        let key = (
            form.to_owned(),
            row.pos.to_owned(),
            row.lemma.to_owned(),
            row.feats.to_owned(),
        );
        f64::from(self.analyses.get(&key).copied().unwrap_or(0))
    }

    fn ending(&self, form: &str, pos: &str) -> f64 {
        let key = (suffix(form).to_owned(), pos.to_owned());
        f64::from(self.suffixes.get(&key).copied().unwrap_or(0)) + 1.0
    }

    fn tag(&self, row: Row) -> f64 {
        let key = (row.pos.to_owned(), row.feats.to_owned());
        f64::from(self.tags.get(&key).copied().unwrap_or(0)) + 1.0
    }

    /// Analyses of a form in their order. Equal probabilities prefer the word as its own lemma and
    /// then the shorter lemma
    pub fn rank<'a>(&self, form: &str, rows: &[Row<'a>]) -> Ranked<'a> {
        let distinct = |pick: fn(&Row<'a>) -> &'a str, of: &[Row<'a>]| -> Vec<&'a str> {
            let mut seen = HashSet::new();
            of.iter().map(pick).filter(|&v| seen.insert(v)).collect()
        };
        let poses = distinct(|row| row.pos, rows);
        // Adjectives in `-ada` are mostly other words: the ending of a participle says it is a verb
        let participle = rows
            .iter()
            .any(|row| row.pos == "VERB" && row.feats.contains("VerbForm=Part"));
        let ending = |pos: &str| {
            if participle && pos == "ADJ" {
                1.0
            } else {
                self.ending(form, pos)
            }
        };
        let seen_pos: f64 = poses.iter().map(|pos| self.part(form, pos)).sum();
        let endings: f64 = poses.iter().map(|pos| ending(pos)).sum();
        let mut scored = Vec::with_capacity(rows.len());
        for &pos in &poses {
            let of_pos: Vec<Row<'a>> = rows.iter().copied().filter(|r| r.pos == pos).collect();
            let prior = PRIOR * ending(pos) / endings;
            let p_pos = (self.part(form, pos) + prior) / (seen_pos + PRIOR);
            let lemmas = distinct(|row| row.lemma, &of_pos);
            let seen_lemma: f64 = lemmas.iter().map(|l| self.lemma(form, pos, l)).sum();
            for &lemma in &lemmas {
                let of_lemma: Vec<Row<'a>> = of_pos
                    .iter()
                    .copied()
                    .filter(|r| r.lemma == lemma)
                    .collect();
                let prior = PRIOR / lemmas.len() as f64;
                let p_lemma = (self.lemma(form, pos, lemma) + prior) / (seen_lemma + PRIOR);
                let seen_tag: f64 = of_lemma.iter().map(|&r| self.analysis(form, r)).sum();
                let tags: f64 = of_lemma.iter().map(|&r| self.tag(r)).sum();
                for &row in &of_lemma {
                    let prior = PRIOR * self.tag(row) / tags;
                    let p_tag = (self.analysis(form, row) + prior) / (seen_tag + PRIOR);
                    scored.push((row, p_pos * p_lemma * p_tag));
                }
            }
        }
        scored.sort_by(|(a, pa), (b, pb)| {
            pb.total_cmp(pa)
                .then_with(|| (b.lemma == form).cmp(&(a.lemma == form)))
                .then_with(|| a.lemma.len().cmp(&b.lemma.len()))
                .then_with(|| (a.lemma, a.pos, a.feats).cmp(&(b.lemma, b.pos, b.feats)))
        });
        let (rows, values) = scored.into_iter().unzip();
        Ranked {
            rows,
            probabilities: (seen_pos > 0.0).then_some(values),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ANALYSES: &str = "que\tPRON\tque\tPronType=Rel\t60\n\
                            que\tSCONJ\tque\t_\t40\n\
                            come\tVERB\tcomer\tMood=Ind\t2\n";
    const PARTS: &str = "que\tPRON\t60\n\
                         que\tSCONJ\t140\n\
                         come\tVERB\t2\n\
                         casas\tNOUN\t10\n\
                         mesas\tNOUN\t10\n\
                         pasas\tVERB\t2\n";

    fn counts() -> Counts {
        Counts::read(ANALYSES, PARTS).unwrap()
    }

    fn row<'a>(lemma: &'a str, pos: &'a str, feats: &'a str) -> Row<'a> {
        Row { lemma, pos, feats }
    }

    #[test]
    fn parts_of_speech_count_in_all_treebanks() {
        let ranked = counts().rank(
            "que",
            &[row("que", "PRON", "PronType=Rel"), row("que", "SCONJ", "_")],
        );
        assert_eq!(ranked.rows[0].pos, "SCONJ");
        let probabilities = ranked.probabilities.unwrap();
        assert!(probabilities[0] > probabilities[1]);
        assert!((probabilities.iter().sum::<f64>() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn unseen_form_has_an_order_and_no_probabilities() {
        let rows = [row("cosa", "NOUN", "_"), row("cosar", "VERB", "_")];
        let ranked = counts().rank("cosas", &rows);
        assert_eq!(ranked.rows[0].pos, "NOUN");
        assert_eq!(ranked.probabilities, None);
    }

    #[test]
    fn equal_probabilities_prefer_the_own_lemma() {
        let rows = [row("niño", "NOUN", "F"), row("niña", "NOUN", "F")];
        assert_eq!(counts().rank("niña", &rows).rows[0].lemma, "niña");
    }

    #[test]
    fn features_of_the_part_of_speech_decide_for_an_unseen_row() {
        let rows = [
            row("comer", "VERB", "Mood=Imp"),
            row("comer", "VERB", "Mood=Ind"),
        ];
        assert_eq!(counts().rank("comes", &rows).rows[0].feats, "Mood=Ind");
    }

    #[test]
    fn an_unseen_participle_is_a_verb() {
        let counts =
            Counts::read("", "colada\tADJ\t50\nmirada\tADJ\t50\npasada\tVERB\t2\n").unwrap();
        let rows = [
            row("comprado", "ADJ", "VerbForm=Part"),
            row("comprar", "VERB", "VerbForm=Part"),
        ];
        assert_eq!(counts.rank("comprada", &rows).rows[0].pos, "VERB");
        let rows = [
            row("cerrada", "ADJ", "_"),
            row("cerrada", "VERB", "VerbForm=Inf"),
        ];
        assert_eq!(counts.rank("cerrada", &rows).rows[0].pos, "ADJ");
    }

    #[test]
    fn short_forms_have_a_suffix() {
        assert_eq!(suffix("a"), "a");
        assert_eq!(suffix("año"), "año");
        assert_eq!(suffix("camión"), "ión");
    }

    #[test]
    fn wrong_counts_are_errors() {
        assert!(Counts::read("que\tPRON\n", "").is_err());
        assert!(Counts::read("", "que\tPRON\tmany\n").is_err());
    }
}
