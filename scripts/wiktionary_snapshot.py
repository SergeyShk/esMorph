"""
Snapshot of the Spanish entries of the English Wiktionary for building the dictionary of esMorph

Description:
    Reads the extract of en.wiktionary.org made by wiktextract (kaikki.org, the file
    kaikki.org-dictionary-Spanish.jsonl.gz) and writes JSON Lines compressed with xz: the word and
    the part of speech of an entry, its forms with tags and the tags and references of its senses.
    Forms with clitics are left out: the analyzer splits clitics by a rule. The snapshot goes to
    a dataset on Hugging Face because kaikki.org keeps only the latest extract. The data of
    Wiktionary is under the CC BY-SA 4.0 license.

Example:
    python scripts/wiktionary_snapshot.py kaikki.org-dictionary-Spanish.jsonl.gz es-wiktionary.jsonl.xz
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


def snapshot(source: Path, output: Path) -> int:
    """
    Snapshot of the entries

    Arguments:
        source (Path): Extract of wiktextract compressed with gzip
        output (Path): File of the snapshot

    Returns:
        int: Number of entries in the snapshot
    """
    count = 0
    with (
        gzip.open(source, "rt", encoding="utf-8") as lines,
        lzma.open(output, "wt", encoding="utf-8", preset=9) as out,
    ):
        for line in lines:
            entry = json.loads(line)
            record = {
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
                    for sense in entry.get("senses", [])
                ],
            }
            out.write(json.dumps(record, ensure_ascii=False) + "\n")
            count += 1
    return count


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=(__doc__ or "").strip().splitlines()[0])
    parser.add_argument("source", type=Path, help="extract of wiktextract (.jsonl.gz)")
    parser.add_argument("output", type=Path, help="file of the snapshot (.jsonl.xz)")
    args = parser.parse_args()
    print(snapshot(args.source, args.output))
