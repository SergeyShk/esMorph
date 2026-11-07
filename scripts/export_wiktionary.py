"""
Export of the Spanish entries of Wiktionary into the text table for the `esmorph-build` builder

Description:
    The snapshot of the entries (`wiktionary_snapshot.py`) lies in a dataset on Hugging Face and is
    checked by SHA-256. Every entry and every form of its table become rows "lemma, part of speech,
    form, features" in the terms of Universal Dependencies; the builder joins the rows of one lemma
    and part of speech into a lexeme. Only the standard library is needed.

Example:
    uv run python scripts/export_wiktionary.py target/wiktionary
"""

import argparse
import hashlib
import http.client
import json
import lzma
import re
import time
import unicodedata
import urllib.error
import urllib.request
from collections.abc import Iterator
from itertools import product
from pathlib import Path

LICENSE = "CC BY-SA 4.0"
# The dump of en.wiktionary.org of 2026-10-01, extracted by wiktextract on 2026-10-30 (kaikki.org)
SNAPSHOT = (
    "es-wiktionary-20261001.jsonl.xz",
    "https://huggingface.co/datasets/SergeyShk/esmorph-wiktionary/resolve/b7cdad1194037090a2a3f3e75be7871776359e63/es-wiktionary-20261001.jsonl.xz",
    "fad6681fb7b60e30c601b75ee94297c6bb83d032551ff2b8d6eeb6cb14aeec4c",
)
ATTEMPTS = 4
PAUSE = 5

PARTS_OF_SPEECH = {
    "adj": "ADJ",
    "adv": "ADV",
    "article": "DET",
    "conj": "CCONJ",
    "det": "DET",
    "intj": "INTJ",
    "name": "PROPN",
    "noun": "NOUN",
    "num": "NUM",
    "particle": "PART",
    "prep": "ADP",
    "pron": "PRON",
    "verb": "VERB",
}
NOMINAL = {"ADJ", "DET", "NOUN", "NUM", "PRON", "PROPN"}
# The lemma of these is singular unless its tags say otherwise
SINGULAR_BASE = {"ADJ", "DET", "NOUN"}
PERSONS = {"first-person": "1", "second-person": "2", "third-person": "3"}
TENSES = {"present": "Pres", "preterite": "Past", "imperfect": "Imp", "future": "Fut"}
GENDERS = {"feminine", "masculine"}
NON_FINITE = {"infinitive", "gerund", "participle"}
# Tags of a form of a noun or an adjective: other rows of their tables are other words with entries
# of their own (`caserón` of `casa`, `grandísimo` of `grande`, `pié` of `pie`)
NOMINAL_FORM = {"canonical", "feminine", "masculine", "plural", "singular"}
# Rows of a conjugation table that are no forms of the standard paradigm; the negative imperative is
# the subjunctive with `no`
OTHER_VERB_FORM = {
    "abbreviation",
    "adjective",
    "alternative",
    "archaic",
    "canonical",
    "class",
    "colloquial",
    "dated",
    "dialectal",
    "misspelling",
    "negative",
    "nonstandard",
    "obsolete",
    "pronunciation-spelling",
    "proscribed",
    "rare",
    "uncommon",
}
# Verbs without a table of their own have bare senses for finite forms: only an infinitive is a lemma
INFINITIVE = re.compile(r"[aeiíoó]r(se)?$")
# The finite rows of a pronominal table are `me destoso`
TABLE_CLITIC = re.compile(r"^(me|te|se|nos|os) ")
CLITIC_ENDING = re.compile(r"(me|te|se|nos|os|lo|la|le|los|las|les)$")
NON_FINITE_ROWS = {"infinitive", "gerund"}
FORMS_WITH_CLITICS = NON_FINITE_ROWS | {"imperative"}
# A sense "diminutive of" names another word: the treebanks keep such a word as its own lemma
DERIVED = {"augmentative", "diminutive", "superlative"}
MASCULINE_PARTICIPLE = "Gender=Masc|Number=Sing|Tense=Past|VerbForm=Part"
WORD_SIGNS = {".", "-", "'"}
MAX_WORD = 64


def sha256(path: Path) -> str:
    """
    SHA-256 of a file

    Arguments:
        path (Path): The file

    Returns:
        str: The digest in hexadecimal
    """
    digest = hashlib.sha256()
    with path.open("rb") as file:
        while chunk := file.read(1 << 20):
            digest.update(chunk)
    return digest.hexdigest()


def download(url: str, path: Path, digest: str) -> Path:
    """
    Downloading a file with its checksum checked; a network failure, 429 or 5xx is retried

    Arguments:
        url (str): Address of the file
        path (Path): Where to save it
        digest (str): SHA-256 of the file

    Returns:
        Path: Path to the file

    Raises:
        ValueError: If no attempt gave a file with the expected checksum
    """
    if path.exists() and sha256(path) == digest:
        return path
    path.parent.mkdir(parents=True, exist_ok=True)
    errors = []
    for attempt in range(ATTEMPTS):
        time.sleep(PAUSE * attempt)
        try:
            with urllib.request.urlopen(url, timeout=300) as response, path.open("wb") as out:
                while chunk := response.read(1 << 20):
                    out.write(chunk)
        except urllib.error.HTTPError as err:
            errors.append(f"{err.code}")
            if err.code != 429 and err.code < 500:
                break
            continue
        except (OSError, http.client.HTTPException) as err:
            errors.append(str(err))
            continue
        if sha256(path) == digest:
            return path
        errors.append("checksum mismatch")
    path.unlink(missing_ok=True)
    raise ValueError(f"{url}: {', '.join(errors)}")


def verb_features(tags: set[str]) -> list[dict[str, str]]:
    """
    Universal Dependencies features of a verb form

    Arguments:
        tags (set[str]): Tags of the form in wiktextract

    Returns:
        list[dict[str, str]]: The features; a sense can name several persons or tenses at once
    """
    if "infinitive" in tags:
        return [{"VerbForm": "Inf"}]
    if "gerund" in tags:
        return [{"VerbForm": "Ger"}]
    if "participle" in tags:
        return [
            {
                "VerbForm": "Part",
                "Tense": "Past",
                "Gender": "Fem" if "feminine" in tags else "Masc",
                "Number": "Plur" if "plural" in tags else "Sing",
            }
        ]
    feats = {"VerbForm": "Fin"}
    if "singular" in tags:
        feats["Number"] = "Sing"
    if "plural" in tags:
        feats["Number"] = "Plur"
    tenses: list[str | None] = [None]
    if "imperative" in tags:
        feats["Mood"] = "Imp"
    elif "conditional" in tags:
        feats["Mood"] = "Cnd"
    else:
        feats["Mood"] = "Sub" if "subjunctive" in tags else "Ind"
        tenses = [tense for tag, tense in TENSES.items() if tag in tags] or [None]
    persons = [person for tag, person in PERSONS.items() if tag in tags] or [None]
    return [
        feats | ({"Person": person} if person else {}) | ({"Tense": tense} if tense else {})
        for person, tense in product(persons, tenses)
    ]


def nominal_features(tags: set[str], singular_base: bool) -> dict[str, str]:
    """
    Gender and number of a noun, an adjective, a pronoun or a determiner

    Arguments:
        tags (set[str]): Tags of the form in wiktextract
        singular_base (bool): Whether a form without a number is singular

    Returns:
        dict[str, str]: The features
    """
    feats = {}
    feminine, masculine = "feminine" in tags, "masculine" in tags
    if feminine != masculine:
        feats["Gender"] = "Fem" if feminine else "Masc"
    if "plural" in tags and "singular" not in tags:
        feats["Number"] = "Plur"
    elif singular_base or "singular" in tags:
        feats["Number"] = "Sing"
    return feats


def features(pos: str, tags: set[str]) -> list[dict[str, str]]:
    """
    Features of a form of a part of speech

    Arguments:
        pos (str): Part of speech
        tags (set[str]): Tags of the form in wiktextract

    Returns:
        list[dict[str, str]]: The features; a noun of both genders has a row for each
    """
    if pos == "VERB":
        return verb_features(tags)
    if pos == "NOUN" and tags >= GENDERS:
        return [
            nominal_features(tags - GENDERS | {gender}, singular_base=True)
            for gender in sorted(GENDERS)
        ]
    if pos in NOMINAL:
        return [nominal_features(tags, pos in SINGULAR_BASE)]
    return [{}]


def clean(word: str) -> str | None:
    """
    A word as the dictionary keeps it: NFC and lower case

    Arguments:
        word (str): Word of the entry

    Returns:
        str | None: The word, or `None` if it is no single word: tables hold `-` for a missing form,
        entries hold phrases, signs and affixes
    """
    word = unicodedata.normalize("NFC", word.lower())
    valid = (
        len(word) <= MAX_WORD
        and not word.startswith("-")
        and not word.endswith("-")
        and any(char.isalpha() for char in word)
        and all(char.isalnum() or char in WORD_SIGNS for char in word)
    )
    return word if valid else None


def is_verb_form(tags: set[str]) -> bool:
    """
    Whether tags of a row or a sense describe a form of a verb

    Arguments:
        tags (set[str]): Tags in wiktextract

    Returns:
        bool: Whether the form is non-finite or has a person, belongs to the standard paradigm and
        has no clitics: `hazlo` has the tags `object-*`, `mirándose` has the tag `reflexive`,
        `abatirme` is an infinitive with a person
    """
    return (
        bool(tags & (NON_FINITE | set(PERSONS)))
        and not tags & OTHER_VERB_FORM
        and "reflexive" not in tags
        and not any(tag.startswith("object-") for tag in tags)
        and not (tags & NON_FINITE_ROWS and tags & set(PERSONS))
    )


def is_entry(word: str, entries: set[str]) -> bool:
    """
    Whether a word a sense refers to is an entry itself

    Arguments:
        word (str): The word: comments and phrases also stand in the place of a reference
        entries (set[str]): Headwords of the snapshot

    Returns:
        bool: Whether it is a headword of one word
    """
    return " " not in word and word in entries


def verb_rows(word: str, forms: list[tuple[str, set[str]]]) -> Iterator[tuple[str, set[str]]]:
    """
    Rows of a conjugation table that are forms without clitics

    Description:
        A table with rows like `abatirme` has a second block for the reflexive verb: `abatirse`,
        `abatiéndose`, `abátete`. Only the first row of a set of tags stays there. The gerund and
        the imperative of a `-se` lemma carry its clitic

    Arguments:
        word (str): Headword
        forms (list[tuple[str, set[str]]]): Rows of the table

    Returns:
        Iterator[tuple[str, set[str]]]: The form and its tags
    """
    pronominal = any(tags & NON_FINITE_ROWS and tags & set(PERSONS) for _, tags in forms)
    seen = set()
    for form, tags in forms:
        if not is_verb_form(tags):
            continue
        if tags & FORMS_WITH_CLITICS:
            if word.endswith("se") and tags & {"gerund", "imperative"}:
                continue
            if pronominal:
                key = frozenset(tags)
                if key in seen:
                    continue
                seen.add(key)
        yield TABLE_CLITIC.sub("", form), tags


def is_other_gender(tags: set[str], gender: set[str]) -> bool:
    """
    Whether a row of a noun table names a word of another gender as a form of the headword

    Arguments:
        tags (set[str]): Tags of the row
        gender (set[str]): Genders of the headword

    Returns:
        bool: Whether it is: `madre` and `niñe` list `padre` and `niño`, while `niño` keeps `niña`,
        which the treebanks and inflection expect of it
    """
    return "canonical" not in tags and (
        ("masculine" in tags and "masculine" not in gender) or ("feminine" in tags and not gender)
    )


def form_of_rows(
    word: str, pos: str, tags: set[str], targets: list[str]
) -> Iterator[tuple[str, str, str, dict[str, str]]]:
    """
    Rows of a sense with `form_of`

    Description:
        A derived word is a lemma of its own. The lemma of a singular feminine noun is both the
        word itself and its masculine form: treebanks use both

    Arguments:
        word (str): Headword
        pos (str): Part of speech
        tags (set[str]): Tags of the sense
        targets (list[str]): The words the sense refers to

    Returns:
        Iterator[tuple[str, str, str, dict[str, str]]]: Lemma, part of speech, form, features
    """
    target = targets[0]
    if pos == "VERB":
        if not is_verb_form(tags) or " combined with " in target:
            return
        # The gerund and the imperative of a reflexive verb carry a clitic: `levántate`
        if (
            target.endswith("se")
            and tags & {"gerund", "imperative"}
            and CLITIC_ENDING.search(word)
        ):
            return
    lemmas = [] if tags & DERIVED else target.split(" and ")
    if tags & DERIVED or (pos == "NOUN" and "feminine" in tags and "plural" not in tags):
        lemmas.append(word)
    for lemma in lemmas:
        for feats in features(pos, tags):
            yield lemma, pos, word, feats


def lemma_rows(
    word: str, pos: str, tags: set[str], targets: list[str], entries: set[str]
) -> Iterator[tuple[str, str, str, dict[str, str]]]:
    """
    Rows of a sense that makes the word a lemma; a word that only spells another one is a form of both

    Arguments:
        word (str): Headword
        pos (str): Part of speech
        tags (set[str]): Tags of the sense with the genders of the headword
        targets (list[str]): The words the sense names as other spellings
        entries (set[str]): Headwords of the snapshot

    Returns:
        Iterator[tuple[str, str, str, dict[str, str]]]: Lemma, part of speech, form, features
    """
    for feats in features(pos, {"infinitive"} if pos == "VERB" else tags):
        yield word, pos, word, feats
        for target in targets:
            if is_entry(target, entries):
                yield target, pos, word, feats


def nominal_rows(
    word: str, pos: str, forms: list[tuple[str, set[str]]], gender: set[str]
) -> Iterator[tuple[str, str, str, dict[str, str]]]:
    """
    Rows of the table of a noun, an adjective, a pronoun or a determiner

    Arguments:
        word (str): Headword
        pos (str): Part of speech
        forms (list[tuple[str, set[str]]]): Rows of the table
        gender (set[str]): Genders of the headword

    Returns:
        Iterator[tuple[str, str, str, dict[str, str]]]: Lemma, part of speech, form, features
    """
    for form, tags in forms:
        if not tags or not tags <= NOMINAL_FORM:
            continue
        if "canonical" in tags and form.lower() != word.lower():
            continue
        if pos == "NOUN" and is_other_gender(tags, gender):
            continue
        row_tags = tags if tags & GENDERS else tags | gender
        for feats in features(pos, row_tags):
            yield word, pos, form.removeprefix("before a noun "), feats


def entry_rows(
    entry: dict, entries: set[str], lower_entries: set[str]
) -> Iterator[tuple[str, str, str, dict[str, str]]]:
    """
    Rows of one entry

    Description:
        A sense with `form_of` gives the word as a form of that lemma, any other sense gives the
        word as a lemma, for a verb only if it looks like an infinitive. The forms of the table
        belong to an entry that has a sense of its own. An adjective with a feminine form is
        masculine in its base form. An acronym with the spelling of a word in lower case is left
        out, as `SE` for `se`: the analyzer does not see case

    Arguments:
        entry (dict): Entry of the snapshot
        entries (set[str]): Headwords of the snapshot
        lower_entries (set[str]): Headwords of the snapshot in lower case

    Returns:
        Iterator[tuple[str, str, str, dict[str, str]]]: Lemma, part of speech, form, features
    """
    word, pos = entry["word"], PARTS_OF_SPEECH.get(entry["pos"])
    if pos is None or (word == word.upper() != word.lower() and word.lower() in lower_entries):
        return
    forms = [(form, set(tags)) for form, tags in entry["forms"]]
    own = False
    gender = {tag for sense in entry["senses"] if not sense["form_of"] for tag in sense["tags"]}
    gender &= GENDERS
    if pos == "ADJ" and any("feminine" in tags and "plural" not in tags for _, tags in forms):
        gender = gender or {"masculine"}
    for sense in entry["senses"]:
        tags = set(sense["tags"])
        # Misspellings and spellings of the Internet: `hay` as `ahí`
        if "misspelling" in tags or ("Internet" in tags and sense["alt_of"]):
            continue
        if sense["form_of"]:
            yield from form_of_rows(word, pos, tags, sense["form_of"])
        elif pos != "VERB" or INFINITIVE.search(word):
            own = True
            yield from lemma_rows(word, pos, tags | gender, sense["alt_of"], entries)
    if own and pos == "VERB":
        for form, tags in verb_rows(word, forms):
            for feats in verb_features(tags):
                yield word, pos, form, feats
    elif own:
        yield from nominal_rows(word, pos, forms, gender)


def snapshot_entries(path: Path) -> Iterator[dict]:
    """
    Entries of the snapshot

    Arguments:
        path (Path): File of the snapshot

    Returns:
        Iterator[dict]: The entries
    """
    with lzma.open(path, "rt", encoding="utf-8") as lines:
        for line in lines:
            yield json.loads(line)


def rows(path: Path) -> set[tuple[str, str, str, str]]:
    """
    Rows of the table

    Arguments:
        path (Path): File of the snapshot

    Returns:
        set[tuple[str, str, str, str]]: Lemma, part of speech, form, features in CoNLL-U notation
    """
    entries = {entry["word"] for entry in snapshot_entries(path)}
    lower_entries = {word for word in entries if word == word.lower()}
    table = set()
    # A participle may refer to its masculine form, as `hablada` to `hablado`: its verb is `hablar`
    participles = {}
    for entry in snapshot_entries(path):
        for lemma, pos, form, feats in entry_rows(entry, entries, lower_entries):
            lemma, form = clean(lemma), clean(form)
            if lemma and form:
                text = "|".join(f"{name}={feats[name]}" for name in sorted(feats, key=str.lower))
                table.add((lemma, pos, form, text or "_"))
                if pos == "VERB" and text == MASCULINE_PARTICIPLE and lemma != form:
                    participles.setdefault(form, lemma)
    table = {
        (participles.get(lemma, lemma) if pos == "VERB" else lemma, pos, form, text)
        for lemma, pos, form, text in table
    }
    # An entry of a plural form may not name a gender that the table of its noun names
    gendered = {
        (lemma, form, text.partition("|")[2])
        for lemma, pos, form, text in table
        if pos == "NOUN" and text.startswith("Gender=")
    }
    return {
        row for row in table if not (row[1] == "NOUN" and (row[0], row[2], row[3]) in gendered)
    }


def export(output: Path) -> int:
    """
    Writing the table and the description of its source

    Arguments:
        output (Path): Directory of the export

    Returns:
        int: Number of rows
    """
    name, url, digest = SNAPSHOT
    table = rows(download(url, output / "raw" / name, digest))
    with (output / "lexemes.tsv").open("w", encoding="utf-8", newline="\n") as out:
        out.writelines("\t".join(row) + "\n" for row in sorted(table))
    meta = {"source": "Wiktionary", "snapshot": name, "sha256": digest, "license": LICENSE}
    (output / "meta.json").write_text(json.dumps(meta, sort_keys=True), encoding="utf-8")
    return len(table)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=(__doc__ or "").strip().splitlines()[0])
    parser.add_argument("output", type=Path, help="directory of the export")
    print(export(parser.parse_args().output))
