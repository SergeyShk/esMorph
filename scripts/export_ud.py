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

RELEASE = "r2.19"
LICENSES = {"UD_Spanish-AnCora": "CC BY 4.0", "UD_Spanish-GSD": "CC BY-SA 4.0"}
# Treebank, commit of the release, file and its SHA-256
SOURCES = {
    "UD_Spanish-AnCora": (
        "bdb41b5f95db8e0a64217fcb8d15cd3ad38c34db",
        "es_ancora-ud-train.conllu",
        "acb96e0b3d28d8ae0288e0fb0a2d5243bd352c86bcecc0773deef5f40e612f83",
    ),
    "UD_Spanish-GSD": (
        "c262f3ca7897ea3b7719731d5958948614e3a214",
        "es_gsd-ud-train.conllu",
        "f8e0c5ba75d33f78ab7299355924ea9923067a4554a5eff4e04b71064b1835fd",
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
