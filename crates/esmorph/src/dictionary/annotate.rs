//! The fields of CoNLL-U of the words of a token and the analysis of a document of the anySeg
//! segmenter

use super::{Analysis, Dictionary};
use crate::alphabet;
use crate::clitics;

/// The fields of a word in CoNLL-U: the lemma, the part of speech and the features, `None` without
/// them
#[doc(hidden)]
pub type Fields<'d> = (String, &'d str, Option<&'d str>);

impl Dictionary {
    /// The fields of CoNLL-U of the most probable analysis of every word of a token, pushed to
    /// `fields`; a word without analyses is in lower case with the part of speech X. The words of
    /// a token of several are analyzed by the token: the first one by the analysis of the token
    /// that has these words (`dime` is the imperative of `decir`, though `di` alone is mostly the
    /// past of `dar`), and clitics after a verb are pronouns (`la` alone is mostly an article)
    #[doc(hidden)]
    pub fn token_fields<'d, S: AsRef<str>>(
        &'d self,
        token: &str,
        words: &[S],
        fields: &mut Vec<Fields<'d>>,
    ) {
        let Some((first, rest)) = words.split_first() else {
            return;
        };
        let first = first.as_ref();
        let host = if rest.is_empty() {
            None
        } else {
            (self.analyze(token).into_iter()).find(|analysis| {
                analysis.words.len() == words.len()
                    && (analysis.words.iter())
                        .zip(words)
                        .all(|(known, word)| same_word(known, word.as_ref()))
            })
        };
        let host = host.or_else(|| self.analyze(first).into_iter().next());
        let verb = host.as_ref().is_some_and(|analysis| {
            (self.ud_tag(analysis.ud)).is_some_and(|tag| matches!(tag.pos(), "VERB" | "AUX"))
        });
        fields.push(self.fields(first, host));
        for word in rest {
            let word = word.as_ref();
            let analyses = self.analyze(word);
            let pronoun = verb && clitics::is_clitic(&word.to_lowercase());
            let analysis = match analyses.iter().position(|analysis| {
                pronoun && (self.ud_tag(analysis.ud)).is_some_and(|tag| tag.pos() == "PRON")
            }) {
                Some(at) => analyses.into_iter().nth(at),
                None => analyses.into_iter().next(),
            };
            fields.push(self.fields(word, analysis));
        }
    }

    /// The fields of CoNLL-U of an analysis of a word
    fn fields(&self, word: &str, analysis: Option<Analysis>) -> Fields<'_> {
        let tag = analysis
            .as_ref()
            .and_then(|analysis| self.ud_tag(analysis.ud));
        match (analysis, tag) {
            (Some(analysis), Some(tag)) => {
                let feats = self.feats.get_or_init(|| {
                    (self.ud_tags())
                        .map(|tag| (tag.feats().len() > 0).then(|| tag.feats_string().into()))
                        .collect()
                });
                let feats = feats
                    .get(usize::from(analysis.ud))
                    .and_then(Option::as_deref);
                (analysis.lemma, tag.pos(), feats)
            }
            _ => (word.to_lowercase(), "X", None),
        }
    }

    /// Fills the columns LEMMA, UPOS and FEATS of the words of a document of the anySeg segmenter
    /// (the crate is `esmorph::anyseg`) with the most probable analysis of every word, as
    /// [`Dictionary::analyze`] orders them. The words of a token of several are analyzed by the
    /// token: `dime` split into `di` + `me` is the imperative of `decir` with the pronoun `me`, and
    /// clitics after a verb are pronouns. A token that the segmenter leaves as one word gets the
    /// analysis of the whole token: `devuélvenos` is the imperative of `devolver`. A word without
    /// analyses gets its lower case as the lemma and the part of speech X; the earlier values of
    /// the columns are replaced
    ///
    /// ```
    /// # #[cfg(feature = "bundled")] {
    /// use esmorph::anyseg::{Column, Segmenter};
    ///
    /// let dictionary = esmorph::Dictionary::bundled()?;
    /// let mut document = Segmenter::new("es").unwrap().segment("Dime la verdad.");
    /// dictionary.annotate(&mut document)?;
    /// assert_eq!(document.column(Column::Lemma, 0), Some("decir"));
    /// assert_eq!(document.column(Column::Upos, 1), Some("PRON"));
    /// # }
    /// # Ok::<(), esmorph::Error>(())
    /// ```
    ///
    /// # Errors
    ///
    /// [`crate::Error::Document`] if the document does not take the columns
    #[cfg(feature = "anyseg")]
    #[cfg_attr(docsrs, doc(cfg(feature = "anyseg")))]
    pub fn annotate(&self, document: &mut anyseg::Document) -> Result<(), crate::Error> {
        use anyseg::Column;

        let mut fields = Vec::with_capacity(document.word_count());
        let mut words = Vec::new();
        for (token, span) in document.tokens().iter().enumerate() {
            words.clear();
            words.extend(document.token_words(token).map(|word| document.form(word)));
            self.token_fields(span.slice(document.text()), &words, &mut fields);
        }
        let lemmas: Vec<Option<&str>> = (fields.iter())
            .map(|(lemma, _, _)| Some(lemma.as_str()))
            .collect();
        let parts: Vec<Option<&str>> = fields.iter().map(|&(_, pos, _)| Some(pos)).collect();
        let feats: Vec<Option<&str>> = fields.iter().map(|&(_, _, feats)| feats).collect();
        let columns = [
            (Column::Lemma, lemmas),
            (Column::Upos, parts),
            (Column::Feats, feats),
        ];
        document
            .set_columns(columns, None)
            .map_err(|err| crate::Error::Document(err.to_string()))
    }
}

/// Whether two spellings are one word: case and accents do not matter (the segmenter gives the host
/// of `dámelo` as `da`, the dictionary as `da` too, but `convirtiéndose` as `convirtiendo`)
fn same_word(a: &str, b: &str) -> bool {
    let letters = |word: &str| {
        let word = word.to_lowercase();
        word.chars()
            .map(|c| alphabet::plain(c).unwrap_or(c))
            .collect::<String>()
    };
    letters(a) == letters(b)
}

#[cfg(all(test, feature = "bundled"))]
mod tests {
    use super::*;

    fn fields(token: &str, words: &[&str]) -> Vec<(String, String, Option<String>)> {
        let dictionary = Dictionary::bundled().unwrap();
        let mut fields = Vec::new();
        dictionary.token_fields(token, words, &mut fields);
        (fields.into_iter())
            .map(|(lemma, pos, feats)| (lemma, pos.to_owned(), feats.map(str::to_owned)))
            .collect()
    }

    fn summary(token: &str, words: &[&str]) -> Vec<(String, String)> {
        (fields(token, words).into_iter())
            .map(|(lemma, pos, _)| (lemma, pos))
            .collect()
    }

    fn pairs(expected: &[(&str, &str)]) -> Vec<(String, String)> {
        (expected.iter())
            .map(|&(lemma, pos)| (lemma.to_owned(), pos.to_owned()))
            .collect()
    }

    #[test]
    fn host_by_token() {
        // `di` alone is mostly the past of `dar`
        assert_eq!(summary("di", &["di"]), pairs(&[("dar", "VERB")]));
        assert_eq!(
            summary("dime", &["di", "me"]),
            pairs(&[("decir", "VERB"), ("yo", "PRON")])
        );
        // A capital letter makes the token a name first, but its words are a verb and a clitic
        assert_eq!(
            summary("Dime", &["Di", "me"]),
            pairs(&[("decir", "VERB"), ("yo", "PRON")])
        );
        // The host without the accent of the clitics
        assert_eq!(
            summary("convirtiéndose", &["convirtiendo", "se"]),
            pairs(&[("convertir", "VERB"), ("él", "PRON")])
        );
    }

    #[test]
    fn clitics_are_pronouns() {
        // `las` alone is mostly an article
        assert_eq!(summary("las", &["las"]), pairs(&[("el", "DET")]));
        assert_eq!(
            summary("verlas", &["ver", "las"]),
            pairs(&[("ver", "VERB"), ("él", "PRON")])
        );
        let words = fields("dárselas", &["dar", "se", "las"]);
        assert_eq!(words[2].1, "PRON");
        assert!(words[2].2.as_deref().unwrap().contains("Case=Acc"));
    }

    #[test]
    fn contractions() {
        assert_eq!(
            fields("del", &["de", "el"]),
            vec![
                ("de".to_owned(), "ADP".to_owned(), None),
                (
                    "el".to_owned(),
                    "DET".to_owned(),
                    Some("Definite=Def|Gender=Masc|Number=Sing|PronType=Art".to_owned())
                ),
            ]
        );
    }

    #[test]
    fn one_word() {
        // A verb with clitics that stays one word is the verb
        assert_eq!(summary("dámelo", &["dámelo"]), pairs(&[("dar", "VERB")]));
        assert_eq!(
            fields("qzxw", &["qzxw"]),
            vec![("qzxw".to_owned(), "X".to_owned(), None)]
        );
        assert!(fields("", &[] as &[&str]).is_empty());
    }

    #[test]
    fn words_that_the_token_has_not() {
        // The words of the document go first: without a fitting analysis of the token every word
        // is analyzed alone, and clitics after a verb are pronouns
        assert_eq!(
            summary("xyz", &["ver", "la"]),
            pairs(&[("ver", "VERB"), ("él", "PRON")])
        );
        assert_eq!(
            summary("xyz", &["de", "la"]),
            pairs(&[("de", "ADP"), ("el", "DET")])
        );
    }

    #[cfg(feature = "anyseg")]
    #[test]
    fn annotates_document() {
        use anyseg::{Column, Segmenter};

        let dictionary = Dictionary::bundled().unwrap();
        let mut document = Segmenter::new("es")
            .unwrap()
            .segment("Devuélvenos, dime la verdad del caso.\n\nDámelo.");
        dictionary.annotate(&mut document).unwrap();
        let columns: Vec<(&str, &str, &str, Option<&str>)> = (0..document.word_count())
            .map(|word| {
                (
                    document.form(word),
                    document.column(Column::Lemma, word).unwrap(),
                    document.column(Column::Upos, word).unwrap(),
                    document.column(Column::Feats, word),
                )
            })
            .collect();
        let expected = [
            // The segmenter leaves the token one word: it is the verb
            ("Devuélvenos", "devolver", "VERB"),
            (",", ",", "PUNCT"),
            ("di", "decir", "VERB"),
            ("me", "yo", "PRON"),
            ("la", "el", "DET"),
            ("verdad", "verdad", "NOUN"),
            ("de", "de", "ADP"),
            ("el", "el", "DET"),
            ("caso", "caso", "NOUN"),
            (".", ".", "PUNCT"),
            ("Da", "dar", "VERB"),
            ("me", "yo", "PRON"),
            ("lo", "él", "PRON"),
            (".", ".", "PUNCT"),
        ];
        assert_eq!(
            (columns.iter())
                .map(|&(form, lemma, pos, _)| (form, lemma, pos))
                .collect::<Vec<_>>(),
            expected
        );
        assert_eq!(columns[6].3, None);
        // The analyses replace the earlier values
        dictionary.annotate(&mut document).unwrap();
        assert_eq!(document.column(Column::Lemma, 0), Some("devolver"));
        let mut empty = Segmenter::new("es").unwrap().segment("");
        dictionary.annotate(&mut empty).unwrap();
        assert_eq!(empty.word_count(), 0);
    }
}
