from dataclasses import dataclass, field

import anyseg
import pytest
from anyseg import Segmenter

from esmorph import Analyzer


@pytest.fixture(scope="module")
def morph():
    return Analyzer()


@pytest.fixture(scope="module")
def segmenter():
    return Segmenter("es")


def columns(document):
    return [(word.form, word.lemma, word.upos) for word in document.words]


def test_annotate(morph, segmenter):
    document = segmenter.segment("Devuélvenos, dime la verdad del caso.\n\nDámelo.")
    assert morph.annotate(document) is None
    assert columns(document) == [
        # The segmenter leaves the token one word: it is the verb
        ("Devuélvenos", "devolver", "VERB"),
        (",", ",", "PUNCT"),
        # `di` alone is mostly the past of `dar`, `la` alone is mostly an article
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
    ]
    feats = document.column("feats")
    assert feats[2] == "Mood=Imp|Number=Sing|Person=2|VerbForm=Fin"
    # A word without features has none
    assert feats[6] is None
    assert "3-4\tdime\t_" in document.to_conllu()


def test_clitics_after_verb(morph, segmenter):
    document = segmenter.segment("Quiero verlas y dárselas.")
    morph.annotate(document)
    assert [(word.form, word.upos) for word in document.words if word.lemma == "él"] == [
        ("las", "PRON"),
        ("se", "PRON"),
        ("las", "PRON"),
    ]


def test_unknown_words(morph, segmenter):
    document = segmenter.segment("qzxw")
    morph.annotate(document)
    assert columns(document) == [("qzxw", "qzxw", "X")]
    assert document.column("feats") == [None]


def test_tabs_and_line_breaks_in_tokens(morph):
    document = anyseg.Document.from_tokens("a\tb x\r\ny", [0, 4], [3, 8])
    morph.annotate(document)
    assert document.column("lemma") == ["a b", "x y"]
    assert document.column("upos") == ["X", "X"]
    # The lemma is the form that anySeg writes in CoNLL-U
    assert "\tx y\tx y\t" in document.to_conllu()


def test_replaces_columns(morph, segmenter):
    document = segmenter.segment("Hablo.")
    document.set_columns(lemma=["x", "y"], upos=["X", "X"], feats=["A=B", "A=B"])
    morph.annotate(document)
    assert columns(document) == [("Hablo", "hablar", "VERB"), (".", ".", "PUNCT")]
    assert document.column("feats")[1] == "PunctType=Peri"


def test_empty_and_long_documents(morph, segmenter):
    document = segmenter.segment("")
    morph.annotate(document)
    assert document.column("lemma") == []
    # More words than a chunk without the GIL, with multiword tokens across the chunks
    document = segmenter.segment(" ".join(["Voy al mercado a verlas."] * 3000))
    morph.annotate(document)
    lemmas = document.column("lemma")
    assert len(lemmas) == 24_000
    assert lemmas[:8] == ["ir", "a", "el", "mercado", "a", "ver", "él", "."]
    assert lemmas[-8:] == lemmas[:8]


@dataclass
class Word:
    index: object


@dataclass
class Token:
    text: object
    words: list


@dataclass
class Document:
    """A document with the interface of anyseg.Document that annotate uses"""

    forms: list
    multiword_tokens: list = field(default_factory=list)
    columns: dict = field(default_factory=dict)

    def column(self, name, /):
        assert name == "form"
        return self.forms

    def set_columns(self, **columns):
        self.columns = columns


def test_document_protocol(morph):
    document = Document(["Di", "me", "la", "verdad"], [Token("Dime", [Word(0), Word(1)])])
    morph.annotate(document)
    assert document.columns == {
        "lemma": ["decir", "yo", "el", "verdad"],
        "upos": ["VERB", "PRON", "DET", "NOUN"],
        "feats": [
            "Mood=Imp|Number=Sing|Person=2|VerbForm=Fin",
            "Case=Dat|Number=Sing|Person=1|PrepCase=Npr|PronType=Prs",
            "Definite=Def|Gender=Fem|Number=Sing|PronType=Art",
            "Gender=Fem|Number=Sing",
        ],
    }
    # A token without words gives none
    document = Document(["hablo"], [Token("x", [])])
    morph.annotate(document)
    assert document.columns["lemma"] == ["hablar"]


@pytest.mark.parametrize(
    "tokens",
    [
        [Token("verla", [Word(0), Word(2)])],
        [Token("verla", [Word(1), Word(0)])],
        [Token("verla", [Word(2), Word(3)])],
        [Token("verla", [Word(-1), Word(0)])],
        [Token("verla", [Word(0), Word(1)]), Token("verla", [Word(1), Word(2)])],
        [Token("verla", [Word(1), Word(2)]), Token("verla", [Word(0)])],
        [Token("verla", [Word(2**70), Word(2**70 + 1)])],
        [Token("verla", [Word(-(2**70)), Word(0)])],
    ],
)
def test_multiword_tokens_in_order(morph, tokens):
    document = Document(["ver", "la", "casa"], tokens)
    with pytest.raises(ValueError, match="consecutive words of the document in order"):
        morph.annotate(document)
    assert document.columns == {}


def test_document_types(morph):
    with pytest.raises(TypeError, match="item 1 is int"):
        morph.annotate(Document(["ver", 1]))
    with pytest.raises(TypeError, match="text of a token must be a string, not int"):
        morph.annotate(Document(["ver", "la"], [Token(1, [Word(0), Word(1)])]))
    with pytest.raises(TypeError, match="index of a word must be an integer, not str"):
        morph.annotate(Document(["ver", "la"], [Token("verla", [Word("0"), Word(1)])]))
    with pytest.raises(TypeError, match="collection of words"):
        morph.annotate(Document("ver"))
