"""
Counts of the analyses of word forms in the treebanks of Universal Dependencies

Description:
    Downloads the training parts of UD_Spanish-AnCora and UD_Spanish-GSD of a pinned release with
    their checksums checked and writes how many times every word form in lower case has each
    analysis in AnCora (`analyses.tsv`) and each part of speech in both (`pos.tsv`): the lemmas and
    features of GSD are automatic. The builder orders analyses by them. Downloaded files stay in the
    directory of the export

Example:
    uv run python scripts/export_ud.py target/ud
"""

import argparse
import json
import unicodedata
from collections import Counter
from collections.abc import Iterator
from pathlib import Path

from conventions import format_features, parse_features
from export_wiktionary import download

RELEASE = "r2.18"
LICENSES = {"UD_Spanish-AnCora": "CC BY 4.0", "UD_Spanish-GSD": "CC BY-SA 4.0"}
# Treebank, commit of the release, file and its SHA-256
SOURCES = {
    "UD_Spanish-AnCora": (
        "197cca385e0e7db1b1fe26a5772dade1b6fbbee8",
        "es_ancora-ud-train.conllu",
        "47b6fc49d6ed48a016e7fa5ad9f5c7ecf3ec21e9db005df3ea14d10f07c6deb6",
    ),
    "UD_Spanish-GSD": (
        "6364fa8cf5420ff7156e6b383d25936d759c68a3",
        "es_gsd-ud-train.conllu",
        "9e527de0e45dca4a8ef00105a3ebd102da941964b35f01916a6baa6288130f7b",
    ),
}
# The features of the dictionary: the others describe the text, not the word form
FEATURES = {
    "Case",
    "Definite",
    "Degree",
    "Gender",
    "Mood",
    "NumType",
    "Number",
    "Person",
    "Polarity",
    "Polite",
    "Poss",
    "PrepCase",
    "PronType",
    "Reflex",
    "Tense",
    "VerbForm",
}


def words(path: Path) -> Iterator[tuple[str, str, str, str]]:
    """
    Words of a treebank: multiword tokens, empty nodes and foreign words are left out

    Arguments:
        path (Path): File in CoNLL-U

    Returns:
        Iterator[tuple[str, str, str, str]]: Form, lemma, part of speech, features; form and lemma
        as the dictionary keeps them
    """
    with path.open(encoding="utf-8") as lines:
        for line in lines:
            fields = line.split("\t")
            if len(fields) != 10 or not fields[0].isdigit():
                continue
            feats = parse_features(fields[5])
            if "Foreign" in feats:
                continue
            kept = {name: value for name, value in feats.items() if name in FEATURES}
            form, lemma = (
                unicodedata.normalize("NFC", text.lower()) for text in (fields[1], fields[2])
            )
            yield form, lemma, fields[3], format_features(kept)


def export(output: Path) -> None:
    """
    Writing the counts and the description of their sources

    Arguments:
        output (Path): Directory of the export
    """
    analyses: Counter[tuple[str, str, str, str]] = Counter()
    parts: Counter[tuple[str, str]] = Counter()
    for treebank, (commit, name, digest) in SOURCES.items():
        url = f"https://raw.githubusercontent.com/UniversalDependencies/{treebank}/{commit}/{name}"
        for form, lemma, pos, feats in words(download(url, output / "raw" / name, digest)):
            parts[form, pos] += 1
            if treebank == "UD_Spanish-AnCora":
                analyses[form, pos, lemma, feats] += 1
    with (output / "analyses.tsv").open("w", encoding="utf-8", newline="\n") as out:
        for key, count in sorted(analyses.items()):
            out.write("\t".join(key) + f"\t{count}\n")
    with (output / "pos.tsv").open("w", encoding="utf-8", newline="\n") as out:
        for (form, pos), count in sorted(parts.items()):
            out.write(f"{form}\t{pos}\t{count}\n")
    sources = {"release": RELEASE, "treebanks": LICENSES}
    (output / "sources.json").write_text(json.dumps(sources, sort_keys=True), encoding="utf-8")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=(__doc__ or "").strip().splitlines()[0])
    parser.add_argument("output", type=Path, help="directory of the export")
    export(parser.parse_args().output)
