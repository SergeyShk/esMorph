"""
Snapshot of the Spanish entries of Wiktionary for building the dictionary of esMorph

Description:
    Reads an extract of Wiktionary made by wiktextract (kaikki.org: the file
    kaikki.org-dictionary-Spanish.jsonl.gz of en.wiktionary.org or the file of the Spanish words of
    es.wiktionary.org) and writes JSON Lines compressed with xz: the word and the part of speech of
    an entry, its forms with tags and the tags and references of its senses. The Spanish edition
    keeps the grammar of a form and the word a spelling refers to only in the text of a sense, so
    its snapshot also has the title and the tags of an entry and the glosses of its senses. Forms
    with clitics are left out: the analyzer splits clitics by a rule. The snapshot goes to a
    dataset on Hugging Face because kaikki.org keeps only the latest extract. The data of
    Wiktionary is under the CC BY-SA 4.0 license.

Example:
    python scripts/wiktionary_snapshot.py kaikki.org-dictionary-Spanish.jsonl.gz es-wiktionary.jsonl.xz
    python scripts/wiktionary_snapshot.py --edition es kaikki.org-dictionary-Español.jsonl.gz \
        es-eswiktionary.jsonl.xz
"""

import argparse
import gzip
import json
import lzma
from pathlib import Path

# Rows of the conjugation tables that hold no form, forms of a verb with clitics
SKIPPED_FORMS = {"table-tags", "inflection-template", "combined-form"}


def targets(sense: dict, name: str) -> list[str]:
    """
    Words a sense refers to

    Arguments:
        sense (dict): Sense of an entry
        name (str): `form_of` or `alt_of`

    Returns:
        list[str]: The words
    """
    return [link["word"] for link in sense.get(name, []) if link.get("word")]


def record(entry: dict, edition: str) -> dict:
    """
    Record of an entry in the snapshot

    Arguments:
        entry (dict): Entry of the extract
        edition (str): Language of the edition: `en` or `es`

    Returns:
        dict: The word, the part of speech, the forms and the senses; for the Spanish edition also
        the title and the tags of the entry and the glosses of the senses
    """
    found = {
        "word": entry["word"],
        "pos": entry.get("pos", ""),
        "forms": [
            [form["form"], form.get("tags", [])]
            for form in entry.get("forms", [])
            if form.get("form") and not SKIPPED_FORMS & set(form.get("tags", []))
        ],
        "senses": [
            {
                "tags": sense.get("tags", []),
                "form_of": targets(sense, "form_of"),
                "alt_of": targets(sense, "alt_of"),
            }
            | ({"glosses": sense.get("glosses", [])} if edition == "es" else {})
            for sense in entry.get("senses", [])
        ],
    }
    if edition == "es":
        found |= {"title": entry.get("pos_title", ""), "tags": entry.get("tags", [])}
    return found


def snapshot(source: Path, output: Path, edition: str = "en") -> int:
    """
    Snapshot of the entries

    Arguments:
        source (Path): Extract of wiktextract compressed with gzip
        output (Path): File of the snapshot
        edition (str): Language of the edition: `en` or `es`

    Returns:
        int: Number of entries in the snapshot
    """
    count = 0
    with (
        gzip.open(source, "rt", encoding="utf-8") as lines,
        lzma.open(output, "wt", encoding="utf-8", preset=9) as out,
    ):
        for line in lines:
            out.write(json.dumps(record(json.loads(line), edition), ensure_ascii=False) + "\n")
            count += 1
    return count


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=(__doc__ or "").strip().splitlines()[0])
    parser.add_argument("source", type=Path, help="extract of wiktextract (.jsonl.gz)")
    parser.add_argument("output", type=Path, help="file of the snapshot (.jsonl.xz)")
    parser.add_argument(
        "--edition", choices=["en", "es"], default="en", help="edition of Wiktionary"
    )
    args = parser.parse_args()
    print(snapshot(args.source, args.output, args.edition))
