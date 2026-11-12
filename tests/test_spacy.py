"""The spaCy component esmorph: the part of speech, the features and the lemma of tokens"""

import importlib.metadata
import importlib.resources
import pickle
import subprocess
import sys
import warnings

import pytest

import esmorph

spacy = pytest.importorskip("spacy")


@pytest.fixture(scope="module")
def nlp():
    nlp = spacy.blank("es")
    nlp.add_pipe("esmorph")
    return nlp


def dictionary_path():
    return str(importlib.resources.files("esmorph") / "data" / "es.dict")


def annotation(doc):
    return [(token.lemma_, token.pos_, token.morph.to_dict()) for token in doc]


def expected(*sentences):
    morph = esmorph.Analyzer()
    return [
        (analyses[0].lemma, analyses[0].pos, analyses[0].feats)
        for words in sentences
        for analyses in morph.analyze_sentence(words)
    ]


def test_entry_point():
    points = importlib.metadata.entry_points(group="spacy_factories", name="esmorph")
    assert [point.value for point in points] == ["esmorph.spacy:create_component"]


def test_context(nlp):
    doc = nlp("Ella la vio del balcón. Dámelo ya.")
    assert [(token.lemma_, token.pos_) for token in doc[:5]] == [
        ("él", "PRON"),
        ("él", "PRON"),
        ("ver", "VERB"),
        ("de", "ADP"),
        ("balcón", "NOUN"),
    ]
    assert (
        str(doc[1].morph) == "Case=Acc|Gender=Fem|Number=Sing|Person=3|PrepCase=Npr|PronType=Prs"
    )
    # A token of several words: the analysis of its first word
    assert (doc[6].lemma_, doc[6].pos_, doc[6].morph.get("Mood")) == ("dar", "VERB", ["Imp"])


def test_sentences_by_punctuation(nlp):
    doc = nlp("Ella la vio. La casa es grande!")
    assert annotation(doc) == expected(
        ["Ella", "la", "vio", "."], ["La", "casa", "es", "grande", "!"]
    )


def test_marked_sentences():
    nlp = spacy.blank("es")
    nlp.add_pipe("sentencizer", config={"punct_chars": ["."]})
    nlp.add_pipe("esmorph")
    doc = nlp("Ella la vio! La casa es grande.")
    assert annotation(doc) == expected(
        ["Ella", "la", "vio", "!", "La", "casa", "es", "grande", "."]
    )


def test_long_sentences(nlp):
    # A sentence of more than 256 tokens goes to the analyzer whole: it analyzes it by parts
    words = ["Ella", "la", "vio"] * 100
    assert annotation(nlp(" ".join(words))) == expected(words)
    marked = spacy.blank("es")
    marked.add_pipe("sentencizer")
    marked.add_pipe("esmorph")
    assert annotation(marked(" ".join(words))) == expected(words)


def test_spaces_and_invisible_tokens(nlp):
    # A token of invisible separators is a space: the model does not see it
    tokens = list(nlp("Ella\n\nla \u2060 vio"))
    assert [(token.pos_, token.lemma_) for token in tokens[1::2]] == [
        ("SPACE", "\n\n"),
        ("SPACE", "\u2060"),
    ]
    analyses = esmorph.Analyzer().analyze_sentence(["Ella", "la", "vio"])
    assert annotation(tokens[::2]) == [(a[0].lemma, a[0].pos, a[0].feats) for a in analyses]
    # A separator inside a token is dropped, as analyze drops it
    joined = nlp("Ella\u200bvio")
    assert [(token.pos_, token.lemma_) for token in joined] == [("PROPN", "Ellavio")]


def test_without_context():
    nlp = spacy.blank("es")
    nlp.add_pipe("esmorph", config={"context": False})
    doc = nlp("Ella la vio")
    assert doc[1].pos_ == "DET"
    assert annotation(doc) == [
        (analyses[0].lemma, analyses[0].pos, analyses[0].feats)
        for analyses in esmorph.Analyzer().analyze_many(["Ella", "la", "vio"])
    ]


def test_config(tmp_path):
    nlp = spacy.blank("es")
    nlp.add_pipe("esmorph", config={"dictionary": dictionary_path()})
    lemmas = ["él", "él", "ver"]
    assert [token.lemma_ for token in nlp("Ella la vio")] == lemmas
    assert nlp.get_pipe_meta("esmorph").assigns == ["token.pos", "token.morph", "token.lemma"]
    nlp.to_disk(tmp_path)
    assert [token.lemma_ for token in spacy.load(tmp_path)("Ella la vio")] == lemmas


def test_extension(nlp):
    doc = nlp("Ella la vio del balcón. Dámelo ya.")
    analysis = doc[4]._.esmorph
    assert isinstance(analysis, esmorph.Analysis)
    assert (analysis.lemma, analysis.inflect(Number="Plur")) == ("balcón", "balcones")
    assert doc[3]._.esmorph.words == ("de", "el")
    assert doc[6]._.esmorph.words == ("da", "me", "lo")
    doc[4].lemma_ = "balcones"
    assert doc[4]._.esmorph is None


def test_analyzer_cache_is_bounded(tmp_path):
    docs = []
    for number in range(esmorph.spacy.CACHE_SIZE + 3):
        link = tmp_path / f"es{number}.dict"
        link.symlink_to(dictionary_path())
        own = spacy.blank("es")
        own.add_pipe("esmorph", config={"dictionary": str(link)})
        docs.append(own("Ella la vio"))
        assert docs[-1].user_data["esmorph"] == str(link)
    assert len(esmorph.spacy._analyzers) <= esmorph.spacy.CACHE_SIZE
    # A dropped analyzer is created anew by the dictionary of the document
    assert docs[0][1]._.esmorph.pos == "PRON"


def test_serialization(nlp):
    doc = nlp("Ella la vio del balcón")
    restored = spacy.tokens.Doc(nlp.vocab).from_bytes(doc.to_bytes())
    assert annotation(restored) == annotation(doc)
    assert restored[1]._.esmorph.pos == "PRON"
    stored = spacy.tokens.DocBin(docs=[doc]).get_docs(nlp.vocab)
    assert next(stored)[4]._.esmorph.lemma == "balcón"
    assert annotation(pickle.loads(pickle.dumps(nlp))("Ella la vio del balcón")) == annotation(doc)


def test_pickle_in_new_process():
    nlp = spacy.blank("es")
    nlp.add_pipe("esmorph", config={"dictionary": dictionary_path()})
    code = (
        "import pickle, sys; doc = pickle.load(sys.stdin.buffer)('Ella la vio'); "
        "print(doc[1].pos_, doc[1]._.esmorph.lemma)"
    )
    result = subprocess.run(
        [sys.executable, "-c", code], input=pickle.dumps(nlp), capture_output=True, check=False
    )
    assert result.returncode == 0, result.stderr.decode(errors="replace")
    assert result.stdout.decode().split() == ["PRON", "él"]


def test_foreign_extension_kept():
    code = (
        "from spacy.tokens import Token; Token.set_extension('esmorph', default='own'); "
        "import esmorph.spacy, spacy; print(spacy.blank('es')('Ella')[0]._.esmorph)"
    )
    result = subprocess.run([sys.executable, "-c", code], capture_output=True, check=False)
    assert result.returncode == 0, result.stderr.decode(errors="replace")
    assert result.stdout.decode().split() == ["own"]


def test_default_dictionary_not_stored(nlp):
    docs = [nlp("Ella la vio."), nlp("La casa es grande.")]
    assert all("esmorph" not in doc.user_data for doc in docs)
    with warnings.catch_warnings():
        warnings.simplefilter("error")
        merged = spacy.tokens.Doc.from_docs(docs)
    assert merged[1]._.esmorph.pos == "PRON"
    assert docs[1][1:3].as_doc()[0]._.esmorph.lemma == "casa"
    # A document analyzed again with the default dictionary forgets another one
    own = spacy.blank("es")
    own.add_pipe("esmorph", config={"dictionary": dictionary_path()})
    doc = nlp(own("Ella la vio"))
    assert "esmorph" not in doc.user_data


def test_anyseg_tokenizer():
    # Surface tokens and sentences of anySeg: a token of several words gets the analysis of its
    # first word, whitespace tokens are SPACE
    tokenizer = {"@tokenizers": "anyseg.Tokenizer.v1"}
    nlp = spacy.blank("es", config={"nlp": {"tokenizer": tokenizer}})
    nlp.add_pipe("anyseg_sentencizer")
    nlp.add_pipe("esmorph")
    doc = nlp("Dámelo ya.\nElla la vio del balcón.")
    words = [(token.text, token.pos_, token.lemma_) for token in doc if not token.is_space]
    assert words[:2] == [("Dámelo", "VERB", "dar"), ("ya", "ADV", "ya")]
    assert words[-5:-1] == [
        ("la", "PRON", "él"),
        ("vio", "VERB", "ver"),
        ("del", "ADP", "de"),
        ("balcón", "NOUN", "balcón"),
    ]
    assert [word["form"] for word in doc[0]._.anyseg_words] == ["Da", "me", "lo"]
    assert [token.pos_ for token in doc if token.is_space] == ["SPACE"]
    assert [sentence.text for sentence in doc.sents][-1] == "Ella la vio del balcón."
