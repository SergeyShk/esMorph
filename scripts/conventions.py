"""
Conventions of the dictionary: the rows of Wiktionary in the terms of the Spanish treebanks of UD

Description:
    Closed classes come from a reviewed table (`data/closed-classes.tsv`, after UD_Spanish-AnCora):
    Wiktionary has other lemmas for them and no `PronType`, `Definite` or `Person`. Open classes
    follow the treebank too: auxiliaries are also AUX, a participle is also an adjective with itself
    as the lemma, and a verb has no `-se` in its lemma. A row is "lemma, part of speech, form,
    features in CoNLL-U notation".
"""

import re
from pathlib import Path

Row = tuple[str, str, str, str]

CLOSED_CLASSES = Path(__file__).resolve().parents[1] / "data" / "closed-classes.tsv"
# What the table answers for alone: a word of such a class out of the table with an analysis of
# another class is no such word, as `hace` is no preposition
CLOSED = {"ADP", "CCONJ", "DET", "PART", "PRON", "SCONJ"}
AUXILIARIES = {"deber", "estar", "haber", "poder", "querer", "saber", "ser", "soler"}
NUMERAL = "Number=Plur|NumType=Card"
ABSOLUTE = re.compile(r"ísim[oa]s?$")
PARTICIPLE_ENDING = re.compile(r"(as|os|a)$")


def closed_classes(path: Path = CLOSED_CLASSES) -> set[Row]:
    """
    Rows of the table of closed classes

    Arguments:
        path (Path): File of the table

    Returns:
        set[Row]: The rows
    """
    rows = set()
    with path.open(encoding="utf-8") as lines:
        for line in lines:
            lemma, pos, form, feats = line.rstrip("\n").split("\t")
            rows.add((lemma, pos, form, feats))
    return rows


def parse_features(text: str) -> dict[str, str]:
    """
    Features from CoNLL-U notation

    Arguments:
        text (str): Features, `_` for none

    Returns:
        dict[str, str]: The features
    """
    return dict(feat.split("=", 1) for feat in text.split("|")) if text != "_" else {}


def format_features(feats: dict[str, str]) -> str:
    """
    Features in CoNLL-U notation

    Arguments:
        feats (dict[str, str]): The features

    Returns:
        str: The features in their order, `_` for none
    """
    return "|".join(f"{name}={feats[name]}" for name in sorted(feats, key=str.lower)) or "_"


def with_table(rows: set[Row], table: set[Row]) -> set[Row]:
    """
    Rows with the table in place of what Wiktionary says of its words

    Description:
        The table replaces the rows of closed classes and numerals of its forms, and the rows of the
        parts of speech it has for a form. A form out of the table keeps a row of a closed class only
        if it has nothing else: it may be an old or a rare word

    Arguments:
        rows (set[Row]): Rows of Wiktionary
        table (set[Row]): Rows of the table

    Returns:
        set[Row]: The rows
    """
    answers: dict[str, set[str]] = {}
    for _, pos, form, _ in table:
        answers.setdefault(form, CLOSED | {"NUM"}).add(pos)
    other = {form for _, pos, form, _ in rows if pos not in CLOSED}
    return table | {
        row
        for row in rows
        if row[1] not in answers.get(row[2], ()) and (row[1] not in CLOSED or row[2] not in other)
    }


def verb_lemmas(rows: set[Row]) -> set[Row]:
    """
    Verbs without `-se` in the lemma; a verb known only with it gets its plain infinitive

    Arguments:
        rows (set[Row]): The rows

    Returns:
        set[Row]: The rows
    """
    result = set()
    for lemma, pos, form, feats in rows:
        if pos == "VERB" and lemma.endswith("rse"):
            lemma = lemma.removesuffix("se")
            result.add((lemma, pos, lemma, "VerbForm=Inf"))
        result.add((lemma, pos, form, feats))
    return result


def names_and_numerals(rows: set[Row]) -> set[Row]:
    """
    Names without features, numerals out of the table as cardinal numbers

    Arguments:
        rows (set[Row]): The rows

    Returns:
        set[Row]: The rows
    """
    result = set()
    for lemma, pos, form, feats in rows:
        if pos == "PROPN":
            feats = "_"
        elif pos == "NUM" and "NumType" not in feats:
            feats = NUMERAL
        result.add((lemma, pos, form, feats))
    return result


def absolute_superlatives(rows: set[Row]) -> set[Row]:
    """
    Adjectives in `-ísimo` with `Degree=Abs`; the masculine singular is their lemma

    Arguments:
        rows (set[Row]): The rows

    Returns:
        set[Row]: The rows
    """
    result = set()
    for lemma, pos, form, feats in rows:
        if pos == "ADJ" and ABSOLUTE.search(form):
            lemma = ABSOLUTE.sub("ísimo", form)
            feats = format_features(parse_features(feats) | {"Degree": "Abs"})
        result.add((lemma, pos, form, feats))
    return result


def auxiliaries(rows: set[Row]) -> set[Row]:
    """
    Forms of auxiliary verbs also as AUX

    Arguments:
        rows (set[Row]): The rows

    Returns:
        set[Row]: The rows
    """
    return rows | {
        (lemma, "AUX", form, feats)
        for lemma, pos, form, feats in rows
        if pos == "VERB" and lemma in AUXILIARIES
    }


def participle_adjectives(rows: set[Row]) -> set[Row]:
    """
    Participles also as adjectives: the lemma is the participle in the masculine singular

    Description:
        `VerbForm=Part` stays and `Tense` goes. An adjective of the same word and features from
        Wiktionary is the same analysis

    Arguments:
        rows (set[Row]): The rows

    Returns:
        set[Row]: The rows
    """
    copies = set()
    plain = set()
    for _, pos, form, text in rows:
        feats = parse_features(text)
        if pos == "VERB" and feats.get("VerbForm") == "Part":
            feats.pop("Tense", None)
            lemma = PARTICIPLE_ENDING.sub("o", form)
            copies.add((lemma, "ADJ", form, format_features(feats)))
            del feats["VerbForm"]
            plain.add((lemma, "ADJ", form, format_features(feats)))
    return (rows - plain) | copies


def apply(rows: set[Row], table: set[Row]) -> set[Row]:
    """
    Rows of Wiktionary in the conventions of the dictionary

    Arguments:
        rows (set[Row]): Rows of Wiktionary
        table (set[Row]): Rows of the table of closed classes

    Returns:
        set[Row]: The rows
    """
    rows = names_and_numerals(with_table(verb_lemmas(rows), table))
    return participle_adjectives(auxiliaries(absolute_superlatives(rows)))
