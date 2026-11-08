import copy
import errno
import json
import pickle
import threading
from pathlib import Path

import pytest

import esmorph
from esmorph import Analysis, Analyzer

ROOT = Path(__file__).parents[1]


@pytest.fixture(scope="module")
def morph():
    return Analyzer()


def lemmas(morph, word):
    return sorted({analysis.lemma for analysis in morph.analyze(word)})


def lemmas_set(morph, word):
    return set(lemmas(morph, word))


def test_analysis_fields(morph):
    analysis = morph.analyze("camión")[0]
    assert isinstance(analysis, Analysis)
    assert analysis.word == "camión"
    assert analysis.lemma == "camión"
    assert analysis.pos == "NOUN"
    assert analysis.feats == {"Gender": "Masc", "Number": "Sing"}
    assert analysis.words == ("camión",)
    assert analysis.source == "dictionary"


def test_analyses_of_inflected_forms(morph):
    assert lemmas(morph, "camiones") == ["camión"]
    assert lemmas(morph, "hablo") == ["hablar"]
    assert "casa" in lemmas(morph, "casas") and "casar" in lemmas(morph, "casas")
    verb = next(a for a in morph.analyze("hablamos") if a.feats.get("Tense") == "Pres")
    assert verb.pos == "VERB"
    assert verb.feats == {
        "Mood": "Ind",
        "Number": "Plur",
        "Person": "1",
        "Tense": "Pres",
        "VerbForm": "Fin",
    }
    participle = next(a for a in morph.analyze("habladas") if a.pos == "VERB")
    assert participle.lemma == "hablar"
    assert participle.feats["VerbForm"] == "Part"
    assert participle.feats["Gender"] == "Fem"


def test_conjugation_tables(morph):
    forms = {
        "hablé": {"Mood": "Ind", "Number": "Sing", "Person": "1", "Tense": "Past"},
        "hablaba": {"Mood": "Ind", "Tense": "Imp"},
        "hablará": {"Mood": "Ind", "Tense": "Fut"},
        "hablaría": {"Mood": "Cnd"},
        "hablen": {"Mood": "Sub", "Tense": "Pres"},
        "hablara": {"Mood": "Sub", "Tense": "Imp"},
        "hablase": {"Mood": "Sub", "Tense": "Imp"},
        "hablare": {"Mood": "Sub", "Tense": "Fut"},
        "hablad": {"Mood": "Imp", "Number": "Plur", "Person": "2"},
        "hablá": {"Mood": "Imp", "Number": "Sing", "Person": "2"},
        "hablando": {"VerbForm": "Ger"},
        "hablar": {"VerbForm": "Inf"},
    }
    for word, expected in forms.items():
        analyses = [a for a in morph.analyze(word) if a.lemma == "hablar"]
        assert any(expected.items() <= a.feats.items() for a in analyses), word


def test_irregular_verbs(morph):
    for word, lemma in [
        ("fue", "ser"),
        ("fue", "ir"),
        ("soy", "ser"),
        ("hizo", "hacer"),
        ("tengo", "tener"),
        ("vaya", "ir"),
        ("puesto", "poner"),
    ]:
        assert lemma in lemmas(morph, word), word


def test_case_does_not_matter(morph):
    for word in ["CAMIÓN", "Camión", "cAmIóN"]:
        assert lemmas(morph, word) == ["camión"]
    assert morph.analyze("Camión")[0].word == "camión"


def test_exact_spelling_wins_over_other_accents(morph):
    assert {a.word for a in morph.analyze("publico")} == {"publico"}
    assert {a.word for a in morph.analyze("público")} == {"público"}
    assert {a.word for a in morph.analyze("publicó")} == {"publicó"}
    assert {a.word for a in morph.analyze("mas")} == {"mas"}
    assert {a.word for a in morph.analyze("más")} == {"más"}


def test_word_absent_as_written_looks_through_accents(morph):
    assert morph.analyze("camion")[0].word == "camión"
    assert morph.analyze("atencion")[0].word == "atención"
    assert morph.analyze("EXAMEN")[0].word == "examen"
    # A moved accent
    assert morph.analyze("cámion")[0].word == "camión"


def test_lost_diaeresis(morph):
    assert morph.analyze("pinguino")[0].word == "pingüino"
    assert morph.analyze("verguenza")[0].word == "vergüenza"
    assert morph.is_known("pinguino")
    assert {a.word for a in morph.analyze("pingüino")} == {"pingüino"}
    # A spelling with `gu` stays as it is
    assert morph.analyze("guerra")[0].word == "guerra"


def test_tilde_is_no_accent(morph):
    assert lemmas(morph, "año") == ["año"]
    assert "ano" in lemmas(morph, "ano")
    assert "año" not in lemmas(morph, "ano")


def test_normalized_queries(morph):
    assert lemmas(morph, "camión") == ["camión"]
    assert lemmas(morph, "CAMIÓN") == ["camión"]
    assert lemmas(morph, "año") == ["año"]
    assert lemmas(morph, "ca­mión") == ["camión"]
    assert lemmas(morph, "﻿camión​") == ["camión"]


@pytest.mark.parametrize(
    "word",
    [
        "hazlo",
        "mirándose",
        "dámelo",
        "hablarlo",
        "hablarme",
        "lávate",
        "levántate",
        "dígame",
        "quítatela",
        "comprársela",
        "dándoselo",
        "abatiéndome",
        "ufánate",
        "ufanándose",
        "ufanarse",
        "antojándose",
        "abóyate",
        "pícatela",
        "dénsela",
        "hételas",
    ],
)
def test_forms_with_clitics_are_not_word_forms(morph, word):
    # The dictionary has no such form: the verb comes from the rule of clitics
    assert all(a.source == "clitics" for a in morph.analyze(word) if a.pos == "VERB"), word


@pytest.mark.parametrize(
    "word, lemma, words",
    [
        ("dámelo", "dar", ("da", "me", "lo")),
        ("Dámelo", "dar", ("da", "me", "lo")),
        ("damelo", "dar", ("da", "me", "lo")),
        ("hacerlo", "hacer", ("hacer", "lo")),
        ("diciéndoselo", "decir", ("diciendo", "se", "lo")),
        ("oírlo", "oír", ("oír", "lo")),
        ("déme", "dar", ("dé", "me")),
        ("vámonos", "ir", ("vamos", "nos")),
        ("digámoselo", "decir", ("digamos", "se", "lo")),
        ("sentaos", "sentar", ("sentad", "os")),
        ("cuéntenos", "contar", ("cuente", "nos")),
        ("llámenos", "llamar", ("llame", "nos")),
        ("vérselas", "ver", ("ver", "se", "las")),
        ("quejarse", "quejar", ("quejar", "se")),
        ("resiéntete", "resentir", ("resiente", "te")),
        ("quejémonos", "quejar", ("quejemos", "nos")),
    ],
)
def test_words_with_clitics(morph, word, lemma, words):
    analysis = morph.analyze(word)[0]
    assert (analysis.lemma, analysis.pos, analysis.words) == (lemma, "VERB", words)
    assert analysis.source == "clitics"
    assert analysis.word == word.lower()
    assert sum(a.prob for a in morph.analyze(word)) == pytest.approx(1)
    assert morph.is_known(word)
    assert morph.lemmatize([word]) == [lemma]


def test_hosts_of_clitics_are_infinitives_gerunds_and_imperatives(morph):
    for word in ["dámelo", "hacerlo", "diciéndoselo", "vámonos"]:
        for analysis in morph.analyze(word):
            assert (
                analysis.feats.get("VerbForm") in ("Inf", "Ger") or analysis.feats["Mood"] == "Imp"
            )


def test_words_of_the_dictionary_are_not_split(morph):
    assert {a.source for a in morph.analyze("comete")} == {"dictionary"}
    assert morph.analyze("cómetelo")[0].words == ("come", "te", "lo")
    assert not morph.is_known("kayla")


def test_words_of_contractions(morph):
    assert morph.analyze("del")[0].words == ("de", "el")
    assert morph.analyze("Al")[0].words == ("a", "el")
    assert morph.analyze("camión")[0].words == ("camión",)


@pytest.mark.parametrize(
    "word, lemma",
    [
        ("resintiendo", "resentir"),
        ("inmiscuyendo", "inmiscuir"),
        ("ufanando", "ufanar"),
        ("antojando", "antojar"),
    ],
)
def test_plain_gerunds_of_reflexive_verbs(morph, word, lemma):
    verbs = [a for a in morph.analyze(word) if a.pos == "VERB"]
    assert [(a.lemma, a.feats) for a in verbs] == [(lemma, {"VerbForm": "Ger"})]


def test_participles_have_their_verb_as_the_lemma(morph):
    for word in ["hablado", "hablada", "hablados", "habladas"]:
        verbs = [a for a in morph.analyze(word) if a.pos == "VERB"]
        assert {a.lemma for a in verbs} == {"hablar"}, word


def test_misspellings_are_not_analyses(morph):
    assert "haber" in lemmas(morph, "hay")
    assert "ahí" not in lemmas(morph, "hay")


def test_plural_is_no_lemma(morph):
    assert "casas" not in {a.lemma for a in morph.analyze("casas") if a.pos == "NOUN"}


@pytest.mark.parametrize(
    "word, lemma, other",
    [
        ("padres", "padre", "madre"),
        ("hijos", "hijo", "hija"),
        ("niños", "niño", "niña"),
        ("niños", "niño", "niñe"),
        ("caballo", "caballo", "yegua"),
    ],
)
def test_masculine_nouns_have_no_lemma_of_another_gender(morph, word, lemma, other):
    nouns = {a.lemma for a in morph.analyze(word) if a.pos == "NOUN"}
    assert lemma in nouns
    assert other not in nouns


@pytest.mark.parametrize(
    "word, lemma",
    [
        ("exaltador", "exaltador"),
        ("exaltadores", "exaltador"),
        ("psicooncólogo", "psicooncólogo"),
    ],
)
def test_masculine_nouns_known_only_from_the_feminine_table(morph, word, lemma):
    nouns = [a for a in morph.analyze(word) if a.pos == "NOUN"]
    assert [a.lemma for a in nouns] == [lemma]
    assert nouns[0].feats["Gender"] == "Masc"


def test_feminine_noun_has_both_lemmas(morph):
    nouns = {a.lemma for a in morph.analyze("niña") if a.pos == "NOUN"}
    assert {"niña", "niño"} <= nouns


def test_nouns_of_both_genders(morph):
    for word in ["presidente", "fin", "personaje", "presidentes"]:
        genders = {a.feats.get("Gender") for a in morph.analyze(word) if a.pos == "NOUN"}
        assert genders == {"Fem", "Masc"}, word


def test_acronyms_do_not_answer_for_words(morph):
    for word, acronym in [
        ("se", "sudeste"),
        ("ha", "ácido"),
        ("me", "muerte"),
        ("una", "universidad"),
    ]:
        assert acronym not in lemmas(morph, word), word
    assert morph.is_known("fbi")


def test_references_that_are_no_entries_are_no_lemmas(morph):
    for word in ["fué", "guión", "iraq", "ó", "mi", "tu", "una", "nada", "hacer", "eu", "eso"]:
        assert not {
            "deprecated",
            "my",
            "your",
            "universidad",
            "de",
            "hacerse",
            "tren",
            "estados",
        } & set(lemmas(morph, word)), word


def test_only_the_first_reference_is_a_lemma(morph):
    for word, gloss in [
        ("jevi", "cool"),
        ("zijo", "son"),
        ("dcho", "right"),
        ("pisha", "dude"),
    ]:
        assert gloss not in lemmas(morph, word), word
    assert "hijo" in lemmas(morph, "zijo")


@pytest.mark.parametrize(
    "word, pos",
    [
        ("treintaiuno", "NUM"),
        ("cuarentaicinco", "NUM"),
        ("denada", "INTJ"),
        ("noramala", "ADV"),
    ],
)
def test_word_that_refers_to_a_phrase_is_a_lemma(morph, word, pos):
    assert (word, pos) in [(a.lemma, a.pos) for a in morph.analyze(word)]


def test_pronoun_references_are_split(morph):
    assert "él" in lemmas(morph, "lo")
    assert "tú" in lemmas(morph, "te")


def test_only_infinitives_are_verb_lemmas(morph):
    assert "VERB" not in {a.pos for a in morph.analyze("persona") if a.lemma == "persona"}
    assert "había" not in lemmas(morph, "había")
    bare = [a for a in morph.analyze("accelerar") if a.pos == "VERB"]
    assert all(a.feats == {"VerbForm": "Inf"} for a in bare)
    assert {"ser", "ir"} <= lemmas_set(morph, "fué")
    for word in ["fuer", "quier"]:
        assert "VERB" not in {a.pos for a in morph.analyze(word)}, word


def test_one_sense_with_several_persons_and_tenses(morph):
    persons = {a.feats.get("Person") for a in morph.analyze("adestraba")}
    assert {"1", "3"} <= persons
    tenses = {a.feats.get("Tense") for a in morph.analyze("ababillamos")}
    assert {"Pres", "Past"} <= tenses


def test_finite_rows_of_pronominal_tables(morph):
    assert morph.is_known("destoso")
    assert "resentir" in lemmas(morph, "resiente")


@pytest.mark.parametrize(
    "word, lemma",
    [
        ("resiente", "resentir"),
        ("quejarse", "quejar"),
        ("destoso", "destoser"),
        ("destoser", "destoser"),
    ],
)
def test_verbs_have_no_clitic_in_the_lemma(morph, word, lemma):
    assert {a.lemma for a in morph.analyze(word) if a.pos == "VERB"} == {lemma}


@pytest.mark.parametrize(
    "word, lemma, pos",
    [
        ("la", "el", "DET"),
        ("los", "el", "DET"),
        ("un", "uno", "DET"),
        ("una", "uno", "DET"),
        ("se", "él", "PRON"),
        ("me", "yo", "PRON"),
        ("te", "tú", "PRON"),
        ("usted", "tú", "PRON"),
        ("esto", "este", "PRON"),
        ("éste", "este", "PRON"),
        ("del", "de", "ADP"),
        ("al", "a", "ADP"),
        ("muy", "mucho", "ADV"),
        ("tan", "tanto", "ADV"),
        ("buena", "buen", "ADJ"),
        ("malos", "mal", "ADJ"),
        ("primer", "primero", "ADJ"),
        ("peor", "peor", "ADV"),
        ("pa", "para", "ADP"),
        ("pa'", "para", "ADP"),
        ("cuando", "cuando", "SCONJ"),
    ],
)
def test_lemmas_of_the_treebanks(morph, word, lemma, pos):
    assert (lemma, pos) in {(a.lemma, a.pos) for a in morph.analyze(word)}, word
    assert morph.analyze(word)[0].lemma == lemma, word


@pytest.mark.parametrize(
    "word, pos, feats",
    [
        ("el", "DET", {"Definite": "Def", "PronType": "Art"}),
        ("mi", "DET", {"Person": "1", "Poss": "Yes"}),
        ("no", "ADV", {"Polarity": "Neg"}),
        ("más", "ADV", {"Degree": "Cmp"}),
        ("dónde", "ADV", {"PronType": "Int"}),
        ("cómo", "PRON", {"PronType": "Int"}),
        ("alguno", "DET", {"PronType": "Ind"}),
        ("ninguno", "DET", {"PronType": "Neg"}),
        ("antepenúltimo", "ADJ", {"NumType": "Ord"}),
        ("decimotercer", "ADJ", {"NumType": "Ord"}),
        ("mejores", "ADJ", {"Degree": "Cmp", "Number": "Plur"}),
        ("primera", "ADJ", {"NumType": "Ord", "Gender": "Fem"}),
        ("máximo", "ADJ", {"Degree": "Sup"}),
        ("dos", "NUM", {"NumType": "Card", "Number": "Plur"}),
        ("treintaiuno", "NUM", {"NumType": "Card", "Number": "Plur"}),
        ("doscientas", "NUM", {"NumType": "Card", "Gender": "Fem"}),
    ],
)
def test_lexical_features(morph, word, pos, feats):
    assert any(a.pos == pos and feats.items() <= a.feats.items() for a in morph.analyze(word)), (
        word
    )


def test_clitics_have_their_case_apart(morph):
    cases = {a.feats.get("Case") for a in morph.analyze("se")}
    assert cases == {"Acc", "Dat", None}
    assert morph.analyze("le")[0].feats["Case"] == "Dat"


def test_auxiliaries_are_also_verbs(morph):
    for word, lemma in [("ha", "haber"), ("fue", "ser"), ("puede", "poder")]:
        pairs = {(a.lemma, a.pos) for a in morph.analyze(word)}
        assert {(lemma, "AUX"), (lemma, "VERB")} <= pairs, word
    assert "AUX" not in {a.pos for a in morph.analyze("come")}


def test_participles_are_also_adjectives(morph):
    analyses = {a.pos: a for a in morph.analyze("compradas") if a.pos in ("VERB", "ADJ")}
    assert analyses["VERB"].lemma == "comprar"
    assert analyses["VERB"].feats["Tense"] == "Past"
    assert analyses["ADJ"].lemma == "comprado"
    assert analyses["ADJ"].feats == {"Gender": "Fem", "Number": "Plur", "VerbForm": "Part"}
    adjectives = [a for a in morph.analyze("cansado") if a.pos == "ADJ"]
    assert [a.feats.get("VerbForm") for a in adjectives] == ["Part"]


@pytest.mark.parametrize(
    "word, gender, number",
    [
        ("grandísimo", "Masc", "Sing"),
        ("grandísima", "Fem", "Sing"),
        ("grandísimos", "Masc", "Plur"),
        ("grandísimas", "Fem", "Plur"),
    ],
)
def test_absolute_superlatives(morph, word, gender, number):
    adjective = next(a for a in morph.analyze(word) if a.pos == "ADJ")
    assert adjective.lemma == "grandísimo"
    assert adjective.feats == {"Degree": "Abs", "Gender": gender, "Number": number}


@pytest.mark.parametrize(
    "query, word",
    [
        ("fotografia", "fotografía"),
        ("autonoma", "autónoma"),
        ("catolicos", "católicos"),
        ("raices", "raíces"),
    ],
)
def test_a_lost_accent_finds_the_common_spelling(morph, query, word):
    analyses = morph.analyze(query)
    assert analyses[0].word == word, query
    assert {a.word for a in analyses} != {word}, query
    probs = [a.prob for a in analyses]
    assert probs == sorted(probs, reverse=True), query
    assert sum(probs) == pytest.approx(1, abs=1e-3), query


@pytest.mark.parametrize(
    "query, word",
    [
        ("pokemon", "pokémon"),
        ("habitues", "habitúes"),
    ],
)
def test_spellings_without_counts_keep_their_order(morph, query, word):
    analyses = morph.analyze(query)
    assert analyses[0].word == word, query
    assert len({a.prob for a in analyses}) == 1, query


def test_no_analysis_is_impossible(morph):
    assert all(a.prob > 0 for word in ["la", "sabe", "de", "que"] for a in morph.analyze(word))


def test_numerals_keep_their_gender(morph):
    assert any(a.feats.get("Gender") == "Fem" for a in morph.analyze("cincuentaiuna"))


def test_abbreviations_of_usted(morph):
    for word in ["ud", "uds", "vd", "vds"]:
        assert "tú" in lemmas(morph, word), word


def test_names_have_no_features(morph):
    names = [a for a in morph.analyze("madrid") if a.pos == "PROPN"]
    assert names and all(a.feats == {} for a in names)


def test_closed_class_out_of_the_table_only_without_other_analyses(morph):
    assert "ADP" not in {a.pos for a in morph.analyze("hace")}
    assert "ADP" not in {a.pos for a in morph.analyze("debajo")}
    assert "ADP" in {a.pos for a in morph.analyze("apud")}


@pytest.mark.parametrize(
    "word, lemma, pos",
    [
        ("casa", "casa", "NOUN"),
        ("la", "el", "DET"),
        ("ha", "haber", "AUX"),
        ("sobre", "sobre", "ADP"),
        ("sólo", "sólo", "ADV"),
    ],
)
def test_the_most_probable_analysis_is_first(morph, word, lemma, pos):
    first = morph.analyze(word)[0]
    assert (first.lemma, first.pos) == (lemma, pos)


def test_most_common_features_are_first(morph):
    assert morph.analyze("come")[0].feats["Mood"] == "Ind"


def test_probabilities(morph):
    for word in ["casa", "la", "que", "como", "fue", "publico", "se"]:
        probs = [a.prob for a in morph.analyze(word)]
        assert probs == sorted(probs, reverse=True), word
        assert sum(probs) == pytest.approx(1, abs=1e-3), word
    assert morph.analyze("camión")[0].prob == 1
    # No treebank has this word: its analyses are equal and still ordered
    probs = {round(a.prob, 6) for a in morph.analyze("ababillamos")}
    assert len(probs) == 1


def test_table_of_closed_classes():
    rows = (ROOT / "data" / "closed-classes.tsv").read_text(encoding="utf-8").splitlines()
    assert rows == sorted(set(rows))
    for row in rows:
        lemma, pos, form, feats = row.split("\t")
        assert lemma == lemma.lower(), row
        assert form == form.lower(), row
        assert pos in {"ADJ", "ADP", "ADV", "CCONJ", "DET", "NUM", "PRON", "SCONJ"}, row
        names = [] if feats == "_" else [feat.split("=")[0] for feat in feats.split("|")]
        assert names == sorted(names, key=str.lower), row


def test_canonical_rows_are_the_word_itself(morph):
    assert morph.analyze("www.rae.es") == []
    assert morph.analyze("tdhle") == []


@pytest.mark.parametrize("word", ["", " ", "qzxw", "­", "123", "a b", "\x00", "\ud83d"])
def test_unknown_words(morph, word):
    assert morph.analyze(word) == []
    assert not morph.is_known(word)


def test_is_known(morph):
    assert morph.is_known("Camión")
    assert morph.is_known("camion")
    assert not morph.is_known("qzxw")


def test_analyze_many(morph):
    result = morph.analyze_many(["camiones", "qzxw", "hablo"])
    assert [analysis.lemma for analysis in result[0]] == ["camión"]
    assert result[1] == []
    assert result[2] == morph.analyze("hablo")
    assert morph.analyze_many([]) == []
    assert [len(analyses) for analyses in morph.analyze_many(iter(["hablo"]))] == [
        len(morph.analyze("hablo"))
    ]
    assert len(morph.analyze_many(["hablo"] * 20_000)) == 20_000


def test_lemmatize(morph):
    assert morph.lemmatize(["CAMIONES", "Qzxw"]) == ["camión", "qzxw"]
    assert morph.lemmatize(iter(["hablo"])) == ["hablar"]
    assert morph.lemmatize([]) == []
    words = morph.lemmatize(["hablo"] * 20_000)
    assert len(words) == 20_000
    # Equal lemmas are one object
    assert words[0] is words[-1]


@pytest.mark.parametrize("words", ["casa", 7, None])
@pytest.mark.parametrize("method", ["analyze_many", "lemmatize"])
def test_collections_of_words(morph, method, words):
    with pytest.raises(TypeError, match="collection of words"):
        getattr(morph, method)(words)


@pytest.mark.parametrize("method", ["analyze_many", "lemmatize"])
def test_words_must_be_strings(morph, method):
    with pytest.raises(TypeError, match="item 1 is int"):
        getattr(morph, method)(["casa", 7])


@pytest.mark.parametrize("word", [7, None, b"casa", ["casa"]])
@pytest.mark.parametrize("method", ["analyze", "is_known"])
def test_a_word_must_be_a_string(morph, method, word):
    with pytest.raises(TypeError, match="A word must be a string"):
        getattr(morph, method)(word)


def test_surrogates(morph):
    assert morph.analyze("casa\udc80") == []
    assert morph.lemmatize(["casa\udc80"]) == ["casa�"]
    assert morph.analyze_many(["\ud83d"]) == [[]]


def test_analysis_is_immutable(morph):
    analysis = morph.analyze("camión")[0]
    with pytest.raises(AttributeError):
        analysis.lemma = "x"
    # The features of an analysis are a copy
    analysis.feats["Gender"] = "Fem"
    assert analysis.feats["Gender"] == "Masc"


def test_analysis_equality_and_hash(morph):
    first, second = morph.analyze("camión")[0], morph.analyze("camión")[0]
    assert first == second
    assert hash(first) == hash(second)
    assert first != morph.analyze("camiones")[0]
    assert first != "camión"
    assert len({first, second}) == 1


def test_analysis_repr(morph):
    assert repr(morph.analyze("camión")[0]) == (
        "Analysis(word='camión', lemma='camión', pos='NOUN', "
        "feats={'Gender': 'Masc', 'Number': 'Sing'}, prob=1.000000, words=('camión',), "
        "source='dictionary')"
    )


def test_analysis_pickle_and_copy(morph):
    analysis = morph.analyze("hablamos")[0]
    for clone in [
        pickle.loads(pickle.dumps(analysis)),
        copy.copy(analysis),
        copy.deepcopy(analysis),
    ]:
        assert clone == analysis
        assert clone.feats == analysis.feats
        assert clone.pos == analysis.pos


def test_analysis_cannot_be_made_with_an_unknown_source():
    with pytest.raises(ValueError, match="Unknown source"):
        esmorph._esmorph._analysis("a", "a", "NOUN", "", 1.0, [], "other")


def test_analyzer_pickle(morph, tmp_path):
    clone = pickle.loads(pickle.dumps(morph))
    assert clone.path == morph.path
    assert clone.analyze("camión") == morph.analyze("camión")
    path = tmp_path / "es.dict"
    path.write_bytes(morph.path.read_bytes())
    explicit = pickle.loads(pickle.dumps(Analyzer(path)))
    assert explicit.path == path


def test_analyzer_repr_and_path(morph, tmp_path):
    assert morph.path.name == "es.dict"
    assert repr(morph) == f"Analyzer(path={str(morph.path)!r})"
    path = tmp_path / "es.dict"
    path.write_bytes(morph.path.read_bytes())
    assert Analyzer(str(path)).path == path
    assert Analyzer(path).is_known("camión")


def test_dictionary_description(morph):
    meta = json.loads(morph._meta())
    assert meta["source"] == "Wiktionary"
    assert meta["counts"]["release"] == "r2.18"
    assert set(meta["counts"]["treebanks"]) == {"UD_Spanish-AnCora", "UD_Spanish-GSD"}
    assert meta["license"] == "CC BY-SA 4.0"


def test_load_errors(morph, tmp_path):
    with pytest.raises(FileNotFoundError) as error:
        Analyzer(tmp_path / "none.dict")
    assert error.value.errno == errno.ENOENT
    assert error.value.filename == str(tmp_path / "none.dict")
    # Opening a directory is `IsADirectoryError` on POSIX and `PermissionError` on Windows
    with pytest.raises((IsADirectoryError, PermissionError), match="is a directory"):
        Analyzer(tmp_path)
    not_dictionary = tmp_path / "text.dict"
    not_dictionary.write_text("not a dictionary, long enough to have a header")
    with pytest.raises(ValueError, match="signature"):
        Analyzer(not_dictionary)
    damaged = tmp_path / "damaged.dict"
    data = bytearray(morph.path.read_bytes())
    data[len(data) // 2] ^= 0x10
    damaged.write_bytes(data)
    with pytest.raises(ValueError, match="checksum"):
        Analyzer(damaged)
    empty = tmp_path / "empty.dict"
    empty.write_bytes(b"")
    with pytest.raises(ValueError, match="signature"):
        Analyzer(empty)


def test_threads(morph):
    expected = morph.analyze_many(["hablamos", "camiones", "qzxw"] * 1000)
    results = []

    def work():
        results.append(morph.analyze_many(["hablamos", "camiones", "qzxw"] * 1000))

    threads = [threading.Thread(target=work) for _ in range(8)]
    for thread in threads:
        thread.start()
    for thread in threads:
        thread.join()
    assert len(results) == 8
    assert all(result == expected for result in results)


def test_dictionary_covers_common_words(morph):
    words = [
        "el",
        "gobierno",
        "ha",
        "dicho",
        "que",
        "los",
        "ciudadanos",
        "pueden",
        "votar",
        "en",
        "las",
        "elecciones",
        "de",
        "este",
        "año",
        "porque",
        "la",
        "ley",
        "no",
        "lo",
        "prohíbe",
        "y",
        "nadie",
        "sabe",
        "cuándo",
        "terminará",
        "todo",
        "esto",
    ]
    assert [word for word in words if not morph.is_known(word)] == []
