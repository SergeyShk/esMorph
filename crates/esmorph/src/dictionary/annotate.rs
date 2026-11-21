//! The fields of CoNLL-U of the words of a token and the analysis of a document of the anySeg
//! segmenter

use std::ops::Range;

use super::{Analysis, Dictionary, SENTENCE_CHUNK, Source, normalized, sentence_parts};
use crate::Error;
use crate::alphabet;
use crate::clitics;

/// Parts of sentences that `sentence_fields` analyzes at a time
pub(crate) const PART_GROUP: usize = 32;

/// Tokens of one word that take the words of their analysis, by their indices, and the words
#[cfg(feature = "anyseg")]
type TokenWords<'d> = Vec<Split<'d>>;

/// A token of one word that takes the words of its analysis: its number, the words and the fields
/// of the whole token, which a document that keeps its words gets
#[doc(hidden)]
#[derive(Clone, Debug, PartialEq)]
pub struct Split<'d> {
    pub token: usize,
    pub words: Vec<String>,
    pub whole: Fields<'d>,
}

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
        self.token_fields_with(token, words, analyses, fields, false);
    }

    /// The fields of CoNLL-U of the words of the tokens of sentences, each token - its text and
    /// its words, as `token_fields` gives them; a sentence is a range of `tokens` and whether the
    /// range starts the sentence: a sentence given by pieces of `sentence_parts` of a multiple of
    /// `SENTENCE_CHUNK` tokens gets the fields of the whole. With `context` the analyses of the
    /// tokens are ordered by the context model, without it as `analyze` orders them. With `words`
    /// a token of one word whose analysis of a verb with clitics or a contraction has several
    /// words gets their fields, and the number of the token among `tokens`, the words and the
    /// fields of the whole go to `words`: `dámelo` is `da` + `me` + `lo`; a token for which `keep`
    /// gives true keeps its word
    ///
    /// # Errors
    ///
    /// [`Error::Context`] if the analysis is in context and the dictionary has no context model
    #[doc(hidden)]
    pub fn sentence_fields<'d, S: AsRef<str>>(
        &'d self,
        tokens: &[(&str, &[S])],
        sentences: &[(Range<usize>, bool)],
        context: bool,
        fields: &mut Vec<Fields<'d>>,
        mut words: Option<&mut Vec<Split<'d>>>,
        keep: &dyn Fn(usize) -> bool,
    ) -> Result<(), Error> {
        // The parts the model analyzes, a group at a time: the model takes four of a group with
        // close lengths together, and the analyses of a group are few; without the model a part
        // at a time keeps them in the cache
        let parts: Vec<(Range<usize>, bool)> = (sentences.iter())
            .flat_map(|(range, start)| {
                (sentence_parts(range.len(), SENTENCE_CHUNK).enumerate()).map(|(number, part)| {
                    let at = range.start + part.start..range.start + part.end;
                    (at, *start && number == 0)
                })
            })
            .collect();
        for group in parts.chunks(if context { PART_GROUP } else { 1 }) {
            let texts: Vec<Vec<&str>> = (group.iter())
                .map(|(range, _)| {
                    (tokens[range.clone()].iter())
                        .map(|(token, words)| token_text(token, words))
                        .collect()
                })
                .collect();
            let analyses = if context {
                let group: Vec<(&[&str], bool)> = (texts.iter().zip(group))
                    .map(|(texts, (_, start))| (texts.as_slice(), *start))
                    .collect();
                self.analyze_sentence_parts(&group)?
            } else {
                (texts.iter())
                    .map(|texts| texts.iter().map(|text| self.analyze(text)).collect())
                    .collect()
            };
            for ((range, _), analyses) in group.iter().zip(analyses) {
                for (at, analyses) in range.clone().zip(analyses) {
                    let (token, forms) = &tokens[at];
                    let split = words.is_some() && !keep(at);
                    if let Some((parts, whole)) =
                        self.token_fields_with(token, forms, analyses, fields, split)
                        && let Some(words) = words.as_deref_mut()
                    {
                        words.push(Split {
                            token: at,
                            words: parts,
                            whole,
                        });
                    }
                }
            }
        }
        Ok(())
    }

    /// The analysis of the first word of a token with the words `words` from `analyses` of the
    /// token in their order: of a token of one word the first, of a token of several the first
    /// with these words, ignoring case and accents, else the first such analysis of the token as
    /// a verb with clitics, its probability the share among them; without one the first analysis
    /// of the first word
    fn host<S: AsRef<str>>(
        &self,
        token: &str,
        words: &[S],
        analyses: Vec<Analysis>,
    ) -> Option<Analysis> {
        let first = words.first()?.as_ref();
        let host = if words.len() == 1 {
            analyses.into_iter().next()
        } else {
            let fits = |analysis: &Analysis| {
                analysis.words.len() == words.len()
                    && (analysis.words.iter())
                        .zip(words)
                        .all(|(known, word)| same_word(known, word.as_ref()))
            };
            let mut fitting: Vec<Analysis> = analyses.into_iter().filter(fits).collect();
            // A word of the dictionary that the segmenter splits is a verb with clitics too:
            // `dale` is an interjection, `dinos` an adjective
            if fitting.is_empty() {
                fitting = (self.with_clitics(&normalized(token)).into_iter())
                    .filter(fits)
                    .collect();
            }
            most_probable(fitting)
        };
        host.or_else(|| self.analyze(first).into_iter().next())
    }

    /// `token_fields` with the analyses of the token in their order: of its text for a token of
    /// several words, of its word for a token of one. With `split` a token of one word whose
    /// first analysis has several words gets the fields of these words, in the case of the token,
    /// and they are returned with the fields of the whole token; the words of a number with a
    /// unit stay one, as the treebanks have them (`48km`)
    fn token_fields_with<'d, S: AsRef<str>>(
        &'d self,
        token: &str,
        words: &[S],
        analyses: Vec<Analysis>,
        fields: &mut Vec<Fields<'d>>,
        split: bool,
    ) -> Option<(Vec<String>, Fields<'d>)> {
        if split
            && let [word] = words
            && let Some(parts) = (analyses.first())
                .filter(|analysis| analysis.words.len() > 1 && analysis.source != Source::Rule)
                .map(|analysis| cased(&analysis.words, token))
        {
            let whole = self.fields(word.as_ref(), analyses.first().cloned());
            self.token_fields_with(token, &parts, analyses, fields, false);
            return Some((parts, whole));
        }
        let (first, rest) = words.split_first()?;
        let first = first.as_ref();
        let host = self.host(token, words, analyses);
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
        None
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
    /// `annotate` gives them; with `split` also the tokens of one word whose analysis has several,
    /// by their indices in the document, and the words
    #[cfg(feature = "anyseg")]
    fn document_fields(
        &self,
        document: &anyseg::Document,
        context: bool,
        split: bool,
    ) -> Result<(Vec<Fields<'_>>, TokenWords<'_>), Error> {
        let mut fields = Vec::with_capacity(document.word_count());
        let mut words = Vec::new();
        let spans = document.tokens();
        let mut number = 0;
        while number < document.sentence_count() {
            // Sentences of about `PART_GROUP` parts at a time: the tokens and analyses of a
            // document are not kept whole
            let first = document.sentence(number).tokens.start;
            let mut end = first;
            let mut sentences = Vec::new();
            while number < document.sentence_count() && sentences.len() < PART_GROUP {
                let tokens = document.sentence(number).tokens;
                sentences.push((tokens.start - first..tokens.end - first, true));
                end = tokens.end;
                number += 1;
            }
            let forms: Vec<Vec<&str>> = (first..end)
                .map(|token| {
                    (document.token_words(token))
                        .map(|word| document.form(word))
                        .collect()
                })
                .collect();
            let tokens: Vec<(&str, &[&str])> = (first..end)
                .zip(&forms)
                .map(|(token, forms)| (spans[token].slice(document.text()), forms.as_slice()))
                .collect();
            let known = words.len();
            let keep = |at: usize| keeps_columns(document, first + at);
            self.sentence_fields(
                &tokens,
                &sentences,
                context,
                &mut fields,
                split.then_some(&mut words),
                &keep,
            )?;
            for split in &mut words[known..] {
                split.token += first;
            }
        }
        Ok((fields, words))
    }

    /// The fields of every word of a document, as `annotate` gives them, and the document with
    /// the words of its tokens of one word whose analysis has several, if there are such. A
    /// document with heads or enhanced dependencies keeps its words, as does a token whose word
    /// has XPOS or MISC other than the spaces around it, which other words would lose (`keeps_columns`);
    /// a token that keeps its word gets the analysis of the whole
    #[cfg(feature = "anyseg")]
    pub(crate) fn document_analysis(
        &self,
        document: &anyseg::Document,
        context: bool,
    ) -> Result<(Option<anyseg::Document>, Vec<Fields<'_>>), Error> {
        let (fields, words) = self.document_fields(document, context, !fixed_ids(document))?;
        if words.is_empty() {
            return Ok((None, fields));
        }
        let parts = (words.iter()).map(|split| (split.token, split.words.clone()));
        match document.clone().with_words(parts) {
            Ok(other) => Ok((Some(other), fields)),
            // A document with empty nodes keeps its words too
            Err(_) => Ok((None, whole_fields(document, fields, words))),
        }
    }

    /// Fills the columns LEMMA, UPOS and FEATS of the words of a document of the anySeg segmenter
    /// (the crate is `esmorph::anyseg`) with the most probable analysis of every word in the
    /// context of its sentence, as [`Dictionary::analyze_sentence`] orders the analyses of its
    /// tokens. A token that the segmenter leaves as one word and whose analysis has several gets
    /// these words: `pónganselo` becomes the multiword token `pongan` + `se` + `lo`, the
    /// imperative of `poner` and two pronouns; a document with heads, enhanced dependencies or
    /// empty nodes keeps its words, and so does a token whose word has XPOS or MISC other than
    /// the spaces after a token, which other words would lose; such a token gets the analysis of
    /// the whole. A number with a unit stays one word, as in the treebanks (`48km`). The words of a token of several are analyzed by the token: `dime` split into `di` +
    /// `me` is the imperative of `decir` with the pronoun `me`, and clitics after a verb are
    /// pronouns. A word without analyses gets its lower case as the lemma and the part of speech
    /// X; the earlier values of the columns are replaced
    ///
    /// ```
    /// # #[cfg(feature = "bundled")] {
    /// use esmorph::anyseg::{Column, Segmenter};
    ///
    /// let dictionary = esmorph::Dictionary::bundled()?;
    /// let mut document = Segmenter::new("es").unwrap().segment("Pónganselo ya.");
    /// dictionary.annotate(&mut document)?;
    /// assert_eq!(document.form(0), "Pongan");
    /// assert_eq!(document.column(Column::Lemma, 0), Some("poner"));
    /// assert_eq!(document.column(Column::Upos, 2), Some("PRON"));
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
        self.annotate_document(document, true)
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
        self.annotate_document(document, false)
    }

    #[cfg(feature = "anyseg")]
    fn annotate_document(
        &self,
        document: &mut anyseg::Document,
        context: bool,
    ) -> Result<(), Error> {
        let (words, fields) = self.document_analysis(document, context)?;
        if let Some(words) = words {
            *document = words;
        }
        set_fields(document, &fields)
    }
}

/// Whether the words of a document cannot change: other words cannot have its heads and enhanced
/// dependencies, which refer to the numbers of words; a document of the segmenter has none
#[cfg(feature = "anyseg")]
fn fixed_ids(document: &anyseg::Document) -> bool {
    use anyseg::Column;

    (0..document.word_count())
        .any(|word| document.head(word).is_some() || document.column(Column::Deps, word).is_some())
}

/// Whether a token keeps its words: other words would lose their XPOS and MISC, except the spaces
/// around a token, which anySeg gives the multiword token
#[cfg(feature = "anyseg")]
fn keeps_columns(document: &anyseg::Document, token: usize) -> bool {
    use anyseg::Column;

    document.token_words(token).any(|word| {
        document.column(Column::Xpos, word).is_some()
            || document
                .column(Column::Misc, word)
                .is_some_and(|misc| misc.split('|').any(|entry| !space_entry(entry)))
    })
}

/// The fields of a document whose tokens keep their words: a token that the analysis splits gets
/// the fields of the whole instead of those of its words
#[cfg(feature = "anyseg")]
fn whole_fields<'d>(
    document: &anyseg::Document,
    fields: Vec<Fields<'d>>,
    words: TokenWords<'d>,
) -> Vec<Fields<'d>> {
    let mut whole = Vec::with_capacity(document.word_count());
    let mut rows = fields.into_iter();
    let mut splits = words.into_iter().peekable();
    for token in 0..document.tokens().len() {
        match splits.next_if(|split| split.token == token) {
            Some(split) => {
                rows.by_ref().take(split.words.len()).for_each(drop);
                whole.push(split.whole);
            }
            None => whole.extend(rows.by_ref().take(document.token_words(token).len())),
        }
    }
    whole
}

/// Whether an entry of MISC is the spaces around a token
#[doc(hidden)]
pub fn space_entry(entry: &str) -> bool {
    ["SpaceAfter=", "SpacesAfter=", "SpacesBefore="]
        .iter()
        .any(|name| entry.starts_with(name))
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

/// The words of a token in its case, as the segmenter gives them: all in capitals if the token
/// is (`DÁMELO` is `DA` + `ME` + `LO`), the first one capitalized if the token starts so
fn cased(words: &[String], token: &str) -> Vec<String> {
    let capitals = token.chars().any(char::is_uppercase) && !token.chars().any(char::is_lowercase);
    let capitalized = token.chars().next().is_some_and(char::is_uppercase);
    (words.iter().enumerate())
        .map(|(number, word)| {
            if capitals {
                word.to_uppercase()
            } else if capitalized && number == 0 {
                let mut chars = word.chars();
                chars.next().map_or_else(String::new, |first| {
                    first.to_uppercase().chain(chars).collect()
                })
            } else {
                word.clone()
            }
        })
        .collect()
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
        // `ve` alone is mostly the present of `ver`, with a clitic the imperative of `ir`
        assert_eq!(summary("ve", &["ve"]), pairs(&[("ver", "VERB")]));
        assert_eq!(
            summary("vete", &["ve", "te"]),
            pairs(&[("ir", "VERB"), ("tú", "PRON")])
        );
        // A capital letter makes the token a name first, but its words are a verb and a clitic
        assert_eq!(
            summary("Vete", &["Ve", "te"]),
            pairs(&[("ir", "VERB"), ("tú", "PRON")])
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
            let sentences = [(0..tokens.len(), true)];
            let keep = |_| false;
            (dictionary.sentence_fields(&tokens, &sentences, context, &mut fields, None, &keep))
                .unwrap();
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
            // The segmenter leaves the token one word: it gets the words of the verb with clitics
            ("Devuelve", "devolver", "VERB"),
            ("nos", "yo", "PRON"),
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
        assert_eq!(columns[7].3, None);
        assert_eq!((document.tokens().len(), document.word_count()), (10, 15));
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
        assert_eq!(columns[4], ("Devuelve", "devolver", "VERB"));
        assert_eq!(columns[5], ("nos", "yo", "PRON"));
        dictionary.annotate_without_context(&mut document).unwrap();
        assert_eq!(document.column(Column::Upos, 1), Some("DET"));
        // A document with heads keeps its words: the token gets the analysis of the whole
        let mut parsed = anyseg::read_conllu(
            "1\tDevuélvenos\t_\t_\t_\t_\t0\troot\t_\tSpaceAfter=No\n2\t.\t_\t_\t_\t_\t1\tpunct\t_\t_\n\n",
        )
        .unwrap()
        .remove(0);
        dictionary.annotate(&mut parsed).unwrap();
        assert_eq!(parsed.word_count(), 2);
        assert_eq!(parsed.column(Column::Lemma, 0), Some("devolver"));
        assert_eq!(parsed.column(Column::Upos, 0), Some("VERB"));
        // So does one whose words have XPOS or MISC beyond the spaces, which other words lose; the
        // spaces go to the multiword token
        for (xpos, misc, words) in [
            ("VMM02S0", "SpaceAfter=No", 2),
            ("_", "SpaceAfter=No|start_char=0", 2),
            ("_", "SpaceAfter=No", 3),
        ] {
            let text = format!(
                "1\tDevuélvenos\t_\t_\t{xpos}\t_\t_\t_\t_\t{misc}\n2\t.\t_\t_\t_\t_\t_\t_\t_\t_\n\n"
            );
            let mut read = anyseg::read_conllu(&text).unwrap().remove(0);
            dictionary.annotate(&mut read).unwrap();
            assert_eq!(read.word_count(), words, "{misc}");
        }
    }
}
