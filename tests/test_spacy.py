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


def test_sentences_by_blank_lines_and_closing_marks(nlp):
    # A heading or a name in a play before a blank line ends a sentence; a closing quote or
    # bracket after a sentence end mark stays in the sentence, and so do spaces
    assert annotation(nlp("Capítulo 1\n\nEra una noche oscura")) == [
        *expected(["Capítulo", "1"]),
        ("\n\n", "SPACE", {}),
        *expected(["Era", "una", "noche", "oscura"]),
    ]
    assert annotation(nlp("«Hola.» Dijo ella.")) == expected(
        ["«", "Hola", ".", "»"], ["Dijo", "ella", "."]
    )
    # A quote after a space or a blank line opens the next sentence
    assert annotation(nlp('Ella la vio. "Ven", dijo.')) == expected(
        ["Ella", "la", "vio", "."], ['"', "Ven", '"', ",", "dijo", "."]
    )
    assert annotation(nlp("Era ella.\n\n» Y luego.")) == [
        *expected(["Era", "ella", "."]),
        ("\n\n", "SPACE", {}),
        *expected(["»", "Y", "luego", "."]),
    ]
    assert annotation(nlp("Ella la vio.  La casa es grande.")) == [
        *expected(["Ella", "la", "vio", "."]),
        (" ", "SPACE", {}),
        *expected(["La", "casa", "es", "grande", "."]),
    ]
    # A single line break goes on with the sentence, as in verse
    words = expected(["Ella", "la", "vio", "ayer"])
    assert annotation(nlp("Ella la vio\nayer")) == [*words[:3], ("\n", "SPACE", {}), words[3]]


def test_marked_sentences():
    nlp = spacy.blank("es")
    nlp.add_pipe("sentencizer", config={"punct_chars": ["."]})
    nlp.add_pipe("esmorph")
    doc = nlp("Ella la vio! La casa es grande.")
    assert annotation(doc) == expected(
        ["Ella", "la", "vio", "!", "La", "casa", "es", "grande", "."]
    )


def test_partly_marked_sentences(nlp):
    # Doc.from_docs marks only the start of every document: the others are split by punctuation
    blank = spacy.blank("es")
    merged = spacy.tokens.Doc.from_docs(
        [blank("Ella la vio. La casa es grande."), blank("Anoche llovió.")]
    )
    assert annotation(nlp(merged)) == expected(
        ["Ella", "la", "vio", "."], ["La", "casa", "es", "grande", "."], ["Anoche", "llovió", "."]
    )
    # A token marked as no start is not split after a sentence end mark
    doc = blank("Ella la vio. La casa es grande.")
    for token in doc:
        token.is_sent_start = token.i == 0
    assert annotation(nlp(doc)) == expected(
        ["Ella", "la", "vio", ".", "La", "casa", "es", "grande", "."]
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


def test_tokens_without_analyses_and_spaces_lose_features(nlp):
    doc = spacy.blank("es")("qzxw  casa")
    for token in doc:
        token.set_morph("Gender=Fem")
    nlp(doc)
    assert [(token.pos_, str(token.morph)) for token in doc] == [
        ("X", ""),
        ("SPACE", ""),
        ("NOUN", "Gender=Fem|Number=Sing"),
    ]


def test_relative_dictionary_path(tmp_path, monkeypatch):
    (tmp_path / "es.dict").symlink_to(dictionary_path())
    monkeypatch.chdir(tmp_path)
    nlp = spacy.blank("es")
    nlp.add_pipe("esmorph", config={"dictionary": "es.dict"})
    doc = nlp("Ella la vio")
    assert doc.user_data["esmorph"] == str(tmp_path / "es.dict")
    monkeypatch.chdir(tmp_path.parent)
    assert doc[1]._.esmorph.pos == "PRON"
    assert pickle.loads(pickle.dumps(nlp))("Ella")[0].pos_ == "PRON"


def test_without_context():
    nlp = spacy.blank("es")
    nlp.add_pipe("esmorph", config={"context": False})
    doc = nlp("Ella la vio")
    assert doc[1].pos_ == "DET"
    assert annotation(doc) == [
        (analyses[0].lemma, analyses[0].pos, analyses[0].feats)
        for analyses in esmorph.Analyzer().analyze_many(["Ella", "la", "vio"])
    ]


def test_score():
    # nlp.evaluate and spacy evaluate score the component as the morphologizer and the lemmatizer
    nlp = spacy.blank("es")
    nlp.add_pipe("esmorph")
    gold = spacy.tokens.Doc(
        nlp.vocab,
        words=["Ella", "la", "vio"],
        pos=["PRON", "PRON", "VERB"],
        lemmas=["él", "él", "ver"],
        morphs=["", "", "Mood=Ind|Number=Sing|Person=3|Tense=Past|VerbForm=Fin"],
    )
    example = spacy.training.Example(nlp.make_doc("Ella la vio"), gold)
    scores = nlp.evaluate([example])
    assert (scores["pos_acc"], scores["lemma_acc"]) == (1.0, 1.0)
    assert scores["morph_acc"] == 1 / 3
    assert "Mood" in scores["morph_per_feat"]
    assert dict(nlp.config["training"]["score_weights"]) == {
        "pos_acc": 0.33,
        "morph_acc": 0.33,
        "morph_per_feat": None,
        "lemma_acc": 0.34,
    }


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
    import esmorph.spacy

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
    # UTF-8 in the child as in the parent: a Windows console writes another code page
    result = subprocess.run(
        [sys.executable, "-X", "utf8", "-c", code],
        input=pickle.dumps(nlp),
        capture_output=True,
        check=False,
    )
    assert result.returncode == 0, result.stderr.decode(errors="replace")
    assert result.stdout.decode("utf-8").split() == ["PRON", "él"]


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
    doc = nlp("Dámelo ya.\nElla la vio del balcón. Dímelo.")
    words = [(token.text, token.pos_, token.lemma_) for token in doc if not token.is_space]
    assert words[:2] == [("Dámelo", "VERB", "dar"), ("ya", "ADV", "ya")]
    assert words[4:8] == [
        ("la", "PRON", "él"),
        ("vio", "VERB", "ver"),
        ("del", "ADP", "de"),
        ("balcón", "NOUN", "balcón"),
    ]
    assert [word["form"] for word in doc[0]._.anyseg_words] == ["Da", "me", "lo"]
    assert [token.pos_ for token in doc if token.is_space] == ["SPACE"]
    assert [sentence.text for sentence in doc.sents][1] == "Ella la vio del balcón."
    # The analysis with the words of the token, as the analysis of a document chooses it: in the
    # sentence alone the token is rather a name
    token = doc[-2]
    assert (token.text, token.pos_, token.lemma_) == ("Dímelo", "VERB", "decir")
    assert token._.esmorph.words == ("di", "me", "lo")
    assert esmorph.Analyzer().analyze_sentence(["Dímelo", "."])[0][0].pos == "PROPN"
    # A word of the dictionary that anySeg splits is a verb with clitics, as for annotate
    dale = nlp("Dale la mano.")[0]
    assert (dale.pos_, dale.lemma_, dale._.esmorph.words) == ("VERB", "dar", ("da", "le"))
