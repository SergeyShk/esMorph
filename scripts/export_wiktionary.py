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
import time
import unicodedata
import urllib.error
import urllib.request
from collections.abc import Iterable, Iterator
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


def verb_features(tags: set[str]) -> dict[str, str]:
    """
    Universal Dependencies features of a verb form

    Arguments:
        tags (set[str]): Tags of the form in wiktextract

    Returns:
        dict[str, str]: The features
    """
    if "infinitive" in tags:
        return {"VerbForm": "Inf"}
    if "gerund" in tags:
        return {"VerbForm": "Ger"}
    if "participle" in tags:
        return {
            "VerbForm": "Part",
            "Tense": "Past",
            "Gender": "Fem" if "feminine" in tags else "Masc",
            "Number": "Plur" if "plural" in tags else "Sing",
        }
    feats = {"VerbForm": "Fin"}
    for tag, person in PERSONS.items():
        if tag in tags:
            feats["Person"] = person
    if "singular" in tags:
        feats["Number"] = "Sing"
    if "plural" in tags:
        feats["Number"] = "Plur"
    if "imperative" in tags:
        feats["Mood"] = "Imp"
    elif "conditional" in tags:
        feats["Mood"] = "Cnd"
    else:
        feats["Mood"] = "Sub" if "subjunctive" in tags else "Ind"
        for tag, tense in (
            ("present", "Pres"),
            ("preterite", "Past"),
            ("imperfect", "Imp"),
            ("future", "Fut"),
        ):
            if tag in tags:
                feats["Tense"] = tense
    return feats


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


def features(pos: str, tags: set[str]) -> dict[str, str]:
    """
    Features of a form of a part of speech

    Arguments:
        pos (str): Part of speech
        tags (set[str]): Tags of the form in wiktextract

    Returns:
        dict[str, str]: The features
    """
    if pos == "VERB":
        return verb_features(tags)
    if pos in NOMINAL:
        return nominal_features(tags, pos in SINGULAR_BASE)
    return {}


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
        has no clitics: `hazlo` has the tags `object-*`, `mirándose` has the tag `reflexive`
    """
    return (
        bool(tags & (NON_FINITE | set(PERSONS)))
        and not tags & OTHER_VERB_FORM
        and "reflexive" not in tags
        and not any(tag.startswith("object-") for tag in tags)
    )


def entry_rows(entry: dict) -> Iterator[tuple[str, str, str, dict[str, str]]]:
    """
    Rows of one entry

    Description:
        A sense with `form_of` gives the word as a form of that lemma, any other sense gives the
        word as a lemma. A derived word is a lemma of its own. The lemma of a feminine noun is both
        the word itself and its masculine form: treebanks use both. A word that only spells another
        one is a form of both. The forms of the table belong to an entry that has a sense of its
        own. An adjective with a feminine form is masculine in its base form

    Arguments:
        entry (dict): Entry of the snapshot

    Returns:
        Iterator[tuple[str, str, str, dict[str, str]]]: Lemma, part of speech, form, features
    """
    word, pos = entry["word"], PARTS_OF_SPEECH.get(entry["pos"])
    if pos is None:
        return
    forms = [(form, set(tags)) for form, tags in entry["forms"]]
    own = False
    base_tags = {"feminine", "masculine"}
    gender = {tag for sense in entry["senses"] if not sense["form_of"] for tag in sense["tags"]}
    gender &= base_tags
    if pos == "ADJ" and any("feminine" in tags and "plural" not in tags for _, tags in forms):
        gender = gender or {"masculine"}
    for sense in entry["senses"]:
        tags = set(sense["tags"])
        # Misspellings and spellings of the Internet: `hay` as `ahí`
        if "misspelling" in tags or ("Internet" in tags and sense["alt_of"]):
            continue
        if sense["form_of"]:
            if pos == "VERB" and not is_verb_form(tags):
                continue
            if not tags & DERIVED:
                yield sense["form_of"][0].split()[0], pos, word, features(pos, tags)
            if tags & DERIVED or (pos == "NOUN" and "feminine" in tags and "plural" not in tags):
                yield word, pos, word, features(pos, tags)
            continue
        own = True
        for target in sense["alt_of"]:
            yield target.split()[0], pos, word, features(pos, tags | gender)
        if pos == "VERB":
            yield word, pos, word, {"VerbForm": "Inf"}
        else:
            yield word, pos, word, features(pos, tags | gender)
    if not own:
        return
    for form, tags in forms:
        if pos == "VERB":
            if is_verb_form(tags):
                yield word, pos, form, verb_features(tags)
        elif tags and tags <= NOMINAL_FORM:
            value = form.removeprefix("before a noun ")
            if not tags & base_tags:
                tags = tags | gender
            yield word, pos, value, features(pos, tags)


def rows(entries: Iterable[dict]) -> set[tuple[str, str, str, str]]:
    """
    Rows of the table

    Arguments:
        entries (Iterable[dict]): Entries of the snapshot

    Returns:
        set[tuple[str, str, str, str]]: Lemma, part of speech, form, features in CoNLL-U notation
    """
    table = set()
    # A participle may refer to its masculine form, as `hablada` to `hablado`: its verb is `hablar`
    participles = {}
    for entry in entries:
        for lemma, pos, form, feats in entry_rows(entry):
            lemma, form = clean(lemma), clean(form)
            if lemma and form:
                text = "|".join(f"{name}={feats[name]}" for name in sorted(feats, key=str.lower))
                table.add((lemma, pos, form, text or "_"))
                if pos == "VERB" and text == MASCULINE_PARTICIPLE and lemma != form:
                    participles.setdefault(form, lemma)
    return {
        (participles.get(lemma, lemma) if pos == "VERB" else lemma, pos, form, text)
        for lemma, pos, form, text in table
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
    snapshot = download(url, output / "raw" / name, digest)
    with lzma.open(snapshot, "rt", encoding="utf-8") as lines:
        table = rows(json.loads(line) for line in lines)
    with (output / "lexemes.tsv").open("w", encoding="utf-8", newline="\n") as out:
        out.writelines("\t".join(row) + "\n" for row in sorted(table))
    meta = {"source": "Wiktionary", "snapshot": name, "sha256": digest, "license": LICENSE}
    (output / "meta.json").write_text(json.dumps(meta, sort_keys=True), encoding="utf-8")
    return len(table)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=(__doc__ or "").strip().splitlines()[0])
    parser.add_argument("output", type=Path, help="directory of the export")
    print(export(parser.parse_args().output))
