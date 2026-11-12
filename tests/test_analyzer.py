import contextlib
import copy
import errno
import gc
import itertools
import os
import pickle
import subprocess
import sys
import threading
import zlib
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
        assert morph.analyze(word)[0].lemma == "camión"
    # A word with a capital letter may also be a name, after the analyses of the dictionary
    assert [(a.lemma, a.source) for a in morph.analyze("Camión")] == [
        ("camión", "dictionary"),
        ("Camión", "rule"),
    ]
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
    assert morph.analyze("año")[0].lemma == "año"
    assert "ano" in lemmas(morph, "ano")
    assert "año" not in lemmas(morph, "ano")


def test_normalized_queries(morph):
    assert lemmas(morph, "camión") == ["camión"]
    assert morph.analyze("CAMIÓN")[0].lemma == "camión"
    assert morph.analyze("año")[0].lemma == "año"
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
        ("damelo", "dar", ("da", "me", "lo")),
        ("hacerlo", "hacer", ("hacer", "lo")),
        ("diciéndoselo", "decir", ("diciendo", "se", "lo")),
        ("oírlo", "oír", ("oír", "lo")),
        ("déme", "dar", ("dé", "me")),
        ("vámonos", "ir", ("vamos", "nos")),
        ("digámoselo", "decir", ("digamos", "se", "lo")),
        ("sentaos", "sentar", ("sentad", "os")),
        ("cuéntenos", "contar", ("cuente", "nos")),
        ("piénselo", "pensar", ("piense", "lo")),
        ("debátelo", "debatir", ("debate", "lo")),
        ("represéntelo", "representar", ("represente", "lo")),
        ("deme", "dar", ("dé", "me")),
        ("dele", "dar", ("dé", "le")),
        ("estate", "estar", ("está", "te")),
        ("sonreírle", "sonreír", ("sonreír", "le")),
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
    for word in ["casa", "la", "que", "como", "fue", "publico", "se", "sí", "madrina", "Doble"]:
        probs = [a.prob for a in morph.analyze(word)]
        assert probs == sorted(probs, reverse=True), word
        # The shares are stored in millionths, but the sum is 1
        assert sum(probs) == pytest.approx(1, abs=1e-12), word
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
    # An address of an entry is no word of the dictionary: a rule gives it
    assert [a.source for a in morph.analyze("www.rae.es")] == ["rule"]
    assert not morph.is_known("www.rae.es")
    assert not morph.is_known("tdhle")


@pytest.mark.parametrize("word", ["", " ", "qzxw", "­", "a b", "\x00", "\ud83d", "1 00"])
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


def test_analyze_sentence(morph):
    sentence = morph.analyze_sentence(["Ella", "la", "vio", "ayer", "."])
    assert len(sentence) == 5
    # The same analyses as analyze, in the order of the model, with its probabilities
    for word, analyses in zip(["Ella", "la", "vio", "ayer", "."], sentence, strict=True):
        assert {(a.lemma, a.pos, tuple(a.feats.items())) for a in analyses} == {
            (a.lemma, a.pos, tuple(a.feats.items())) for a in morph.analyze(word)
        }
        assert abs(sum(analysis.prob for analysis in analyses) - 1) < 1e-6
        assert [analysis.prob for analysis in analyses] == sorted(
            (analysis.prob for analysis in analyses), reverse=True
        )
    # The article and the pronoun by the context
    assert sentence[1][0].pos == "PRON"
    assert morph.analyze_sentence(["la", "casa"])[0][0].pos == "DET"
    # Without neighbors a token is analyzed as analyze; a sentence of none is empty
    assert morph.analyze_sentence(["la"]) == [morph.analyze("la")]
    assert morph.analyze_sentence([]) == []
    assert morph.analyze_sentence(iter(["la", "casa"])) == morph.analyze_sentence(["la", "casa"])
    # Multiword tokens are analyzed by the token, a token without analyses has none
    first = morph.analyze_sentence(["Dámelo", "ya"])[0][0]
    assert (first.pos, first.words) == ("VERB", ("da", "me", "lo"))
    assert morph.analyze_sentence(["qzxw", "casa"])[0] == []
    # A long sentence goes by parts of 256 tokens, and only its first part starts it: the part
    # after a piece read without the GIL is analyzed as the same part inside the first piece
    long = morph.analyze_sentence(["la", "casa", "es", "grande"] * 3000)
    assert len(long) == 12_000

    def probabilities(part):
        return [[(analysis.pos, analysis.prob) for analysis in analyses] for analyses in part]

    assert probabilities(long[8192:8448]) == probabilities(long[4096:4352])
    assert probabilities(long[:256]) != probabilities(long[4096:4352])


def test_model_as_trained(morph):
    # Sentences of AnCora dev with the order and the probabilities of the quantized model of the
    # training: the analyzer computes the input of the model and its layers as the training does
    sentences: dict[int, dict[int, tuple[str, list]]] = {}
    for line in (ROOT / "data" / "es-context-check.tsv").read_text(encoding="utf-8").splitlines():
        sentence, token, form, *fields = line.split("\t")
        found = sentences.setdefault(int(sentence), {}).setdefault(int(token), (form, []))[1]
        if fields:
            pos, lemma, feats, words, source, prob = fields
            found.append(((pos, lemma, feats, int(words), source), float(prob)))
    assert len(sentences) >= 20
    for tokens in sentences.values():
        forms = [form for form, _ in tokens.values()]
        analyses = morph.analyze_sentence(forms)
        for (form, expected), found in zip(tokens.values(), analyses, strict=True):
            probs = dict(expected)
            tags = ["|".join(f"{k}={v}" for k, v in sorted(a.feats.items())) or "_" for a in found]
            keys = [
                (a.pos, a.lemma, tag, len(a.words), a.source)
                for a, tag in zip(found, tags, strict=True)
            ]
            assert sorted(keys) == sorted(probs), form
            for key, analysis in zip(keys, found, strict=True):
                assert abs(analysis.prob - probs[key]) < 1e-4, (form, key)
            # The order of the training, except for probabilities that are almost equal
            ordered = [probs[key] for key in keys]
            assert all(a >= b - 1e-4 for a, b in itertools.pairwise(ordered)), form


def test_lemmatize(morph):
    assert morph.lemmatize(["CAMIONES", "Qzxw", "qzxw", "10.000"]) == [
        "camión",
        "Qzxw",
        "qzxw",
        "10000",
    ]
    assert morph.lemmatize(iter(["hablo"])) == ["hablar"]
    assert morph.lemmatize([]) == []
    words = morph.lemmatize(["hablo"] * 20_000)
    assert len(words) == 20_000
    # Equal lemmas are one object
    assert words[0] is words[-1]


@pytest.mark.parametrize("words", ["casa", b"casa", bytearray(b"casa"), 7, None])
@pytest.mark.parametrize("method", ["analyze_many", "lemmatize", "analyze_sentence"])
def test_collections_of_words(morph, method, words):
    with pytest.raises(TypeError, match="collection of words"):
        getattr(morph, method)(words)


@pytest.mark.parametrize("method", ["analyze_many", "lemmatize", "analyze_sentence"])
def test_words_must_be_strings(morph, method):
    with pytest.raises(TypeError, match="item 1 is int"):
        getattr(morph, method)(["casa", 7])

    # Words are read in chunks: the number counts from the start of the input
    def words():
        yield from ["casa"] * 10_000
        yield 7

    with pytest.raises(TypeError, match="item 10000 is int"):
        getattr(morph, method)(words())


@pytest.mark.parametrize("method", ["analyze_many", "lemmatize", "analyze_sentence"])
def test_an_ended_input_is_not_asked_again(morph, method):
    class Words:
        calls = 0

        def __iter__(self):
            return self

        def __next__(self):
            self.calls += 1
            if self.calls > 3:
                raise StopIteration
            return "casa"

    words = Words()
    assert len(getattr(morph, method)(words)) == 3
    assert words.calls == 4


def test_pickled_analyses_keep_their_tables(morph):
    forms = morph.analyze("zumbeábamos")[0].forms()
    assert all(pickle.loads(pickle.dumps(form)).forms() == forms for form in forms)
    # A pickle without the table, or with the table of another dictionary, finds it on first use
    state = forms[0].__reduce__()[1]
    other = state[8] ^ (1 << 32)
    for restored in [state[:8], (*state[:8], other)]:
        assert esmorph._esmorph._analysis(*restored).forms() == forms


def test_analyses_are_not_tracked_by_the_garbage_collector(morph):
    analyses = [
        *morph.analyze("casa"),
        *morph.analyze_many(["gatos"])[0],
        *morph.analyze("gatos")[0].forms(),
        pickle.loads(pickle.dumps(morph.analyze("casa")[0])),
    ]
    assert not any(gc.is_tracked(analysis) for analysis in analyses)
    # Nor are the lists of analyze_many, which hold only analyses
    result = morph.analyze_many(["gatos", "qzxw"])
    assert gc.is_tracked(result) and not any(gc.is_tracked(analyses) for analyses in result)


@pytest.mark.parametrize("word", [7, None, b"casa", ["casa"]])
@pytest.mark.parametrize("method", ["analyze", "is_known"])
def test_a_word_must_be_a_string(morph, method, word):
    with pytest.raises(TypeError, match="A word must be a string"):
        getattr(morph, method)(word)


def test_surrogates(morph):
    assert morph.analyze("casa\udc80") == []
    # A token without analyses comes back in lower case as it is, with its lone surrogates
    assert morph.lemmatize(["casa\udc80", "CA\udcffSA"]) == ["casa\udc80", "ca\udcffsa"]
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
        "feats={'Gender': 'Masc', 'Number': 'Sing'}, prob=1.0, words=('camión',), "
        "source='dictionary')"
    )
    # Six significant digits, so a tiny probability is not zero
    tiny = esmorph._esmorph._analysis("a", "a", "NOUN", "", 1.234567e-7, [], "rule")
    assert "prob=1.23457e-07," in repr(tiny)


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
    info = morph.dictionary_info
    assert set(info) == {
        "source",
        "snapshot",
        "sha256",
        "license",
        "ud_release",
        "treebanks",
        "context",
        "word_forms",
        "checksum",
    }
    assert info["source"] == "Wiktionary"
    assert info["ud_release"] == "r2.18"
    assert info["treebanks"] == {
        "UD_Spanish-AnCora": "CC BY 4.0",
        "UD_Spanish-GSD": "CC BY-SA 4.0",
    }
    assert info["context"] == {"ud_release": "r2.18", "treebanks": info["treebanks"]}
    assert info["license"] == "CC BY-SA 4.0"
    assert info["snapshot"].endswith(".jsonl.xz") and len(info["sha256"]) == 64
    assert info["word_forms"] > 600_000 and 0 <= info["checksum"] < 2**32
    # A new dictionary every time
    info["source"] = "x"
    assert morph.dictionary_info["source"] == "Wiktionary"


def test_dictionary_description_of_another_builder(morph, tmp_path):
    # The description is the first section of the file, after the header with the checksum
    data = morph.path.read_bytes()
    size = int.from_bytes(data[16:24], "little")
    rest = data[24 + (size + 7) // 8 * 8 :]

    def described(meta: bytes) -> Analyzer:
        body = len(meta).to_bytes(8, "little") + meta + b"\0" * (-len(meta) % 8) + rest
        path = tmp_path / f"{zlib.crc32(meta)}.dict"
        path.write_bytes(data[:12] + zlib.crc32(body).to_bytes(4, "little") + body)
        return Analyzer(path)

    meta = b'{"source": 1, "license": ["a"], "sha256": "x", "counts": {"release": 2, "treebanks": {"a": 1}}, "context": []}'
    info = described(meta).dictionary_info
    assert set(info) == {"sha256", "context", "word_forms", "checksum"}
    assert info["context"] == {}
    assert set(described(b"not json").dictionary_info) == {"context", "word_forms", "checksum"}


def without_model(data: bytes) -> bytes:
    """A dictionary file with an empty last section, the context model"""
    at = 16
    for _ in range(13):
        at += 8 + (int.from_bytes(data[at : at + 8], "little") + 7) // 8 * 8
    body = data[16:at] + bytes(8)
    return data[:12] + zlib.crc32(body).to_bytes(4, "little") + body


def test_dictionary_without_model(morph, tmp_path):
    path = tmp_path / "es.dict"
    path.write_bytes(without_model(morph.path.read_bytes()))
    plain = Analyzer(path, verify=True)
    assert "context" not in plain.dictionary_info
    assert plain.analyze("la") == morph.analyze("la")
    for words in (["la", "casa"], []):
        with pytest.raises(ValueError, match="no context model"):
            plain.analyze_sentence(words)


def test_dictionary_paths(morph, tmp_path, monkeypatch):
    path = tmp_path / "es.dict"
    path.write_bytes(morph.path.read_bytes())
    monkeypatch.chdir(tmp_path)
    # A relative path is kept absolute, so pickle restores the analyzer from any directory
    relative = Analyzer("es.dict")
    assert relative.path == Path.cwd() / "es.dict"
    analysis = relative.analyze("gatos")[0]
    monkeypatch.chdir(ROOT)
    assert pickle.loads(pickle.dumps(relative)).path == relative.path
    assert pickle.loads(pickle.dumps(analysis)).inflect(Number="Sing") == "gato"
    with pytest.raises(ValueError, match="NUL"):
        Analyzer("es\0.dict")
    if hasattr(os, "mkfifo"):
        os.mkfifo(tmp_path / "pipe")
        with pytest.raises(OSError, match="not a file") as error:
            Analyzer(tmp_path / "pipe")
        assert (error.value.errno, error.value.filename) == (errno.EINVAL, str(tmp_path / "pipe"))


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


def crafted(data: bytes, at: int, flip: int) -> bytes:
    """The dictionary with a byte of its automaton of word forms changed and the checksum anew"""
    # Sections after the header of 16 bytes: a length of 8 bytes, the data, alignment to 8
    start = 16
    for _ in range(8):
        length = int.from_bytes(data[start : start + 8], "little")
        start = (start + 8 + length + 7) // 8 * 8
    length = int.from_bytes(data[start : start + 8], "little")
    damaged = bytearray(data)
    damaged[start + 8 + length - at] ^= flip
    damaged[12:16] = zlib.crc32(bytes(damaged[16:])).to_bytes(4, "little")
    return bytes(damaged)


def test_verify(morph, tmp_path):
    assert Analyzer(verify=True).analyze("gatos")[0].lemma == "gato"
    # A crafted automaton with a right checksum loads; verify rejects it
    data, path = morph.path.read_bytes(), tmp_path / "crafted.dict"
    for at, flip in itertools.product(range(22, 80), (0x08, 0x10, 0x40)):
        path.write_bytes(crafted(data, at, flip))
        try:
            Analyzer(path)
        except ValueError:
            continue
        with contextlib.suppress(ValueError):
            Analyzer(path, verify=True)
            continue
        break
    else:
        pytest.fail("no crafted dictionary that loads and fails verify")
    with pytest.raises(ValueError, match="the automaton of word forms is damaged"):
        Analyzer(path, verify=True)
    with pytest.raises(TypeError):
        Analyzer(morph.path, True)
    # Pickle keeps the check: an analyzer or an analysis of a verified dictionary is restored with
    # it, so a file replaced by a crafted one is rejected, even if the process has it unverified
    good = tmp_path / "good.dict"
    good.write_bytes(data)
    verified = Analyzer(good, verify=True)
    assert verified.__reduce__()[1] == (str(good), True)
    assert pickle.loads(pickle.dumps(verified)) is verified
    analysis = verified.analyze("gatos")[0]
    state = analysis.__reduce__()[1]
    assert state[-1] is True
    with pytest.raises(ValueError, match="the automaton of word forms is damaged"):
        esmorph._esmorph._analyzer(path, True)
    assert esmorph._esmorph._analyzer(path).path == path
    # An analyzer verified after its analyses were pickled pickles them verified
    later = tmp_path / "later.dict"
    later.write_bytes(data)
    unverified = esmorph._esmorph._analyzer(later)
    unverified.analyze("gatos")[0].__reduce__()
    assert esmorph._esmorph._analyzer(later, True) is unverified
    saved = unverified.analyze("gatos")[0].__reduce__()[1][7]
    assert saved.__reduce__()[1] == (str(later), True)
    for restore in (
        lambda: esmorph._esmorph._analyzer(path, True),
        lambda: esmorph._esmorph._analysis(*state[:7], path, None, True),
    ):
        with pytest.raises(ValueError, match="the automaton of word forms is damaged"):
            restore()


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


def verb(morph, word):
    return next(analysis for analysis in morph.analyze(word) if analysis.pos == "VERB")


def test_forms_of_a_lexeme(morph):
    gato = morph.analyze("gatos")[0]
    forms = gato.forms()
    assert [form.word for form in forms] == ["gato", "gata", "gatos", "gatas"]
    assert all(form.lemma == "gato" and form.pos == "NOUN" for form in forms)
    assert all(form.prob == 1.0 and form.source == "dictionary" for form in forms)
    assert forms[3].feats == {"Gender": "Fem", "Number": "Plur"}
    # An analysis equals its form: the probability takes no part in the comparison
    assert gato in forms
    assert [form.word for form in morph.analyze("bonitas")[0].forms()] == [
        "bonito",
        "bonita",
        "bonitos",
        "bonitas",
    ]


def test_forms_of_a_verb_go_in_the_order_of_the_paradigm(morph):
    forms = verb(morph, "hablamos").forms()
    words = [form.word for form in forms]
    assert words[:8] == [
        "hablar",
        "hablando",
        "hablado",
        "hablada",
        "hablados",
        "habladas",
        "hablo",
        "hablas",
    ]
    # The variants follow the main paradigm: the forms of vos and old spellings
    main = forms[: words.index("fablar")]
    order = {"Inf": 0, "Ger": 1, "Part": 2, "Fin": 3}
    places = [order[form.feats["VerbForm"]] for form in main]
    assert places == sorted(places)
    moods = [form.feats["Mood"] for form in main if "Mood" in form.feats]
    assert moods.index("Sub") > moods.index("Cnd") > moods.index("Ind")
    assert moods[-1] == "Imp"
    present = [
        form.word
        for form in main
        if form.feats.get("Tense") == "Pres" and form.feats.get("Mood") == "Ind"
    ]
    assert present == ["hablo", "hablas", "habla", "hablamos", "habláis", "hablan"]
    assert {"fablar", "hablás", "hablá"} <= set(words[len(main) :])


def test_inflect_keeps_the_other_features(morph):
    assert morph.analyze("gata")[0].inflect(Number="Plur") == "gatas"
    assert morph.analyze("bonita")[0].inflect(Gender="Masc") == "bonito"
    assert morph.analyze("el")[0].inflect(Gender="Fem", Number="Plur") == "las"
    hablamos = next(a for a in morph.analyze("hablamos") if a.feats.get("Tense") == "Pres")
    assert hablamos.inflect(Number="Sing") == "hablo"
    assert hablamos.inflect(Mood="Sub") == "hablemos"
    assert hablamos.inflect() == "hablamos"
    # An adjective of both genders agrees with a noun of either
    assert morph.analyze("grande")[0].inflect(Gender="Fem", Number="Plur") == "grandes"
    assert morph.analyze("nacional")[0].inflect(Gender="Masc") == "nacional"


def test_inflect_from_the_infinitive(morph):
    hablar = verb(morph, "hablar")
    assert hablar.inflect(Tense="Past", Person="3", Number="Plur") == "hablaron"
    assert hablar.inflect(Tense="Past") == "hablé"
    assert hablar.inflect(Mood="Sub", Tense="Pres", Person="1", Number="Plur") == "hablemos"
    assert hablar.inflect(Mood="Imp") == "habla"
    assert hablar.inflect(VerbForm="Ger") == "hablando"
    assert hablar.inflect(VerbForm="Part", Gender="Fem", Number="Plur") == "habladas"


@pytest.mark.parametrize(
    ("word", "lemma", "feats", "form"),
    [
        ("es", "ser", {"Tense": "Past"}, "fue"),
        ("fue", "ir", {"Tense": "Imp"}, "iba"),
        ("puse", "poner", {"Tense": "Fut"}, "pondré"),
        ("tengo", "tener", {"Mood": "Sub"}, "tenga"),
        ("dijo", "decir", {"VerbForm": "Part"}, "dicho"),
        ("pienso", "pensar", {"Number": "Plur"}, "pensamos"),
        ("juego", "jugar", {"Mood": "Sub", "Person": "3"}, "juegue"),
        ("yo", "yo", {"Number": "Plur"}, "nosotros"),
        ("él", "él", {"Gender": "Fem"}, "ella"),
    ],
)
def test_inflect_irregular_words(morph, word, lemma, feats, form):
    analysis = next(a for a in morph.analyze(word) if a.lemma == lemma and a.pos != "AUX")
    assert analysis.inflect(**feats) == form


def test_inflect_gives_a_variant_only_for_a_variant(morph):
    assert verb(morph, "hablas").inflect(Mood="Imp") == "habla"
    assert verb(morph, "hablás").inflect(Mood="Imp") == "hablá"
    # Also for a verb whose variant changes the stem, as the vos forms of `seguir`
    sigas = verb(morph, "sigás")
    assert (sigas.inflect(Mood="Imp"), sigas.inflect(Mood="Ind")) == ("seguí", "seguís")
    assert morph.analyze("grandes")[0].inflect(Number="Sing") == "grande"
    # Of the two forms of the imperfect subjunctive the analysis keeps its own
    hablase = verb(morph, "hablase")
    assert hablase.inflect(Person="1", Number="Plur") == "hablásemos"
    assert verb(morph, "hablara").inflect(Person="1", Number="Plur") == "habláramos"


def test_a_verb_with_clitics_inflects_as_its_verb(morph):
    token = morph.analyze("dámelo")[0]
    assert token.forms()[0].word == "dar"
    assert all(form.words == (form.word,) for form in token.forms())
    assert token.inflect(Number="Plur") == "dad"


def test_inflect_errors(morph):
    gato = morph.analyze("gato")[0]
    with pytest.raises(esmorph.FormNotFoundError, match="'gato' has no form Tense=Past"):
        gato.inflect(Tense="Past")
    assert issubclass(esmorph.FormNotFoundError, LookupError)
    with pytest.raises(
        ValueError, match=r"Unknown value of the feature Number: Dual; .*Plur, Sing"
    ):
        gato.inflect(Number="Dual")
    with pytest.raises(ValueError, match=r"Unknown UD feature Animacy; .*Gender"):
        gato.inflect(Animacy="Anim")
    with pytest.raises(TypeError, match="Number must be a string, not int"):
        gato.inflect(Number=1)


def test_pickle_keeps_the_analyzer(morph):
    analyses = morph.analyze("gatos") + morph.analyze("hablamos")
    restored = pickle.loads(pickle.dumps(analyses))
    assert restored == analyses
    assert [form.word for form in restored[0].forms()][:2] == ["gato", "gata"]
    assert restored[-1].inflect(Number="Sing") == analyses[-1].inflect(Number="Sing")
    # Analyses unpickled one by one share the analyzer of the package
    singles = [pickle.loads(pickle.dumps(analysis)) for analysis in analyses]
    assert singles == analyses
    assert pickle.loads(pickle.dumps(morph)) is pickle.loads(pickle.dumps(Analyzer()))


def test_pickle_takes_the_analyzer_of_a_file(morph, tmp_path):
    path = tmp_path / "es.dict"
    path.write_bytes(morph.path.read_bytes())
    own = Analyzer(path)
    assert pickle.loads(pickle.dumps(own)) is own
    analysis = pickle.loads(pickle.dumps(own.analyze("gatos")[0]))
    assert analysis.inflect(Gender="Fem") == "gatas"
    # The path is a string: a path of pathlib of one system does not unpickle on another
    assert b"pathlib" not in pickle.dumps(own) + pickle.dumps(own.analyze("gatos"))
    # The analyses of a file that is gone come with the dictionary of the package
    gone = esmorph._esmorph._analysis(
        "gatos", "gato", "NOUN", "Gender=Masc|Number=Plur", 1.0, [], "dictionary", tmp_path / "x"
    )
    assert gone.inflect(Number="Sing") == "gato"
    with pytest.raises(FileNotFoundError):
        esmorph._esmorph._analyzer(tmp_path / "x")
    # A list keeps the analyzer once; a pickle with the path of the file, of an earlier version,
    # takes the analyzer of the file too
    analyses = own.analyze_many(["gatos", "hablamos", "casa"] * 100)
    data = pickle.dumps(analyses)
    assert data.count(path.parent.name.encode()) == 1
    # The analyses of a tag share its strings
    assert data.count(b"Gender=Masc|Number=Plur") == 1
    assert pickle.loads(data) == analyses
    older = esmorph._esmorph._analysis(
        "gatos", "gato", "NOUN", "Gender=Masc|Number=Plur", 1.0, [], "dictionary", str(path)
    )
    assert older.__reduce__()[1][7].__reduce__()[1] == (str(path), False)
    assert pickle.loads(pickle.dumps(older)) == older
    # Features of an earlier version in another order: the analysis has the tag of the dictionary
    ordinal = esmorph._esmorph._analysis(
        "2.ª", "2.º", "ADJ", "Gender=Fem|NumForm=Digit|NumType=Ord|Number=Sing", 1.0, [], "rule"
    )
    assert ordinal == morph.analyze("2.ª")[0]
    # Analyses of a file that is gone come with the dictionary of the package
    gone_path = tmp_path / "gone.dict"
    gone_path.write_bytes(morph.path.read_bytes())
    data = pickle.dumps(Analyzer(gone_path).analyze_many(["gatos", "hablamos"]))
    gc.collect()
    gone_path.unlink()
    restored = pickle.loads(data)
    assert restored[0][0].inflect(Number="Sing") == "gato"
    # And of a file of another version of the format, which no analyzer reads
    older_path = tmp_path / "older.dict"
    older_path.write_bytes(morph.path.read_bytes())
    data = pickle.dumps(Analyzer(older_path).analyze_many(["gatos"]))
    gc.collect()
    replaced = tmp_path / "replaced.dict"
    content = bytearray(morph.path.read_bytes())
    content[8:12] = (4).to_bytes(4, "little")
    replaced.write_bytes(content)
    replaced.replace(older_path)
    with pytest.raises(ValueError, match="format version 4"):
        Analyzer(older_path)
    restored = pickle.loads(data)[0][0]
    assert restored.__reduce__()[1][7].__reduce__()[1] == (None, False)
    assert restored.inflect(Number="Sing") == "gato"


THREADS = """
import pickle, sys, threading
from esmorph import Analyzer

def work(path):
    for _ in range(30):
        pickle.loads(pickle.dumps(Analyzer(path).analyze("gatos")))

threads = [threading.Thread(target=work, args=(path,)) for path in sys.argv[1:]]
for thread in threads:
    thread.start()
for thread in threads:
    thread.join()
"""


def test_pickle_from_threads(morph, tmp_path):
    # Analyzers of files pickled from several threads at once in a new interpreter, where the first
    # pickle of a path imports pathlib: no thread waits for another forever
    paths = []
    for number in range(4):
        paths.append(tmp_path / f"{number}.dict")
        paths[-1].write_bytes(morph.path.read_bytes())
    subprocess.run([sys.executable, "-c", THREADS, *map(str, paths)], check=True, timeout=60)


def test_an_analysis_of_another_dictionary_has_no_forms(morph):
    lost = esmorph._esmorph._analysis("gatox", "gato", "NOUN", "", 1.0, [], "dictionary")
    assert lost.forms() == []
    with pytest.raises(esmorph.FormNotFoundError):
        lost.inflect(Number="Plur")
    # Without an analyzer the dictionary of the package is taken
    found = esmorph._esmorph._analysis(
        "gatos", "gato", "NOUN", "Gender=Masc|Number=Plur", 1.0, [], "dictionary"
    )
    assert found.inflect(Number="Sing") == "gato"


def summary(morph, token):
    return [(a.word, a.lemma, a.pos, a.feats, a.source) for a in morph.analyze(token)]


@pytest.mark.parametrize(
    ("token", "feats"),
    [
        (",", {"PunctType": "Comm"}),
        (".", {"PunctType": "Peri"}),
        ("...", {"PunctType": "Elip"}),
        ("«", {"PunctType": "Quot"}),
        ("—", {"PunctType": "Dash"}),
        ("(", {"PunctSide": "Ini", "PunctType": "Brck"}),
        ("?", {"PunctSide": "Fin", "PunctType": "Qest"}),
        ("¡", {"PunctSide": "Ini", "PunctType": "Excl"}),
        ("?!", {}),
    ],
)
def test_punctuation(morph, token, feats):
    assert summary(morph, token) == [(token, token, "PUNCT", feats, "rule")]


def test_symbols_and_numbers(morph):
    assert summary(morph, "%") == [("%", "%", "SYM", {}, "rule")]
    assert summary(morph, "€") == [("€", "€", "SYM", {}, "rule")]
    card = {"NumForm": "Digit", "NumType": "Card"}
    for token, lemma in [
        ("1999", "1999"),
        ("10.000", "10000"),
        ("1.234,5", "1234,5"),
        ("3,14", "3,14"),
        ("15:00", "15:00"),
        ("2-1", "2-1"),
    ]:
        assert summary(morph, token) == [(token, lemma, "NUM", card, "rule")], token


def test_roman_numerals(morph):
    analyses = morph.analyze("XXI")
    assert [(a.pos, a.feats) for a in analyses] == [
        ("NUM", {"NumForm": "Roman", "NumType": "Card"}),
        ("PROPN", {}),
    ]
    assert analyses[0].prob > 0.9
    assert sum(a.prob for a in analyses) == pytest.approx(1.0)


def test_names(morph):
    for token in ["Josep", "CiU", "W.", "TV-3", "Macapagal-Arroyo"]:
        assert summary(morph, token) == [(token, token, "PROPN", {}, "rule")], token
    assert morph.lemmatize(["Clinton", "clinton"]) == ["Clinton", "clinton"]
    # The rules give analyses, not words of the dictionary
    assert not morph.is_known("Josep")
    assert not morph.is_known("1999")


def test_words_in_capitals_may_be_names(morph):
    # A short word in capitals is mostly an acronym
    eta = morph.analyze("ETA")
    assert (eta[0].lemma, eta[0].pos, eta[0].source) == ("ETA", "PROPN", "rule")
    assert eta[1].lemma == "eta"
    assert sum(a.prob for a in eta) == pytest.approx(1.0)
    # A longer word and a word of a closed class are mostly words of a headline
    camiones = morph.analyze("CAMIONES")
    assert (camiones[0].lemma, camiones[-1].pos) == ("camión", "PROPN")
    article = morph.analyze("EL")
    assert article[0].pos == "DET"
    assert any(a.pos == "PROPN" for a in article)
    # A name of the dictionary needs no other
    assert [a.source for a in morph.analyze("PSOE")] == ["dictionary"] * len(morph.analyze("PSOE"))


def test_an_analysis_of_a_rule_is_its_only_form(morph):
    number = morph.analyze("10.000")[0]
    assert number.forms() == [number]
    assert number.forms()[0].prob == 1.0
    assert number.inflect(NumType="Card") == "10.000"
    # A gender or a number it has not is any; another feature is not
    assert number.inflect(Number="Plur") == "10.000"
    with pytest.raises(esmorph.FormNotFoundError):
        number.inflect(NumType="Ord")
    name = morph.analyze("Josep")[0]
    assert pickle.loads(pickle.dumps(name)) == name
    assert pickle.loads(pickle.dumps(name)).forms() == [name]


def test_rules_for_signs_of_the_treebanks(morph):
    assert summary(morph, "/") == [("/", "/", "PUNCT", {"PunctType": "Colo"}, "rule")]
    for sign in ["²", "º", "ª"]:
        assert summary(morph, sign) == [(sign, sign, "SYM", {}, "rule")], sign
    # A long token of Roman letters is no numeral
    assert [a.pos for a in morph.analyze("M" * 100)] == ["PROPN"]


def test_known_roman_numerals_and_names_with_clitics(morph):
    six = morph.analyze("VI")
    assert (six[0].pos, six[0].feats["NumForm"]) == ("NUM", "Roman")
    assert any(a.lemma == "ver" for a in six)
    assert sum(a.prob for a in six) == pytest.approx(1.0)
    # An acronym that looks like a numeral stays a name
    assert morph.analyze("CD")[0].pos == "PROPN"
    mandela = morph.analyze("Mandela")
    assert [(a.lemma, a.pos, a.source) for a in mandela] == [
        ("Mandela", "PROPN", "rule"),
        ("mandar", "VERB", "clitics"),
    ]
    assert morph.analyze("mandela")[0].source == "clitics"
    # An imperative with an accent is mostly a verb, and case does not matter for its words
    verb = morph.analyze("Dámelo")[0]
    assert (verb.lemma, verb.word, verb.words) == ("dar", "dámelo", ("da", "me", "lo"))
    assert morph.analyze("Hacerlo")[0].lemma == "hacer"


def test_names_of_the_dictionary(morph):
    # A name keeps its case, and its forms keep the lemma
    assert morph.lemmatize(["Madrid", "madrid", "PSOE", "EE.UU."]) == [
        "Madrid",
        "Madrid",
        "PSOE",
        "EE.UU.",
    ]
    madrid = morph.analyze("Madrid")[0]
    assert {form.lemma for form in madrid.forms()} == {"Madrid"}
    assert pickle.loads(pickle.dumps(madrid)).forms() == madrid.forms()
    # A word without accents that the dictionary has only with them is mostly a name
    assert [(a.lemma, a.pos) for a in morph.analyze("Angel")][:1] == [("Angel", "PROPN")]
    # A word of the dictionary with a capital letter may be a name, after the dictionary
    estados = morph.analyze("Estados")
    assert estados[0].lemma == "estado"
    assert ("Estados", "PROPN") in [(a.lemma, a.pos) for a in estados]
    # A lowercase word is a name rarely
    assert morph.analyze("miró")[0].lemma == "mirar"
    assert morph.analyze("Miró")[0].pos == "PROPN"
    # A Roman numeral of one letter that the dictionary knows is a number first
    assert [(a.pos, a.feats.get("NumForm")) for a in morph.analyze("X")][:1] == [("NUM", "Roman")]


def test_conventions_of_the_dictionary(morph):
    def verb(lemma):
        return next(a for a in morph.analyze(lemma) if a.lemma == lemma and a.pos == "VERB")

    # Equal forms go by the table of the word, and a form of some countries is a variant
    assert verb("ser").inflect(Mood="Ind", Tense="Pres", Person="1", Number="Sing") == "soy"
    assert verb("apagar").inflect(VerbForm="Part", Gender="Fem", Number="Sing") == "apagada"
    assert verb("colar").inflect(Mood="Ind", Tense="Pres", Person="1", Number="Sing") == "cuelo"
    # A spelling of a word of the table has its lemma, and a word goes before its old spelling
    assert morph.lemmatize(["usté", "güeno", "á", "tambien"]) == ["tú", "buen", "a", "también"]
    # Such a spelling has the features of the table and is a variant
    assert len(morph.analyze("uds.")) == 1
    assert morph.analyze("ozté")[0].inflect(Number="Plur") == "oztedes"
    buen = morph.analyze("buen")[0]
    forms = [form.word for form in buen.forms()]
    assert forms[:5] == ["buen", "bueno", "buena", "buenos", "buenas"]
    # A variant takes a main form closer in spelling than its variants
    assert (buen.inflect(Number="Plur"), buen.inflect(Gender="Fem")) == ("buenos", "buena")
    assert morph.analyze("weno")[0].inflect(Gender="Fem") == "wena"
    # A verb has its infinitive among its forms
    assert morph.analyze("acepillo")[0].inflect(VerbForm="Inf") == "acepillar"
    # Older contractions, initials and symbols of units
    assert morph.analyze("desta")[0].words == ("de", "esta")
    initial = [(a.lemma, a.pos) for a in morph.analyze("V.")]
    assert initial[0] == ("V.", "PROPN") and ("tú", "PRON") in initial
    assert [(a.lemma, a.pos) for a in morph.analyze("km")][:1] == [("km", "NOUN")]
    # A demonstrative with an accent is a variant: it inflects to variants
    este = next(a for a in morph.analyze("éste") if a.pos == "PRON")
    assert este.inflect(Gender="Fem", Number="Plur") == "éstas"
    assert morph.analyze("este")[0].inflect(Gender="Fem", Number="Plur") == "estas"


def test_more_tokens_out_of_the_dictionary(morph):
    def first(word):
        analysis = morph.analyze(word)[0]
        return analysis.lemma, analysis.pos

    assert first("kg.") == ("kg", "NOUN")
    assert first("fig.") == ("fig.", "NOUN")
    assert first("b") == ("b", "NOUN")
    # A word of the dictionary with a sentence period, a letter without case and many signs
    assert morph.analyze_many(["come.", "él.", "円", "-" * 50_000 + "5"]) == [[], [], [], []]
    assert [first(word) for word in ["5kg", "100ml"]] == [("5", "NUM"), ("100", "NUM")]
    ordinal = morph.analyze("2.ª")[0]
    assert (ordinal.lemma, ordinal.pos, ordinal.feats["NumType"]) == ("2.º", "ADJ", "Ord")
    assert first("500cc") == ("500", "NUM")
    assert first("-5") == ("-5", "NUM")
    assert first("10 000") == ("10 000", "NUM")
    assert first("www.aeat.es") == ("www.aeat.es", "SYM")
    assert first("xii") == ("xii", "NUM")
    # Unicode hyphens and marks of direction do not stop the analysis
    assert first("franco\u2010alemán") == ("franco-alemán", "ADJ")
    assert first("casa\u200f") == ("casa", "NOUN")
    # An adjective in -ísimo is one, and a short rest after a prefix is no word
    esbelta = morph.analyze("esbeltísima")[0]
    assert (esbelta.lemma, esbelta.pos, esbelta.feats["Degree"]) == ("esbeltísimo", "ADJ", "Abs")
    assert first("procul")[0] != "procool"


def test_inflection_keeps_the_features_of_the_analysis(morph):
    def pronoun(word):
        return next(a for a in morph.analyze(word) if a.pos == "PRON")

    assert pronoun("lo").inflect(Case="Dat") == "le"
    # A pronoun without a gender or a number keeps its other features
    assert pronoun("ustedes").inflect(Gender="Fem") == "ustedes"
    assert pronoun("se").inflect(Number="Plur") == "se"
    assert pronoun("os").inflect(Gender="Fem") == "os"
    # A value that shares a part is no other value
    assert pronoun("él").inflect(PrepCase="Npr") == "lo"
    assert pronoun("nosotros").inflect(Number="Sing") == "yo"
    assert morph.analyze("chulitos")[0].inflect(Number="Sing") == "chulito"
    # A value of several parts in any order, a rule analysis without number
    el = morph.analyze("él")[0]
    assert el.inflect(Case="Nom,Acc") == el.inflect(Case="Acc,Nom") == "él"
    assert morph.analyze("ETA")[0].inflect(Number="Plur") == "ETA"
    assert morph.analyze("123")[0].inflect(Number="Plur") == "123"
    # An analysis whose lexeme the dictionary has not, and a guessed form after pickle
    lost = esmorph._esmorph._analysis(
        "gatos", "perro", "NOUN", "Gender=Masc|Number=Plur", 1.0, [], "dictionary"
    )
    with pytest.raises(esmorph.FormNotFoundError, match="no lexeme of 'gatos'"):
        lost.inflect(Number="Sing")
    forms = morph.analyze("blorpear")[0].forms()
    assert all(pickle.loads(pickle.dumps(form)).forms() == forms for form in forms)


def test_clitics_in_literary_writing(morph):
    # A finite form takes enclitics with the accent of the token
    dijole = morph.analyze("díjole")[0]
    assert (dijole.lemma, dijole.words, dijole.source) == ("decir", ("dijo", "le"), "clitics")
    assert morph.is_known("levantóse")
    # `te` follows only the imperative of `tú`
    assert {a.lemma for a in morph.analyze("siéntate")} == {"sentar"}
    # A verb with clitics goes before a word with other accents, and a host as written first
    assert morph.analyze("verte")[0].words == ("ver", "te")
    assert morph.analyze("verse")[0].words == ("ver", "se")
    assert morph.analyze("alientelo")[0].lemma == "alentar"


def test_guessed_words(morph):
    analyses = morph.analyze("zumbeábamos")
    first = analyses[0]
    assert (first.lemma, first.pos, first.source) == ("zumbear", "VERB", "guess")
    assert first.feats == {
        "Mood": "Ind",
        "Number": "Plur",
        "Person": "1",
        "Tense": "Imp",
        "VerbForm": "Fin",
    }
    assert sum(a.prob for a in analyses) == pytest.approx(1.0)
    assert not morph.is_known("zumbeábamos")
    # A guessed word has the forms of its sample
    assert first.inflect(Tense="Pres", Person="1", Number="Sing") == "zumbeo"
    assert [form.word for form in first.forms()][:3] == ["zumbear", "zumbeando", "zumbeado"]
    assert pickle.loads(pickle.dumps(first)) == first
    assert morph.lemmatize(["zumbeábamos"]) == ["zumbear"]


@pytest.mark.parametrize(
    ("word", "lemma", "pos"),
    [
        ("supergallos", "supergallo", "NOUN"),
        ("sobreandan", "sobreandar", "VERB"),
        ("antimundialización", "antimundialización", "NOUN"),
        ("palestino-israelíes", "palestino-israelí", "NOUN"),
        ("ciencia-ficción", "ciencia-ficción", "NOUN"),
        ("decretorios", "decretorio", "NOUN"),
    ],
)
def test_guessed_by_parts_and_endings(morph, word, lemma, pos):
    first = morph.analyze(word)[0]
    assert (first.word, first.lemma, first.pos, first.source) == (word, lemma, pos, "guess")


def test_what_is_not_guessed(morph):
    # No sample has the ending, a word in capitals is a name, a mixed token is not a word
    assert morph.analyze("qzxw") == []
    assert morph.analyze("Zumbeábamos")[0].pos == "PROPN"
    assert morph.analyze("zumbe4") == []
    assert morph.analyze("z" * 100) == []


def test_forms_of_guessed_words_are_guessed(morph):
    forms = morph.analyze("zumbeábamos")[0].forms()
    assert all(form.source == "guess" for form in forms)
    # Every form keeps the lexeme: its forms and its inflection
    present = next(form for form in forms if form.word == "zumbeo")
    assert present.forms() == forms
    assert present.inflect(Number="Plur") == "zumbeamos"
    prefixed = morph.analyze("supergallos")[0]
    assert prefixed.forms()[0].forms() == prefixed.forms()


@pytest.mark.parametrize(
    ("word", "lemma", "words"),
    [
        ("autogenerarse", "autogenerar", ("autogenerar", "se")),
        ("zumbeándose", "zumbear", ("zumbeando", "se")),
        ("zumbearlo", "zumbear", ("zumbear", "lo")),
    ],
)
def test_guessed_verbs_with_clitics(morph, word, lemma, words):
    first = morph.analyze(word)[0]
    assert (first.lemma, first.pos, first.words, first.source) == (lemma, "VERB", words, "guess")
    assert first.forms()[0].word == lemma


def test_the_last_part_after_a_hyphen_is_of_an_open_class(morph):
    assert morph.analyze("sur-este")[0].pos == "NOUN"
    assert morph.analyze("post-nosotros") == []
