//! Constraints of Spanish on the analyses the context model chooses in a sentence

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
/// Parts of speech of a word that heads the goal of `ir a`: `fue a casa`, `fue a ver`; a pronoun
/// after `fue a` is mostly a cleft (`fue a ella a quien`)
const GOAL: [&str; 5] = ["DET", "NOUN", "PROPN", "NUM", "VERB"];
/// Words of the prepositional phrases `a ... de`, which are no goal: `fue a partir de 1990`, `fue
/// a finales del siglo`
const LOCUTIONS: [&str; 18] = [
    "base",
    "cambio",
    "cargo",
    "causa",
    "consecuencia",
    "costa",
    "excepción",
    "falta",
    "favor",
    "finales",
    "fuerza",
    "mediados",
    "partir",
    "pesar",
    "petición",
    "principios",
    "raíz",
    "través",
];
/// Adverbial phrases `al ...`, which are no goal of `ir`: `fue al principio difícil`
const AL_PHRASES: [&str; 12] = [
    "cabo",
    "contrario",
    "fin",
    "final",
    "instante",
    "menos",
    "mismo",
    "parecer",
    "principio",
    "propio",
    "punto",
    "revés",
];
/// Words a verb follows, not a noun without a determiner: the negation and the subject `tú`
const BEFORE_VERB: [&str; 2] = ["no", "tú"];
/// Lemmas of the verbs followed by a bare infinitive: `puede ser`, `suele parecer`, but `tiene
/// poder`, `siento pesar`
const BEFORE_INFINITIVE: [&str; 16] = [
    "conseguir",
    "deber",
    "decidir",
    "desear",
    "esperar",
    "intentar",
    "lograr",
    "necesitar",
    "parecer",
    "pensar",
    "poder",
    "preferir",
    "pretender",
    "querer",
    "saber",
    "soler",
];
/// Shares of the verbs of a form in the counts of the dictionary: the model, which sees no lemma,
/// does not choose a verb below RARE where another is above FREQUENT
const RARE: f64 = 0.2;
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
                .or_else(|| self.opening_word(words, sentence, at, start));
            if let Some(index) = chosen {
                promote(&mut sentence[at], index);
            }
            // Also of the verb a rule chose: `no temas` is `temer`, not `temar`
            if let Some(index) = self.frequent_lemma(&sentence[at], &shares[at]) {
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

    /// A word the model reads as a common noun right after a verb of BEFORE_INFINITIVE is its
    /// infinitive: `puede ser`, `suele parecer`
    fn infinitive(&self, sentence: &[Vec<Analysis>], at: usize) -> Option<usize> {
        let list = &sentence[at];
        let before = at
            .checked_sub(1)
            .and_then(|before| sentence[before].first())?;
        if self.pos(list.first()?) != "NOUN"
            || !matches!(self.pos(before), "VERB" | "AUX")
            || !BEFORE_INFINITIVE.contains(&before.lemma.as_str())
        {
            return None;
        }
        list.iter().position(|analysis| {
            matches!(self.pos(analysis), "VERB" | "AUX") && self.has(analysis, "VerbForm", "Inf")
        })
    }

    /// A form of `ser` and `ir` is `ir` after a reflexive clitic but before a predicate (`se fue`,
    /// `me fui de casa`, not `me fue muy útil`), before `a` and its goal (`fue a casa`, `fueron a
    /// ver`, not `fue a partir de`, `fue al principio`, `fue a las cinco`) and before a gerund
    /// (`fue cayendo`)
    fn ser_ir(&self, lower: &[String], sentence: &[Vec<Analysis>], at: usize) -> Option<usize> {
        let list = &sentence[at];
        if !SER_IR.contains(&lower[at].as_str()) || list.first()?.lemma == "ir" {
            return None;
        }
        let word = |at: usize| lower.get(at).map_or("", String::as_str);
        let next = word(at + 1);
        // The predicate after the adverbs before it: `me fue muy difícil`
        let predicate = (at + 1..sentence.len())
            .find(|&next| self.top_pos(sentence, next) != "ADV")
            .unwrap_or(sentence.len());
        let after_clitic = at > 0
            && PROCLITICS.contains(&lower[at - 1].as_str())
            && next != "de"
            && self.top_pos(sentence, predicate) != "ADJ"
            && !self.top_verb_form(sentence, predicate, "Part");
        let locution = (LOCUTIONS.contains(&word(at + 2)) && matches!(word(at + 3), "de" | "del"))
            || (next == "al" && AL_PHRASES.contains(&word(at + 2)))
            || (matches!(next, "a" | "á") && word(at + 2) == "la" && word(at + 3) == "vez");
        let hour = matches!(word(at + 2), "la" | "las") && self.top_pos(sentence, at + 3) == "NUM";
        // `á` is the preposition in older writing: `fué á ocultarse`
        let goal = matches!(next, "a" | "á" | "al")
            && !locution
            && !hour
            && (next == "al" || GOAL.contains(&self.top_pos(sentence, at + 2)));
        let gerund = self.top_verb_form(sentence, at + 1, "Ger");
        if !(after_clitic || goal || gerund) {
            return None;
        }
        list.iter().position(|analysis| analysis.lemma == "ir")
    }

    /// A word the model reads as a noun after a reflexive or a dative clitic, the negation or
    /// `tú` is a finite verb: `que te vayas`, `no vayas`, `adonde tú vayas`; so is one it reads as
    /// an adverb or an adjective after such a clitic: `se acerca`, `te quedas`
    fn verb_after(&self, lower: &[String], sentence: &[Vec<Analysis>], at: usize) -> Option<usize> {
        let list = &sentence[at];
        let before = at.checked_sub(1)?;
        let clitic = PROCLITICS.contains(&lower[before].as_str())
            && self.top_pos(sentence, before) == "PRON";
        // `no` after a preposition or a determiner makes a noun negative: `punto de no retorno`
        let negation = BEFORE_VERB.contains(&lower[before].as_str())
            && !(before > 0 && matches!(self.top_pos(sentence, before - 1), "ADP" | "DET"));
        let read = self.pos(list.first()?);
        let noun = matches!(read, "NOUN" | "PROPN");
        if !(((clitic || negation) && noun) || (clitic && matches!(read, "ADV" | "ADJ"))) {
            return None;
        }
        list.iter().position(|analysis| {
            matches!(self.pos(analysis), "VERB" | "AUX") && self.has(analysis, "VerbForm", "Fin")
        })
    }

    /// A capitalized word opening a sentence that a rule reads as a name is its imperative when the
    /// dictionary has one, the model ranks it above the other words of the dictionary and a
    /// lowercase word of AFTER_IMPERATIVE follows: `Indique si desea`, not `Tony`, `Defensa quiere`
    /// or `Sangre de Abel`
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
        // A noun the model ranks above the verb is no imperative: `Sangre de Abel`
        let known = list
            .iter()
            .find(|analysis| analysis.source == Source::Dictionary)?;
        if self.pos(known) != "VERB" {
            return None;
        }
        list.iter().position(|analysis| {
            analysis.source == Source::Dictionary
                && self.pos(analysis) == "VERB"
                && self.has(analysis, "Mood", "Imp")
        })
    }

    /// Shares of the verbs of a word in the counts, among its verbs, if it is the form of several
    /// (`venga` of `venir` and `vengar`), empty otherwise
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
        // Among the verbs alone: `temas` is mostly the noun, but of the verbs `temer`
        let total: f64 = shares.iter().map(|(_, share)| share).sum();
        if total > 0.0 {
            for (_, share) in &mut shares {
                *share /= total;
            }
        }
        shares
    }

    /// The analysis of the frequent verb the model ranks highest when the model chose a finite form
    /// of a rare one: `aunque vengan` is `venir`, whose subjunctive is the present of `vengar`; a
    /// form of the same kind, so `me visto` is no participle of `ver`
    fn frequent_lemma(&self, list: &[Analysis], shares: &[(String, f64)]) -> Option<usize> {
        let top = list.first()?;
        let share = |lemma: &str| {
            shares
                .iter()
                .find(|(name, _)| name == lemma)
                .map_or(0.0, |&(_, share)| share)
        };
        // A participle stands: the model reads it from its context (`había entrevisto`)
        if self.pos(top) != "VERB" || self.has(top, "VerbForm", "Part") || share(&top.lemma) >= RARE
        {
            return None;
        }
        let form = |analysis: &Analysis| {
            (self.ud_tag(analysis.ud))
                .and_then(|tag| tag.feats().find(|&(name, _)| name == "VerbForm"))
                .map(|(_, value)| value)
        };
        list.iter().position(|analysis| {
            analysis.lemma != top.lemma
                && self.pos(analysis) == "VERB"
                && form(analysis) == form(top)
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
