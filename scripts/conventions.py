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
from typing import NamedTuple

Row = tuple[str, str, str, str]

CLOSED_CLASSES = Path(__file__).resolve().parents[1] / "data" / "closed-classes.tsv"
UNITS = Path(__file__).resolve().parents[1] / "data" / "units.tsv"
SPELLINGS = Path(__file__).resolve().parents[1] / "data" / "spellings.tsv"
# What the table answers for alone: a word of such a class out of the table with an analysis of
# another class is no such word, as `hace` is no preposition
CLOSED = {"ADP", "CCONJ", "DET", "PART", "PRON", "SCONJ"}
AUXILIARIES = {"deber", "estar", "haber", "poder", "querer", "saber", "ser", "soler"}
CARDINAL = {"Number": "Plur", "NumType": "Card"}
ABSOLUTE = re.compile(r"ísim([oa])s?$")
PARTICIPLE_ENDING = re.compile(r"(as|os|a)$")
INFINITIVE = re.compile(r"[aeiíoó]r$")


def closed_classes(path: Path = CLOSED_CLASSES) -> set[Row]:
    """
    Rows of a table: of closed classes or of units

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


def spellings(path: Path = SPELLINGS) -> set[tuple[str, str]]:
    """
    Spoken spellings of the table and their standard words: `pos` and `pues`

    Arguments:
        path (Path): File of the table

    Returns:
        set[tuple[str, str]]: The spellings and their standard words
    """
    with path.open(encoding="utf-8") as lines:
        return {tuple(line.rstrip("\n").split("\t")) for line in lines}  # type: ignore[misc]


class Spoken(NamedTuple):
    """A spelling of Wiktionary as a word sounds"""

    spelling: str
    pos: str
    standard: str
    # The forms of the own lexeme of the spelling and their features, if it has no other sense
    forms: frozenset[tuple[str, str]] | None


def spoken(
    rows: set[Row], pairs: set[Spoken], table: set[tuple[str, str]]
) -> tuple[set[Row], dict[Row, str]]:
    """
    Rows with the spoken spellings of standard words

    Description:
        A spoken spelling is a form of every analysis of its standard word, with its features: the
        treebanks give it the lemma and the features of that word and keep the spelling apart
        (`Ortho=` of COSER). A spelling that is no verb takes no verb but a participle (`na` is no
        form of `nadar`, `dejao` is `dejar`), and one that is no name takes no name (`güeno` is no
        `Bueno`). The rows of a spelling of Wiktionary (`güeno` of `bueno`, `er` of `el`) in the
        lexemes of its standard word give way to these. Of a spelling that has no other sense the
        forms of its own lexeme with the features of the spelling take the analyses of the
        standard word too (`ná` of `na`); its other forms are the forms of the lexemes of the
        standard word with the features they have, the closest in ending and then in start to the
        standard word (`güevos` of `huevos`, `esplicase` of `explicase`, `wena` of `buena`); a form
        that none has stays as it is. A spelling of the table (`pos` of `pues`, `to` of `todo`)
        keeps its other analyses: `tos` is also a noun. A spelling whose standard word the rows
        have not stays as it is

    Arguments:
        rows (set[Row]): The rows
        pairs (set[Spoken]): Spellings of Wiktionary as words sound
        table (set[tuple[str, str]]): Spellings of the table and their standard words

    Returns:
        tuple[set[Row], dict[Row, str]]: The rows without what the spoken spellings replace, and
        the rows of the spoken spellings with the forms they stand for
    """
    of_form: dict[str, list[Row]] = {}
    of_lexeme: dict[tuple[str, str], list[Row]] = {}
    for row in rows:
        of_form.setdefault(row[2], []).append(row)
        of_lexeme.setdefault((row[0], row[1]), []).append(row)
    gone: set[Row] = set()
    result: dict[Row, str] = {}

    # The lexemes of the spoken rows of a form: its rows in them give way
    added: dict[str, set[tuple[str, str]]] = {}

    def add(analyses: list[Row], form: str, standard: str) -> None:
        for lemma, part, _, feats in analyses:
            result.setdefault((lemma, part, form, feats), standard)
            added.setdefault(form, set()).add((lemma, part))

    for spelling, pos, standard, forms in sorted(pairs, key=lambda pair: pair[:3]):
        analyses = spoken_analyses(of_form.get(standard, []), pos)
        if not analyses:
            continue
        lexemes = {(lemma, part) for lemma, part, _, _ in analyses}
        add(analyses, spelling, standard)
        replaced = {spelling}
        heads = {feats for form, feats in forms or () if form == spelling}
        own_forms = {form for form, _ in forms or ()}
        for form, feats in sorted(forms or ()):
            if form == spelling:
                continue
            if feats in heads:
                add(analyses, form, standard)
                replaced.add(form)
                continue
            own = parse_features(feats)
            candidates = [
                row
                for lexeme in sorted(lexemes)
                if lexeme[1] == pos
                for row in of_lexeme[lexeme]
                if row[2] not in own_forms and own.items() <= parse_features(row[3]).items()
            ]
            if candidates:
                row = max(
                    candidates,
                    key=lambda row: (common_end(row[2], form), common_start(row[2], standard)),
                )
                add([row], form, row[2])
                replaced.add(form)
        # A row of another lexeme stays: `dejá` of `dejao` is also `dejar`
        for form in replaced:
            gone |= {
                row
                for row in of_form.get(form, [])
                if (row[0], row[1]) in added[form]
                or (forms is not None and (row[0], row[1]) == (spelling, pos))
            }
    for spelling, standard in sorted(table):
        add(spoken_analyses(of_form.get(standard, []), None), spelling, standard)
    rows = rows - gone
    return rows, {row: standard for row, standard in result.items() if row not in rows}


def common_end(word: str, other: str) -> int:
    """
    Length of the common end of two words

    Arguments:
        word (str): A word
        other (str): Another word

    Returns:
        int: The number of letters
    """
    return common_start(word[::-1], other[::-1])


def common_start(word: str, other: str) -> int:
    """
    Length of the common start of two words

    Arguments:
        word (str): A word
        other (str): Another word

    Returns:
        int: The number of letters
    """
    length = 0
    for letter, other_letter in zip(word, other, strict=False):
        if letter != other_letter:
            break
        length += 1
    return length


def spoken_analyses(rows: list[Row], pos: str | None) -> list[Row]:
    """
    Analyses of a standard word that its spoken spelling takes

    Arguments:
        rows (list[Row]): Rows of the standard word
        pos (str | None): Part of speech of the spelling, `None` if unknown

    Returns:
        list[Row]: The rows; a verb only for a verb or as a participle, a name only for a name
    """
    verb = pos in ("VERB", "AUX")
    return [
        row
        for row in rows
        if (verb or row[1] not in ("VERB", "AUX") or "VerbForm=Part" in row[3])
        and (row[1] != "PROPN" or pos == "PROPN")
    ]


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


def table_lemmas(rows: set[Row], table: set[Row]) -> tuple[set[Row], set[tuple[str, str]]]:
    """
    Rows whose lemma is a form of the table with another lemma, in the terms of the table

    Description:
        Such a row takes the lemma of the table and the features of the table under its own:
        `usté`, a spelling of `usted`, is a form of `tú`, `güeno` of `bueno` a form of `buen`. The
        lexeme of such a spelling is a lexeme of the table word too: `usté` has no lemma of its
        own. The rows of the table stay as they are

    Arguments:
        rows (set[Row]): The rows
        table (set[Row]): Rows of the table

    Returns:
        tuple[set[Row], set[tuple[str, str]]]: The rows; part of speech and form of the moved rows
        that the table has not, spellings of its words
    """
    analyses: dict[tuple[str, str], list[tuple[str, str]]] = {}
    for lemma, pos, form, feats in table:
        analyses.setdefault((form, pos), []).append((lemma, feats))
    forms = {form for _, _, form, _ in table}
    # Spellings of a word of the table and the analyses of that word
    spellings: dict[tuple[str, str], list[tuple[str, str]]] = {}
    for lemma, pos, form, _ in rows:
        others = [(new, feats) for new, feats in analyses.get((lemma, pos), []) if new != lemma]
        if others and form not in forms:
            spellings.setdefault((form, pos), []).extend(others)
    result = set()
    moved = set()
    for row in rows:
        lemma, pos, form, feats = row
        if row in table:
            result.add(row)
            continue
        others = [
            (new, table_feats)
            for new, table_feats in analyses.get((lemma, pos), [])
            if new != lemma
        ]
        others = others or spellings.get((lemma, pos), [])
        if not others:
            result.add(row)
            continue
        if (form, pos) not in analyses:
            moved.add((pos, form))
        for new, table_feats in others:
            merged = parse_features(table_feats) | parse_features(feats)
            result.add((new, pos, form, format_features(merged)))
    return result, moved


def without_ordinals_in_digits(rows: set[Row]) -> set[Row]:
    """
    Rows without the ordinal numbers in digits (`1.º`, `2ª`): a rule of the analyzer gives them,
    with the features that Wiktionary has not

    Arguments:
        rows (set[Row]): The rows

    Returns:
        set[Row]: The rows
    """
    return {row for row in rows if not (row[1] == "ADJ" and row[2][:1].isdigit())}


def infinitives(rows: set[Row]) -> set[Row]:
    """
    Rows with the infinitive of every verb: a verb that only the entries of its forms give, as a
    rare spelling of another verb (`acepillar` of `cepillar`), has its own lemma among its forms

    Arguments:
        rows (set[Row]): The rows

    Returns:
        set[Row]: The rows
    """
    verbs = {lemma for lemma, pos, _, _ in rows if pos == "VERB" and INFINITIVE.search(lemma)}
    return rows | {(lemma, "VERB", lemma, "VerbForm=Inf") for lemma in verbs}


def verb_lemmas(rows: set[Row]) -> set[Row]:
    """
    Verbs without `-se` in the lemma; a verb known only with it gets its plain infinitive. An
    infinitive with `-se` is a verb with a clitic, which the analyzer splits

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
        if pos == "VERB" and form != lemma and form.endswith("rse") and feats == "VerbForm=Inf":
            continue
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
            gender = parse_features(feats).get("Gender")
            feats = format_features(CARDINAL | ({"Gender": gender} if gender else {}))
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
        ending = ABSOLUTE.search(form)
        if pos == "ADJ" and ending:
            lemma = ABSOLUTE.sub("ísimo", form)
            gender = "Masc" if ending[1] == "o" else "Fem"
            feats = format_features(parse_features(feats) | {"Degree": "Abs", "Gender": gender})
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


def apply(rows: set[Row], table: set[Row]) -> tuple[set[Row], set[tuple[str, str]]]:
    """
    Rows of Wiktionary in the conventions of the dictionary

    Arguments:
        rows (set[Row]): Rows of Wiktionary
        table (set[Row]): Rows of the table of closed classes

    Returns:
        tuple[set[Row], set[tuple[str, str]]]: The rows; part of speech and form of the spellings
        of words of the table that it has not, as `güeno`
    """
    rows = with_table(infinitives(verb_lemmas(without_ordinals_in_digits(rows))), table)
    rows, spellings = table_lemmas(rows, table)
    rows = names_and_numerals(rows)
    # Symbols of units, which Wiktionary has not as words: `km` is a noun or a symbol
    rows |= closed_classes(UNITS)
    return participle_adjectives(auxiliaries(absolute_superlatives(rows))), spellings
