# Spanish morphological analyzer (esMorph)
#
# Copyright (C) 2026
# Author: Sergey Shkarin <kouki.sergey@gmail.com>
# URL: <https://github.com/SergeyShk/esMorph>

"""
The spaCy component esmorph: the part of speech, the features and the lemma of tokens by the
analysis of esMorph in context

Description:
    The component replaces the tagger, the morphologizer and the lemmatizer of a spaCy pipeline:
    it analyzes the sentences of a document with the context model of esMorph and writes the part
    of speech (`pos_`), the features (`morph`) and the lemma (`lemma_`) of the chosen analysis to
    the tokens. A token of several words (`del`, `dámelo`) gets the analysis of its first word:
    `del` is ADP `de`, `dámelo` is VERB `dar`; its words are in the analysis. The sentences are
    those of `Doc.sents` if a parser or a sentencizer marked them, otherwise the document is
    split after the tokens of sentence end marks. Whitespace tokens get the part of speech SPACE.
    The analysis of esMorph of a token is `token._.esmorph`. After `pip install "esmorph[spacy]"`
    the component is added by its name, without importing esmorph

Example:
    >>> import spacy
    >>> nlp = spacy.blank("es")
    >>> _ = nlp.add_pipe("esmorph")
    >>> [(token.lemma_, token.pos_) for token in nlp("Ella la vio del balcón.")]
    [('él', 'PRON'), ('él', 'PRON'), ('ver', 'VERB'), ('de', 'ADP'), ('balcón', 'NOUN'), ('.', 'PUNCT')]
"""

import threading
import unicodedata
from collections import OrderedDict
from collections.abc import Iterator

from spacy.language import Language
from spacy.tokens import Doc, MorphAnalysis, Token

from ._esmorph import Analysis, Analyzer

__all__ = ["Component"]

# A document without marked sentences is split after a token of sentence end marks
SENTENCE_END = frozenset(".!?…")
# The key of the dictionary of the component in `Doc.user_data`: `token._.esmorph` finds the
# analyzer by it; the default dictionary is not written, as `Doc.from_docs`, `DocBin` and
# `Span.as_doc` lose it
SETTINGS = "esmorph"
# Analyzers of different dictionaries in a process: at most this many
CACHE_SIZE = 8
# Invisible separators, as in anySeg: a token of them and spaces (a BOM, U+200B between words for
# the tokenizer of anySeg) is a space, the model does not see it
SEPARATORS = frozenset("­​‌‍⁠﻿")

_analyzers: OrderedDict[str | None, Analyzer] = OrderedDict()
_analyzers_lock = threading.Lock()


@Language.factory(
    "esmorph",
    default_config={"dictionary": None, "context": True},
    assigns=["token.pos", "token.morph", "token.lemma"],
)
def create_component(
    nlp: Language, name: str, dictionary: str | None, context: bool
) -> "Component":
    """
    The component esmorph for a spaCy pipeline: `nlp.add_pipe("esmorph", config={...})`

    Arguments:
        nlp (Language): The pipeline
        name (str): The name of the component in the pipeline
        dictionary (str | None): The path of the dictionary, None - the dictionary of the package
        context (bool): Choose the analysis by the tokens around; False - the most probable
            analysis of a token without context, as `Analyzer.analyze` orders them

    Returns:
        Component: The component
    """
    return Component(dictionary, context)


class Component:
    """
    The spaCy component: the part of speech, the features and the lemma of tokens by the analysis
    of esMorph

    Arguments:
        dictionary (str | None): The path of the dictionary, None - the dictionary of the package
        context (bool): Choose the analysis by the tokens around
    """

    def __init__(self, dictionary: str | None = None, context: bool = True) -> None:
        self.dictionary = dictionary
        self.context = context
        self.analyzer = analyzer(dictionary)
        if context:
            # A dictionary without a context model fails here, not on the first document
            self.analyzer.analyze_sentence([])

    def __reduce__(self) -> tuple[type, tuple[str | None, bool]]:
        # pickle keeps the path, not the dictionary
        return Component, (self.dictionary, self.context)

    def __call__(self, doc: Doc) -> Doc:
        """
        Analysis of a document: the part of speech, the features and the lemma of every token

        Arguments:
            doc (Doc): The document

        Returns:
            Doc: The same document
        """
        for sentence in sentences(doc):
            tokens = [token for token in sentence if not space(token)]
            words = [token.text for token in tokens]
            if self.context:
                analyses = self.analyzer.analyze_sentence(words)
            else:
                analyses = self.analyzer.analyze_many(words)
            for token, candidates in zip(tokens, analyses, strict=True):
                if not candidates:
                    # As a word without analyses in the command esmorph
                    token.pos_, token.lemma_ = "X", token.text.lower()
                    continue
                best = candidates[0]
                token.pos_ = best.pos
                # A string: from a dict spaCy 3.8 accumulates memory for every token
                feats = "|".join(f"{name}={value}" for name, value in best.feats.items())
                token.morph = MorphAnalysis(doc.vocab, feats)
                token.lemma_ = best.lemma
        for token in doc:
            if space(token):
                token.pos_ = "SPACE"
                token.lemma_ = token.text
        if self.dictionary is not None:
            doc.user_data[SETTINGS] = self.dictionary
        else:
            doc.user_data.pop(SETTINGS, None)
        return doc


def space(token: Token) -> bool:
    """
    Whether a token is a space: of whitespace, control characters and invisible separators

    Arguments:
        token (Token): The token

    Returns:
        bool: Whether the token is a space
    """
    return token.is_space or all(
        character.isspace() or character in SEPARATORS or unicodedata.category(character) == "Cc"
        for character in token.text
    )


def analyzer(dictionary: str | None) -> Analyzer:
    """
    The analyzer of a dictionary: one per process for a dictionary, the long unused ones are
    dropped

    Arguments:
        dictionary (str | None): The path of the dictionary, None - the dictionary of the package

    Returns:
        Analyzer: The analyzer
    """
    with _analyzers_lock:
        found = _analyzers.get(dictionary)
        if found is not None:
            _analyzers.move_to_end(dictionary)
            return found
    created = Analyzer(dictionary)
    with _analyzers_lock:
        _analyzers[dictionary] = created
        _analyzers.move_to_end(dictionary)
        while len(_analyzers) > CACHE_SIZE:
            _analyzers.popitem(last=False)
    return created


def analysis(token: Token) -> Analysis | None:
    """
    The analysis of esMorph of a token (`token._.esmorph`): the analysis of the word form with the
    lemma, the part of speech and the features of the token by the dictionary of the component
    that analyzed the document; it gives the words of the token and the forms of the word. The
    probability is the one without context

    Arguments:
        token (Token): The token

    Returns:
        Analysis | None: The analysis or None if the word form has no such analysis
    """
    dictionary = token.doc.user_data.get(SETTINGS)
    feats = token.morph.to_dict()
    for candidate in analyzer(dictionary).analyze(token.text):
        if (candidate.lemma, candidate.pos, candidate.feats) == (token.lemma_, token.pos_, feats):
            return candidate
    return None


# An extension of someone else with the same name is not replaced
if not Token.has_extension("esmorph"):
    Token.set_extension("esmorph", getter=analysis)


def sentences(doc: Doc) -> Iterator[list[Token]]:
    """
    The sentences of a document: marked or up to a sentence end mark

    Arguments:
        doc (Doc): The document

    Returns:
        Iterator[list[Token]]: The tokens of the sentences
    """
    if doc.has_annotation("SENT_START"):
        for sentence in doc.sents:
            yield list(sentence)
        return
    chunk = []
    for token in doc:
        chunk.append(token)
        if token.text and set(token.text) <= SENTENCE_END:
            yield chunk
            chunk = []
    if chunk:
        yield chunk
