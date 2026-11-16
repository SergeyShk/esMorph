"""
Entries of the Spanish edition of Wiktionary in the shape of those of the English one

Description:
    The snapshot of es.wiktionary.org (`wiktionary_snapshot.py --edition es`) keeps what its
    entries say in their own way: the grammar of a form only in the gloss of its sense ("Forma del
    plural de casa", "Primera persona del singular (yo) del presente de indicativo de comer"), the
    word a spelling refers to also in the gloss ("Variante obsoleta de mano"), the gender of a noun
    in the title of its entry ("Sustantivo femenino"), the preterite in the conjugation tables as
    `present perfect` and the imperfect as `past imperfect`. `entries` gives them with the tags and
    references of the English edition, so that the export reads both editions alike. Only nouns,
    adjectives, adverbs, interjections and verbs are kept; the forms of the conjugation tables
    that have their own entries and the phrases are left to the export.
"""

import json
import lzma
import re
from collections.abc import Iterator
from pathlib import Path

# The dump of es.wiktionary.org of 2026-11-01, extracted by wiktextract on 2026-11-02 (kaikki.org)
SNAPSHOT = (
    "es-eswiktionary-20261101.jsonl.xz",
    "https://huggingface.co/datasets/SergeyShk/esmorph-wiktionary/resolve/10611df301f35da5b60a54ade1a84e748648950c/es-eswiktionary-20261101.jsonl.xz",
    "b35093f1bb71f5096e4c4b8997f13f542a5f9d6f6236e0d88674609897969d23",
)
PARTS_OF_SPEECH = {"adj", "adv", "intj", "noun", "verb"}
GENDERS = {"femenin": "feminine", "masculin": "masculine"}
PERSONS = {"Primera": "first-person", "Segunda": "second-person", "Tercera": "third-person"}
# Times of the glosses of verb forms, the longer first
TIMES = (
    ("pretérito imperfecto de subjuntivo", {"imperfect", "subjunctive"}),
    ("pretérito imperfecto de indicativo", {"imperfect", "indicative"}),
    ("pretérito perfecto simple de indicativo", {"preterite", "indicative"}),
    ("presente de subjuntivo", {"present", "subjunctive"}),
    ("presente de indicativo", {"present", "indicative"}),
    ("futuro de subjuntivo", {"future", "subjunctive"}),
    ("futuro de indicativo", {"future", "indicative"}),
    ("condicional", {"conditional"}),
    ("imperativo", {"imperative"}),
)
VERB_FORM = re.compile(
    r"^(Primera|Segunda|Tercera) persona del (singular|plural) (?:\([^)]*\) )?del (.+) de [^ ]+?\.?$"
)
NOMINAL_FORM = re.compile(r"^Forma del ((?:femenino|masculino|singular|plural)(?: \w+)?) de ")
NOMINAL_TAGS = {
    "femenino": "feminine",
    "masculino": "masculine",
    "singular": "singular",
    "plural": "plural",
}
# A sense that only spells another word: "Variante obsoleta de mano", "Grafía alternativa de güija"
SPELLING = re.compile(r"^(?:Variante|Grafía)\b[^.,;:()]*? de ([^\s.,;:()]+)")
# Labels of such a sense in the tags of the English edition
SPELLING_TAGS = (
    ("anticuad", "archaic"),
    ("obsolet", "obsolete"),
    ("poco usad", "rare"),
    ("subestándar", "nonstandard"),
    ("informal", "colloquial"),
)


def verb_tags(gloss: str) -> set[str] | None:
    """
    Tags of a verb form by its gloss

    Arguments:
        gloss (str): The gloss: "Tercera persona del plural (ellos, ellas; ustedes) del futuro de
            indicativo de comer"

    Returns:
        set[str] | None: The person, the number, the mood and the tense; `None` for a gloss of
        another kind
    """
    match = VERB_FORM.match(gloss.strip())
    if not match:
        return None
    person, number, time = match.groups()
    tags = next((tags for text, tags in TIMES if time.startswith(text)), None)
    return None if tags is None else {PERSONS[person], number} | tags


def nominal_tags(gloss: str) -> set[str] | None:
    """
    Tags of a form of a noun or an adjective by its gloss

    Arguments:
        gloss (str): The gloss: "Forma del femenino plural de bonito"

    Returns:
        set[str] | None: The gender and the number; `None` for a gloss of another kind
    """
    match = NOMINAL_FORM.match(gloss.strip())
    if not match:
        return None
    return {NOMINAL_TAGS[word] for word in match[1].split() if word in NOMINAL_TAGS}


def table_tags(tags: list[str]) -> list[str]:
    """
    Tags of a row of a conjugation table as the English edition names them

    Arguments:
        tags (list[str]): Tags of the row

    Returns:
        list[str]: The tags: the preterite instead of `present perfect`, the imperfect without
        `past`, the imperative without `present`, the infinitive, the gerund and the participle
        without `impersonal`
    """
    found = set(tags) - {"impersonal"}
    if "imperfect" in found:
        found -= {"past", "present", "perfect"}
    elif {"present", "perfect"} <= found:
        found = found - {"present", "perfect"} | {"preterite"}
    elif "imperative" in found:
        found -= {"present"}
    return sorted(found)


def sense(record: dict, pos: str, gender: set[str]) -> dict | None:
    """
    A sense in the shape of the English edition

    Arguments:
        record (dict): Sense of the snapshot
        pos (str): Part of speech of its entry
        gender (set[str]): Genders of its entry

    Returns:
        dict | None: Tags, `form_of` and `alt_of`; `None` for a form whose gloss names no grammar
    """
    tags = set(record["tags"]) - {"form-of"}
    gloss = " ".join(record.get("glosses", []))
    if record["form_of"]:
        found = verb_tags(gloss) if pos == "verb" else nominal_tags(gloss)
        if found is None:
            return None
        return {"tags": sorted(tags | found), "form_of": record["form_of"][:1], "alt_of": []}
    spelling = SPELLING.match(gloss) if pos != "verb" else None
    if spelling:
        head = gloss[: spelling.start(1)]
        tags |= {tag for text, tag in SPELLING_TAGS if text in head}
    return {
        "tags": sorted(tags | gender),
        "form_of": [],
        "alt_of": [spelling[1]] if spelling else [],
    }


def entry(record: dict) -> dict | None:
    """
    An entry in the shape of the English edition

    Arguments:
        record (dict): Entry of the snapshot

    Returns:
        dict | None: Word, part of speech, forms and senses; `None` for a part of speech that the
        export does not take from this edition
    """
    pos = record["pos"]
    if pos not in PARTS_OF_SPEECH:
        return None
    title = record.get("title", "").lower()
    gender = {tag for text, tag in GENDERS.items() if text in title} if pos == "noun" else set()
    senses = [sense(item, pos, gender) for item in record["senses"]]
    forms = [[form, table_tags(tags)] for form, tags in record["forms"]]
    # A conjugation table has the infinitive only in the compound form (`haber comido`), while
    # the export tells a table of a reflexive verb by a missing infinitive
    if pos == "verb" and forms:
        forms.insert(0, [record["word"], ["infinitive"]])
    return {
        "word": record["word"],
        "pos": pos,
        "forms": forms,
        "senses": [item for item in senses if item is not None],
    }


def entries(path: Path) -> Iterator[dict]:
    """
    Entries of the snapshot of the Spanish edition in the shape of the English one

    Arguments:
        path (Path): File of the snapshot

    Returns:
        Iterator[dict]: The entries of the parts of speech that the export takes
    """
    with lzma.open(path, "rt", encoding="utf-8") as lines:
        for line in lines:
            found = entry(json.loads(line))
            if found is not None:
                yield found
