//! The fields of CoNLL-U of the words of a token and the analysis of a document of the anySeg
//! segmenter

use super::{Analysis, Dictionary, SENTENCE_CHUNK, normalized, sentence_parts};
use crate::Error;
use crate::alphabet;
use crate::clitics;

/// The fields of a word in CoNLL-U by its most probable analysis
#[doc(hidden)]
#[derive(Clone, Debug, PartialEq)]
pub struct Fields<'d> {
    pub lemma: String,
    pub pos: &'d str,
    /// `None` without features
    pub feats: Option<&'d str>,
    /// The probability of the analysis; 0 without analyses
    pub prob: f64,
}

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
        let analyses = self.analyze(token_text(token, words));
        self.token_fields_with(token, words, analyses, fields);
    }

    /// The fields of CoNLL-U of the words of the tokens of a sentence, each token - its text and
    /// its words, as `token_fields` gives them. With `context` the analyses of the tokens are
    /// ordered by the context model, without it as `analyze` orders them. `start` - whether the
    /// tokens start the sentence: a sentence given by pieces of `sentence_parts` of a multiple of
    /// `SENTENCE_CHUNK` tokens gets the fields of the whole; the analyses are kept by parts
    ///
    /// # Errors
    ///
    /// [`Error::Context`] if the analysis is in context and the dictionary has no context model
    #[doc(hidden)]
    pub fn sentence_fields<'d, S: AsRef<str>>(
        &'d self,
        tokens: &[(&str, &[S])],
        context: bool,
        start: bool,
        fields: &mut Vec<Fields<'d>>,
    ) -> Result<(), Error> {
        for (number, range) in sentence_parts(tokens.len(), SENTENCE_CHUNK).enumerate() {
            let part = &tokens[range];
            let texts: Vec<&str> = (part.iter())
                .map(|(token, words)| token_text(token, words))
                .collect();
            let analyses = if context {
                self.analyze_sentence_part(&texts, start && number == 0)?
            } else {
                texts.iter().map(|text| self.analyze(text)).collect()
            };
            for ((token, words), analyses) in part.iter().zip(analyses) {
                self.token_fields_with(token, words, analyses, fields);
            }
        }
        Ok(())
    }

    /// `token_fields` with the analyses of the token in their order: of its text for a token of
    /// several words, of its word for a token of one
    fn token_fields_with<'d, S: AsRef<str>>(
        &'d self,
        token: &str,
        words: &[S],
        analyses: Vec<Analysis>,
        fields: &mut Vec<Fields<'d>>,
    ) {
        let Some((first, rest)) = words.split_first() else {
            return;
        };
        let first = first.as_ref();
        let mut analyses = Some(analyses);
        let host = if rest.is_empty() {
            analyses
                .take()
                .and_then(|analyses| analyses.into_iter().next())
        } else {
            let fits = |analysis: &Analysis| {
                analysis.words.len() == words.len()
                    && (analysis.words.iter())
                        .zip(words)
                        .all(|(known, word)| same_word(known, word.as_ref()))
            };
            let mut fitting: Vec<Analysis> = (analyses.take().into_iter().flatten())
                .filter(fits)
                .collect();
            // A word of the dictionary that the segmenter splits is a verb with clitics too:
            // `dale` is an interjection, `dinos` an adjective
            if fitting.is_empty() {
                fitting = (self.with_clitics(&normalized(token)).into_iter())
                    .filter(fits)
                    .collect();
            }
            most_probable(fitting)
        };
        let host = host.or_else(|| self.analyze(first).into_iter().next());
        let verb = host.as_ref().is_some_and(|analysis| {
            (self.ud_tag(analysis.ud)).is_some_and(|tag| matches!(tag.pos(), "VERB" | "AUX"))
        });
        fields.push(self.fields(first, host));
        for (number, word) in rest.iter().enumerate() {
            let word = word.as_ref();
            let analyses = self.analyze(word);
            let pronoun = verb && clitics::is_clitic(&word.to_lowercase());
            // A clitic before an accusative of the third person is a dative: `se` of `díselo`
            let dative = (rest.get(number + 1)).is_some_and(|next| {
                matches!(
                    next.as_ref().to_lowercase().as_str(),
                    "lo" | "la" | "los" | "las"
                )
            });
            // An enclitic is a personal pronoun, not the `lo` of `lo que`
            let personal = |analysis: &&Analysis| {
                (self.ud_tag(analysis.ud))
                    .is_some_and(|tag| tag.pos() == "PRON" && tag.feat("Definite").is_none())
            };
            let case = |analysis: &&Analysis| {
                (self.ud_tag(analysis.ud)).is_some_and(|tag| tag.feat("Case") == Some("Dat"))
            };
            let analysis = if pronoun {
                let personal: Vec<&Analysis> = analyses.iter().filter(personal).collect();
                let datives: Vec<Analysis> = (personal.iter())
                    .filter(|analysis| dative && case(analysis))
                    .map(|&analysis| analysis.clone())
                    .collect();
                most_probable(datives)
                    .or_else(|| most_probable(personal.into_iter().cloned().collect()))
            } else {
                None
            };
            let analysis = analysis.or_else(|| analyses.into_iter().next());
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
                Fields {
                    pos: tag.pos(),
                    feats,
                    prob: analysis.prob,
                    lemma: analysis.lemma,
                }
            }
            _ => Fields {
                lemma: one_line(&word.to_lowercase()),
                pos: "X",
                feats: None,
                prob: 0.0,
            },
        }
    }

    /// The fields of every word of a document of the anySeg segmenter by sentences, as
    /// `annotate` gives them
    #[cfg(feature = "anyseg")]
    pub(crate) fn document_fields(
        &self,
        document: &anyseg::Document,
        context: bool,
    ) -> Result<Vec<Fields<'_>>, Error> {
        let mut fields = Vec::with_capacity(document.word_count());
        let spans = document.tokens();
        for number in 0..document.sentence_count() {
            let sentence = document.sentence(number);
            let words: Vec<Vec<&str>> = (sentence.tokens.clone())
                .map(|token| {
                    (document.token_words(token))
                        .map(|word| document.form(word))
                        .collect()
                })
                .collect();
            let tokens: Vec<(&str, &[&str])> = (sentence.tokens.clone())
                .zip(&words)
                .map(|(token, words)| (spans[token].slice(document.text()), words.as_slice()))
                .collect();
            self.sentence_fields(&tokens, context, true, &mut fields)?;
        }
        Ok(fields)
    }

    /// Fills the columns LEMMA, UPOS and FEATS of the words of a document of the anySeg segmenter
    /// (the crate is `esmorph::anyseg`) with the most probable analysis of every word in the
    /// context of its sentence, as [`Dictionary::analyze_sentence`] orders the analyses of its
    /// tokens. The words of a token of several are analyzed by the token: `dime` split into `di` + `me` is the imperative of `decir` with the pronoun `me`, and
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
    /// [`Error::Context`] if the dictionary has no context model, [`Error::Document`] if the
    /// document does not take the columns
    #[cfg(feature = "anyseg")]
    #[cfg_attr(docsrs, doc(cfg(feature = "anyseg")))]
    pub fn annotate(&self, document: &mut anyseg::Document) -> Result<(), Error> {
        let fields = self.document_fields(document, true)?;
        set_fields(document, &fields)
    }

    /// [`Dictionary::annotate`] without the context model: the analyses of a token are ordered
    /// as [`Dictionary::analyze`] orders them
    ///
    /// # Errors
    ///
    /// [`Error::Document`] if the document does not take the columns
    #[cfg(feature = "anyseg")]
    #[cfg_attr(docsrs, doc(cfg(feature = "anyseg")))]
    pub fn annotate_without_context(&self, document: &mut anyseg::Document) -> Result<(), Error> {
        let fields = self.document_fields(document, false)?;
        set_fields(document, &fields)
    }
}

/// The columns LEMMA, UPOS and FEATS of the words of a document from their fields
#[cfg(feature = "anyseg")]
pub(crate) fn set_fields(
    document: &mut anyseg::Document,
    fields: &[Fields],
) -> Result<(), crate::Error> {
    use anyseg::Column;

    // The values go to the document as they are read: no column is copied first
    let values = |column| {
        fields.iter().map(move |word| match column {
            Column::Lemma => Some(word.lemma.as_str()),
            Column::Upos => Some(word.pos),
            _ => word.feats,
        })
    };
    let columns =
        [Column::Lemma, Column::Upos, Column::Feats].map(|column| (column, values(column)));
    document
        .set_columns(columns, None)
        .map_err(|err| crate::Error::Document(err.to_string()))
}

/// The text a token is analyzed by: the token of several words, the word of a token of one
fn token_text<'a, S: AsRef<str>>(token: &'a str, words: &'a [S]) -> &'a str {
    match words {
        [word] => word.as_ref(),
        _ => token,
    }
}

/// The first of the analyses that a choice is limited to, with its share among them
fn most_probable(analyses: Vec<Analysis>) -> Option<Analysis> {
    let total: f64 = analyses.iter().map(|analysis| analysis.prob).sum();
    let mut first = analyses.into_iter().next()?;
    if total > 0.0 {
        first.prob /= total;
    }
    Some(first)
}

/// A word as one line of CoNLL-U, as anySeg writes a form there: a tab or a line break (CR LF
/// together) is a space
pub(crate) fn one_line(word: &str) -> String {
    const BREAKS: [char; 8] = [
        '\t', '\n', '\r', '\u{b}', '\u{c}', '\u{85}', '\u{2028}', '\u{2029}',
    ];
    if word.contains(BREAKS) {
        word.replace("\r\n", " ").replace(BREAKS, " ")
    } else {
        word.to_owned()
    }
}

/// Whether two spellings are one word: case, accents, combining marks and invisible characters do
/// not matter (the segmenter gives the host of `dámelo` as `da`, the dictionary as `da` too, but
/// `convirtiéndose` as `convirtiendo`)
fn same_word(a: &str, b: &str) -> bool {
    let letters = |word: &str| {
        let word = normalized(word).to_lowercase();
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
            .map(|word| {
                (
                    word.lemma,
                    word.pos.to_owned(),
                    word.feats.map(str::to_owned),
                )
            })
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
        // An enclitic `lo` is a personal pronoun, not the `lo` of `lo que`
        let feats = |token, words: &[&str]| fields(token, words).pop().unwrap().2.unwrap();
        assert_eq!(
            feats("verlo", &["ver", "lo"]),
            "Case=Acc|Gender=Masc|Number=Sing|Person=3|PrepCase=Npr|PronType=Prs"
        );
        // A clitic before an accusative of the third person is a dative
        let words = fields("díselo", &["di", "se", "lo"]);
        assert!(
            words[1].2.as_deref().unwrap().contains("Case=Dat"),
            "{words:?}"
        );
        assert!(
            !words[2].2.as_deref().unwrap().contains("Definite"),
            "{words:?}"
        );
    }

    #[test]
    fn words_of_the_dictionary_split_by_the_segmenter() {
        // `dale` is an interjection and `dinos` an adjective of the dictionary, but split into
        // words they are a verb with clitics
        let host = |token, words: &[&str]| fields(token, words).swap_remove(0);
        let dale = host("dale", &["da", "le"]);
        assert_eq!((dale.0.as_str(), dale.1.as_str()), ("dar", "VERB"));
        assert!(dale.2.unwrap().contains("Mood=Imp"));
        assert_eq!(host("dinos", &["di", "nos"]).0, "decir");
        assert_eq!(host("Cállate", &["Calla", "te"]).0, "callar");
    }

    #[test]
    fn words_in_other_unicode_forms() {
        // Combining marks and invisible characters in the words of the document
        let decomposed = "di\u{301}";
        assert_eq!(
            summary("di\u{301}melo", &[decomposed, "me", "lo"])[0],
            ("decir".to_owned(), "VERB".to_owned())
        );
        assert!(same_word("dí\u{200b}", "di"));
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

    #[test]
    fn sentences_in_and_without_context() {
        let dictionary = Dictionary::bundled().unwrap();
        let pos = |tokens: &[&str], context: bool| {
            let tokens: Vec<(&str, [&str; 1])> =
                (tokens.iter()).map(|&token| (token, [token])).collect();
            let tokens: Vec<(&str, &[&str])> = (tokens.iter())
                .map(|(token, words)| (*token, words.as_slice()))
                .collect();
            let mut fields = Vec::new();
            (dictionary.sentence_fields(&tokens, context, true, &mut fields)).unwrap();
            fields.into_iter().map(|word| word.pos).collect::<Vec<_>>()
        };
        assert_eq!(pos(&["Ella", "la", "vio"], true), ["PRON", "PRON", "VERB"]);
        assert_eq!(pos(&["Ella", "la", "vio"], false), ["PRON", "DET", "VERB"]);
        // A word without neighbors is analyzed as `analyze` in context too
        assert_eq!(pos(&["Casa"], true), ["NOUN"]);
    }

    #[cfg(feature = "anyseg")]
    #[test]
    fn annotates_document() {
        use anyseg::{Column, Segmenter};

        let dictionary = Dictionary::bundled().unwrap();
        let mut document = Segmenter::new("es")
            .unwrap()
            .segment("Devuélvenos, dime la verdad del caso.\n\nDámelo.");
        dictionary.annotate_without_context(&mut document).unwrap();
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
        dictionary.annotate_without_context(&mut document).unwrap();
        assert_eq!(document.column(Column::Lemma, 0), Some("devolver"));
        // A token with a tab or a line break inside
        let text = "a\tb x\r\ny z\u{2028}w".to_owned();
        let tokens = vec![
            anyseg::Span { start: 0, end: 3 },
            anyseg::Span { start: 4, end: 8 },
            anyseg::Span { start: 9, end: 14 },
        ];
        let mut spaced = anyseg::Document::from_tokens(text, tokens, None).unwrap();
        dictionary.annotate(&mut spaced).unwrap();
        let lemmas: Vec<_> = (0..3)
            .map(|word| spaced.column(Column::Lemma, word))
            .collect();
        assert_eq!(lemmas, [Some("a b"), Some("x y"), Some("z w")]);
        // The lemma is the form that anySeg writes in CoNLL-U
        let conllu = spaced.to_conllu();
        assert!(conllu.contains("\tx y\tx y\t"), "{conllu}");
        let mut empty = Segmenter::new("es").unwrap().segment("");
        dictionary.annotate(&mut empty).unwrap();
        assert_eq!(empty.word_count(), 0);
    }

    #[cfg(feature = "anyseg")]
    #[test]
    fn annotates_document_in_context() {
        use anyseg::{Column, Segmenter};

        let dictionary = Dictionary::bundled().unwrap();
        let mut document = Segmenter::new("es")
            .unwrap()
            .segment("Ella la vio. Devuélvenos el libro.");
        dictionary.annotate(&mut document).unwrap();
        let columns: Vec<(&str, &str, &str)> = (0..document.word_count())
            .map(|word| {
                (
                    document.form(word),
                    document.column(Column::Lemma, word).unwrap(),
                    document.column(Column::Upos, word).unwrap(),
                )
            })
            .collect();
        // The pronoun after a subject; the verb with clitics the segmenter leaves one word
        assert_eq!(columns[1], ("la", "él", "PRON"));
        assert_eq!(columns[4], ("Devuélvenos", "devolver", "VERB"));
        dictionary.annotate_without_context(&mut document).unwrap();
        assert_eq!(document.column(Column::Upos, 1), Some("DET"));
    }
}
