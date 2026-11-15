//! Constraints of Spanish on the analyses the context model chooses: an article before a finite
//! verb is a pronoun (`no la descubras`), an infinitive after a verb is no noun (`puede ser`), a
//! form of `ser` and `ir` is `ir` where only `ir` fits (`se fue`, `fue a casa`, `fue cayendo`), an
//! imperative opening a sentence is no name (`Indique si desea`), a word after a clitic or the
//! negation is a verb (`que te vayas`, `no vayas`), a word that is mostly a verb with clitics is
//! one (`¡Cállate!`) and a rare verb does not take the place of a frequent one with the same form
//! (`venga` is `venir`, not `vengar`)

use super::{Analysis, Dictionary, Source};
use crate::clitics;

/// Clitic pronouns before a verb: a noun or an article cannot follow them
const PROCLITICS: [&str; 5] = ["me", "te", "se", "nos", "os"];
/// Forms of an article that are also the clitic pronouns of an object
const ARTICLE_CLITICS: [&str; 4] = ["la", "las", "los", "lo"];
/// Forms shared by `ser` and `ir`: the preterite and the past subjunctive
const SER_IR: [&str; 19] = [
    "fui",
    "fuiste",
    "fue",
    "fuimos",
    "fuisteis",
    "fueron",
    "fuera",
    "fueras",
    "fuéramos",
    "fuerais",
    "fueran",
    "fuese",
    "fueses",
    "fuésemos",
    "fueseis",
    "fuesen",
    "fuere",
    "fueres",
    "fueren",
];
/// Parts of speech of a word that heads the goal of `ir a`: `fue a casa`, `fue a ver`
const GOAL: [&str; 6] = ["DET", "NOUN", "PROPN", "PRON", "NUM", "VERB"];
/// Words a verb follows, not a noun without a determiner: the negation and the subject `tú`
const BEFORE_VERB: [&str; 2] = ["no", "tú"];
/// Lemmas of the copulas, whose predicate is a noun (`será titular`)
const COPULAS: [&str; 2] = ["ser", "estar"];
/// Shares of the verbs of a form in the counts of the dictionary: the model, which sees no lemma,
/// does not choose a verb below RARE where another is above FREQUENT
const RARE: f64 = 0.3;
const FREQUENT: f64 = 0.6;
/// Parts of speech of the word after an imperative opening a sentence, not after a name:
/// `Indique si`, `Diga usted`, `Señale el`, but `Defensa quiere`, `Hacienda ya`
const AFTER_IMPERATIVE: [&str; 4] = ["SCONJ", "PRON", "DET", "ADP"];

impl Dictionary {
    /// Applies the constraints to the analyses of the words of a sentence, ranked by the context
    /// model; `shares` - the shares of the verbs of every word by `verb_shares`, `start` - whether
    /// the words start the sentence. A constraint moves the analysis it chooses to the front, and
    /// the probabilities stay those of the model
    pub(crate) fn constrain(
        &self,
        words: &[&str],
        sentence: &mut [Vec<Analysis>],
        shares: &[Vec<(String, f64)>],
        start: bool,
    ) {
        let lower: Vec<String> = words.iter().map(|word| word.to_lowercase()).collect();
        for at in 0..sentence.len() {
            let chosen = clitic_homonym(&lower[at], &sentence[at])
                .or_else(|| self.article_clitic(&lower, sentence, at))
                .or_else(|| self.infinitive(sentence, at))
                .or_else(|| self.ser_ir(&lower, sentence, at))
                .or_else(|| self.verb_after(&lower, sentence, at))
                .or_else(|| self.opening_word(words, sentence, at, start))
                .or_else(|| self.frequent_lemma(&sentence[at], &shares[at]));
            if let Some(index) = chosen {
                promote(&mut sentence[at], index);
            }
        }
    }

    /// Part of speech of an analysis
    fn pos(&self, analysis: &Analysis) -> &str {
        self.ud_tag(analysis.ud).map_or("", |tag| tag.pos())
    }

    /// Whether an analysis has a feature with a value among its values
    fn has(&self, analysis: &Analysis, name: &str, value: &str) -> bool {
        self.ud_tag(analysis.ud).is_some_and(|tag| {
            tag.feats()
                .any(|(feat, values)| feat == name && values.split(',').any(|v| v == value))
        })
    }

    /// Part of speech of the first analysis of a word
    fn top_pos<'a>(&'a self, sentence: &'a [Vec<Analysis>], at: usize) -> &'a str {
        sentence
            .get(at)
            .and_then(|list| list.first())
            .map_or("", |analysis| self.pos(analysis))
    }

    /// Whether the first analysis of a word is a verb form with a value of `VerbForm`
    fn top_verb_form(&self, sentence: &[Vec<Analysis>], at: usize, form: &str) -> bool {
        sentence
            .get(at)
            .and_then(|list| list.first())
            .is_some_and(|analysis| {
                matches!(self.pos(analysis), "VERB" | "AUX") && self.has(analysis, "VerbForm", form)
            })
    }

    /// An article before a finite verb is the clitic pronoun: `no la descubras`, `¿dónde las has
    /// puesto?`; after a preposition or a determiner it stays an article (`de la luthiere`)
    fn article_clitic(
        &self,
        lower: &[String],
        sentence: &[Vec<Analysis>],
        at: usize,
    ) -> Option<usize> {
        let list = &sentence[at];
        if !ARTICLE_CLITICS.contains(&lower[at].as_str())
            || self.pos(list.first()?) != "DET"
            || !self.top_verb_form(sentence, at + 1, "Fin")
            || (at > 0 && matches!(self.top_pos(sentence, at - 1), "ADP" | "DET"))
        {
            return None;
        }
        list.iter()
            .position(|analysis| self.pos(analysis) == "PRON")
    }

    /// A word the model reads as a common noun right after a verb other than a copula is its
    /// infinitive: `puede ser`, `suele parecer`, but `será titular`
    fn infinitive(&self, sentence: &[Vec<Analysis>], at: usize) -> Option<usize> {
        let list = &sentence[at];
        let before = at
            .checked_sub(1)
            .and_then(|before| sentence[before].first())?;
        if self.pos(list.first()?) != "NOUN"
            || !matches!(self.pos(before), "VERB" | "AUX")
            || COPULAS.contains(&before.lemma.as_str())
        {
            return None;
        }
        list.iter().position(|analysis| {
            matches!(self.pos(analysis), "VERB" | "AUX") && self.has(analysis, "VerbForm", "Inf")
        })
    }

    /// A form of `ser` and `ir` is `ir` after a reflexive clitic but before a predicate (`se fue`,
    /// `me fui de casa`, not `me fue útil`), before `a` and its goal (`fue a casa`, `fueron a ver`)
    /// and before a gerund (`fue cayendo`)
    fn ser_ir(&self, lower: &[String], sentence: &[Vec<Analysis>], at: usize) -> Option<usize> {
        let list = &sentence[at];
        if !SER_IR.contains(&lower[at].as_str()) || list.first()?.lemma == "ir" {
            return None;
        }
        let next = lower.get(at + 1).map_or("", String::as_str);
        let next_pos = self.top_pos(sentence, at + 1);
        let after_clitic = at > 0
            && PROCLITICS.contains(&lower[at - 1].as_str())
            && next != "de"
            && next_pos != "ADJ"
            && !self.top_verb_form(sentence, at + 1, "Part");
        let goal = matches!(next, "a" | "al")
            && (next == "al" || GOAL.contains(&self.top_pos(sentence, at + 2)));
        let gerund = self.top_verb_form(sentence, at + 1, "Ger");
        if !(after_clitic || goal || gerund) {
            return None;
        }
        list.iter().position(|analysis| analysis.lemma == "ir")
    }

    /// A word the model reads as a noun after a reflexive or a dative clitic, the negation or
    /// `tú` is a finite verb: `que te vayas`, `no vayas`, `adonde tú vayas`
    fn verb_after(&self, lower: &[String], sentence: &[Vec<Analysis>], at: usize) -> Option<usize> {
        let list = &sentence[at];
        let before = at.checked_sub(1)?;
        let clitic = PROCLITICS.contains(&lower[before].as_str())
            && self.top_pos(sentence, before) == "PRON";
        if !(clitic || BEFORE_VERB.contains(&lower[before].as_str()))
            || !matches!(self.pos(list.first()?), "NOUN" | "PROPN")
        {
            return None;
        }
        list.iter().position(|analysis| {
            matches!(self.pos(analysis), "VERB" | "AUX") && self.has(analysis, "VerbForm", "Fin")
        })
    }

    /// A capitalized word opening a sentence that a rule reads as a name is its imperative when the
    /// dictionary has one and a lowercase word of AFTER_IMPERATIVE follows: `Indique si desea`, not
    /// `Tony`, `Lite - Brite` or `Defensa quiere`
    fn opening_word(
        &self,
        words: &[&str],
        sentence: &[Vec<Analysis>],
        at: usize,
        start: bool,
    ) -> Option<usize> {
        let list = &sentence[at];
        let opening = start
            && sentence[..at].iter().all(|list| {
                list.first()
                    .is_some_and(|analysis| self.pos(analysis) == "PUNCT")
            })
            && words
                .get(at + 1)
                .and_then(|word| word.chars().next())
                .is_some_and(char::is_lowercase)
            && AFTER_IMPERATIVE.contains(&self.top_pos(sentence, at + 1));
        if !opening || list.first()?.source != Source::Rule || self.pos(&list[0]) != "PROPN" {
            return None;
        }
        list.iter().position(|analysis| {
            analysis.source == Source::Dictionary
                && self.pos(analysis) == "VERB"
                && self.has(analysis, "Mood", "Imp")
        })
    }

    /// Shares of the verbs of a word in the counts if it is the form of several (`venga` of `venir`
    /// and `vengar`), empty otherwise
    pub(crate) fn verb_shares(&self, list: &[Analysis]) -> Vec<(String, f64)> {
        let verb = |analysis: &&Analysis| self.pos(analysis) == "VERB";
        let Some(first) = list.iter().find(verb) else {
            return Vec::new();
        };
        if !list
            .iter()
            .filter(verb)
            .any(|analysis| analysis.lemma != first.lemma)
        {
            return Vec::new();
        }
        let mut shares: Vec<(String, f64)> = Vec::new();
        for analysis in list.iter().filter(verb) {
            match shares
                .iter_mut()
                .find(|(lemma, _)| *lemma == analysis.lemma)
            {
                Some((_, share)) => *share += analysis.prob,
                None => shares.push((analysis.lemma.clone(), analysis.prob)),
            }
        }
        shares
    }

    /// The analysis of the frequent verb the model ranks highest when the model chose the form of a
    /// rare one: `aunque vengan` is `venir`, whose subjunctive is the present of `vengar`
    fn frequent_lemma(&self, list: &[Analysis], shares: &[(String, f64)]) -> Option<usize> {
        let top = list.first()?;
        let share = |lemma: &str| {
            shares
                .iter()
                .find(|(name, _)| name == lemma)
                .map_or(0.0, |&(_, share)| share)
        };
        if self.pos(top) != "VERB" || share(&top.lemma) >= RARE {
            return None;
        }
        list.iter().position(|analysis| {
            analysis.lemma != top.lemma
                && self.pos(analysis) == "VERB"
                && share(&analysis.lemma) > FREQUENT
        })
    }
}

/// A word of the dictionary that is mostly a verb with clitics is one in a sentence, though the
/// model, which knows few imperatives, may read it as the word of the dictionary or a name:
/// `¡Cállate!`, `—Vete, y déjame`
fn clitic_homonym(lower: &str, list: &[Analysis]) -> Option<usize> {
    if !clitics::homonym(lower) || list.first()?.source == Source::Clitics {
        return None;
    }
    list.iter()
        .position(|analysis| analysis.source == Source::Clitics)
}

/// Moves the analysis at `index` to the front; every analysis keeps the probability of the model
fn promote(list: &mut [Analysis], index: usize) {
    list[..=index].rotate_right(1);
}

#[cfg(test)]
mod tests {
    use crate::dictionary::context::tests::{dictionary, weights};

    #[test]
    fn article_before_a_finite_verb_is_a_pronoun() {
        // The model ranks the article first, the constraint the pronoun, with the probabilities
        // of the model
        let article = "C=DET|Definite=Def|Gender=Fem|Number=Sing|PronType=Art|1";
        let dictionary = dictionary(Some(weights(&[("P=PRON", 2.0), (article, 3.0)])));
        let sentence = dictionary.analyze_sentence(&["la", "vio"]).unwrap();
        let first = &sentence[0][0];
        assert_eq!(dictionary.ud_tag(first.ud).unwrap().pos(), "PRON");
        assert!(first.prob < sentence[0][1].prob);
        // Alone the article stays: no verb follows it
        let alone = dictionary.analyze_sentence(&["la", "la"]).unwrap();
        assert_eq!(dictionary.ud_tag(alone[0][0].ud).unwrap().pos(), "DET");
    }
}
