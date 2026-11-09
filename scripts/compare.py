# /// script
# requires-python = ">=3.11,<3.14"
# dependencies = [
#     "esmorph",
#     "simplemma==2.0.0",
#     "spacy==3.8.16",
#     "es-core-news-sm @ https://github.com/explosion/spacy-models/releases/download/es_core_news_sm-3.8.0/es_core_news_sm-3.8.0-py3-none-any.whl",
#     "stanza==1.15.0",
#     "ablaut==0.11.0",
#     "verbecc==2.0.3",
# ]
#
# [[tool.uv.index]]
# name = "testpypi"
# url = "https://test.pypi.org/simple/"
# explicit = true
#
# [tool.uv.sources]
# esmorph = { index = "testpypi" }
# ///
"""
Comparison of esMorph with other tools: accuracy on the test parts of UD treebanks, verb
inflection and speed

Description:
    Downloads the test parts of UD_Spanish-AnCora, UD_Spanish-GSD, UD_Spanish-PUD and
    UD_Spanish-COSER of a pinned UD release with their checksums checked and the Stanza model into
    the data directory, and analyzes the gold words with every system. esMorph analyzes a word
    without context and is scored by its first analysis and by the best of its analyses (oracle);
    simplemma gives only a lemma; spaCy and Stanza tag a sentence and give one analysis per word.
    The lemmas and features of GSD are automatic, so only the part of speech is compared on it.
    Inflection: the gold form of a verb of AnCora, PUD and COSER from its gold lemma and features,
    by esMorph, ablaut and verbecc. Speed - tokens per second on AnCora test without loading (the
    best of two runs) and, for the chart, the whole time with import and loading in a new process
    on texts of different size. Writes Markdown tables,
    results.json in the data directory (a run of some systems updates only their results) and
    the chart in SVG. The versions of all packages are pinned in compare.py.lock; esMorph is the
    version from it or a wheel from `--with`

Example:
    uv run scripts/compare.py target/compare --chart docs/img/speed.svg
    uv run --with dist/esmorph-*.whl scripts/compare.py target/compare --systems esmorph simplemma
"""

import argparse
import hashlib
import json
import math
import os
import subprocess
import sys
import time
import unicodedata
import urllib.request
from collections.abc import Callable
from dataclasses import dataclass
from importlib import metadata
from pathlib import Path
from typing import NamedTuple

# Treebank, file, commit of the UD 2.18 release and SHA-256
DATASETS = {
    "AnCora": (
        "UD_Spanish-AnCora",
        "es_ancora-ud-test.conllu",
        "197cca385e0e7db1b1fe26a5772dade1b6fbbee8",
        "679f1c05ca654aa26801c80319df45f8f97606718fb534b91755b48f3e67fa6f",
    ),
    "GSD": (
        "UD_Spanish-GSD",
        "es_gsd-ud-test.conllu",
        "6364fa8cf5420ff7156e6b383d25936d759c68a3",
        "ecce253f44bffaa9803ae7ec0c10911c13e4f1fc3cb439dbd1db2cebe0a12741",
    ),
    "PUD": (
        "UD_Spanish-PUD",
        "es_pud-ud-test.conllu",
        "8af7ac68bbd846bc47e525c3193d96f123dd388b",
        "48a7b5c7f409100b24eba90c7397e01f6fecaaf3207315a9a1bda029f77e98d3",
    ),
    "COSER": (
        "UD_Spanish-COSER",
        "es_coser-ud-test.conllu",
        "253575e3acab8f7b91cdf525004bf71d0390b3d1",
        "9bbbd54bb70ac448d6178b5446c70bef7d9285f93735a02791663872fc489fec",
    ),
}
# Sets whose lemmas and features are assigned automatically: only the part of speech is compared
AUTOMATIC = {"GSD"}
# Features compared: the others are absent in some sets or describe the text, not the word form
CORE = ("Number", "Gender", "Person", "Tense", "Mood", "VerbForm")
# Parts of speech whose features differ between the sets (PUD gives names and numerals a number)
NO_FEATURES = {"PROPN", "NUM"}
# Stanza processors: the words are given, the part of speech, features and lemma are needed
STANZA_PROCESSORS = "tokenize,pos,lemma"
# Text sizes for the speed chart, tokens
SIZES = (10, 100, 1_000, 10_000, 50_000)
# Persons in the order of the conjugation tables
PERSONS = (
    ("1", "Sing"),
    ("2", "Sing"),
    ("3", "Sing"),
    ("1", "Plur"),
    ("2", "Plur"),
    ("3", "Plur"),
)


@dataclass
class Token:
    word: str
    lemma: str
    pos: str
    feats: dict[str, str]


# Analysis of a word: lemma, UD part of speech and UD features (simplemma gives only the lemma)
Candidate = tuple[str, str | None, dict[str, str] | None]


def download(url: str, path: Path, digest: str) -> Path:
    """
    Download of a file with its checksum checked

    Arguments:
        url (str): Address of the file
        path (Path): Where to save it
        digest (str): SHA-256 of the file

    Returns:
        Path: Path to the file

    Raises:
        ValueError: If the checksum does not match
    """
    if path.exists() and hashlib.sha256(path.read_bytes()).hexdigest() == digest:
        return path
    with urllib.request.urlopen(url, timeout=300) as response:
        data = response.read()
    if hashlib.sha256(data).hexdigest() != digest:
        raise ValueError(f"{url}: the checksum does not match")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(data)
    return path


def sentences(path: Path) -> list[list[Token]]:
    """
    Sentences of a CoNLL-U file: syntactic words without multiword tokens and empty nodes

    Arguments:
        path (Path): File in CoNLL-U

    Returns:
        list[list[Token]]: Sentences
    """
    result: list[list[Token]] = [[]]
    for line in path.read_text(encoding="utf-8").splitlines():
        if not line:
            if result[-1]:
                result.append([])
            continue
        cols = line.split("\t")
        if line.startswith("#") or "-" in cols[0] or "." in cols[0]:
            continue
        feats = dict(f.split("=", 1) for f in cols[5].split("|")) if cols[5] != "_" else {}
        result[-1].append(Token(cols[1], cols[2], cols[3], feats))
    return [sentence for sentence in result if sentence]


def gold(data: Path) -> dict[str, list[list[Token]]]:
    """
    Test parts of the treebanks, downloaded into the data directory

    Arguments:
        data (Path): Data directory

    Returns:
        dict[str, list[list[Token]]]: Set - sentences
    """
    result = {}
    for dataset, (treebank, name, commit, digest) in DATASETS.items():
        url = f"https://raw.githubusercontent.com/UniversalDependencies/{treebank}/{commit}/{name}"
        result[dataset] = sentences(download(url, data / name, digest))
    return result


def prepare(name: str, data: Path) -> None:
    """
    Models of a system in the data directory, before the measurements

    Arguments:
        name (str): Name of the system
        data (Path): Data directory
    """
    if name == "stanza":
        import stanza

        directory = str(data / "stanza")
        stanza.download("es", model_dir=directory, processors=STANZA_PROCESSORS, verbose=False)


Analyze = Callable[[list[list[str]]], object]
Convert = Callable[[list[list[str]], object], list[list[list[Candidate]]]]


class System(NamedTuple):
    """Analysis of sentences by a system with its own API and conversion of its answer into
    analyses"""

    analyze: Analyze
    convert: Convert


def esmorph_system(data: Path) -> System:
    """esMorph: all analyses of a word, the most probable first"""
    from esmorph import Analyzer

    morph = Analyzer()

    def convert(text: list[list[str]], found: object) -> list[list[list[Candidate]]]:
        analyses = iter(found)
        # A word without analyses keeps its lowercase form as the lemma, as `lemmatize` gives it
        return [
            [
                [(a.lemma, a.pos, dict(a.feats)) for a in next(analyses)] or [(w.lower(), "", {})]
                for w in s
            ]
            for s in text
        ]

    return System(lambda text: morph.analyze_many([w for s in text for w in s]), convert)


def simplemma_system(data: Path) -> System:
    """simplemma: the lemma of a word; its cache of answers starts empty in every run"""
    import simplemma

    simplemma.lemmatize("casa", lang="es")

    def analyze(text: list[list[str]]) -> object:
        lemmatize = simplemma.Lemmatizer().lemmatize
        return [[lemmatize(word, "es") for word in sentence] for sentence in text]

    def convert(text: list[list[str]], found: object) -> list[list[list[Candidate]]]:
        return [[[(lemma, None, None)] for lemma in s] for s in found]

    return System(analyze, convert)


def spacy_system(data: Path) -> System:
    """spaCy es_core_news_sm: tagger and lemmatizer on the gold words"""
    import spacy
    from spacy.tokens import Doc

    nlp = spacy.load("es_core_news_sm", exclude=["parser", "ner"])

    def analyze(text: list[list[str]]) -> object:
        return list(nlp.pipe(Doc(nlp.vocab, words=sentence) for sentence in text))

    def convert(text: list[list[str]], docs: object) -> list[list[list[Candidate]]]:
        return [[[(t.lemma_, t.pos_, t.morph.to_dict())] for t in doc] for doc in docs]

    return System(analyze, convert)


def stanza_system(data: Path) -> System:
    """Stanza: tagger and lemmatizer of the default model on the gold words"""
    import stanza

    nlp = stanza.Pipeline(
        "es",
        dir=str(data / "stanza"),
        processors=STANZA_PROCESSORS,
        tokenize_pretokenized=True,
        download_method=None,
        use_gpu=False,
        verbose=False,
    )

    def feats(word) -> dict[str, str]:
        return dict(f.split("=", 1) for f in (word.feats or "").split("|") if f)

    def convert(text: list[list[str]], doc: object) -> list[list[list[Candidate]]]:
        return [[[(w.lemma or "", w.upos, feats(w))] for w in s.words] for s in doc.sentences]

    return System(nlp, convert)


SYSTEMS = {
    "esmorph": esmorph_system,
    "simplemma": simplemma_system,
    "spacy": spacy_system,
    "stanza": stanza_system,
}
CONJUGATORS = ("esmorph", "ablaut", "verbecc")
TITLES = {
    "esmorph": "esMorph",
    "simplemma": "simplemma",
    "spacy": "spaCy es_core_news_sm",
    "stanza": "Stanza",
    "ablaut": "ablaut",
    "verbecc": "verbecc",
}


def norm(lemma: str) -> str:
    return unicodedata.normalize("NFC", lemma).lower()


def core(token: Token, feats: dict[str, str]) -> tuple[str | None, ...]:
    return () if token.pos in NO_FEATURES else tuple(feats.get(name) for name in CORE)


def check(token: Token, candidate: Candidate) -> dict[str, bool | None]:
    """
    Right answers of the analysis of a word by metrics

    Arguments:
        token (Token): Gold word
        candidate (Candidate): Analysis

    Returns:
        dict[str, bool | None]: Metric - whether it is right; `None` if the system does not give it
    """
    lemma, pos, feats = candidate
    lemma_ok = norm(lemma) == norm(token.lemma)
    if pos is None or feats is None:
        return {"lemma": lemma_ok, "pos": None, "pos+lemma": None, "feats": None, "all": None}
    feats_ok = core(token, feats) == core(token, token.feats)
    return {
        "lemma": lemma_ok,
        "pos": pos == token.pos,
        "pos+lemma": lemma_ok and pos == token.pos,
        "feats": feats_ok,
        "all": lemma_ok and pos == token.pos and feats_ok,
    }


def scores(
    gold: list[list[Token]], found: list[list[list[Candidate]]], automatic: bool
) -> dict[str, float | None]:
    """
    Shares of right answers of the first analysis and of the oracle, without punctuation

    Arguments:
        gold (list[list[Token]]): Gold sentences
        found (list[list[list[Candidate]]]): Analyses of the words
        automatic (bool): The lemmas and features of the set are automatic: only the part of
            speech is compared

    Returns:
        dict[str, float | None]: Metric - share in percent; `None` if the system does not give it
    """
    hits: dict[str, int] = {}
    total = 0
    given: dict[str, bool] = {}
    for sentence, analyses in zip(gold, found, strict=True):
        for token, candidates in zip(sentence, analyses, strict=True):
            if token.pos == "PUNCT":
                continue
            total += 1
            for key, ok in check(token, candidates[0]).items():
                given[key] = ok is not None
                hits[key] = hits.get(key, 0) + bool(ok)
            oracle = any(check(token, candidate)["pos+lemma"] for candidate in candidates)
            hits["oracle"] = hits.get("oracle", 0) + oracle
            given["oracle"] = given["pos"]
    result: dict[str, float | None] = {
        key: 100 * value / total if given[key] else None for key, value in hits.items()
    }
    if automatic:
        result = {key: value if key == "pos" else None for key, value in result.items()}
    return result


def verb_key(feats: dict[str, str]) -> str | None:
    """Cell of a conjugation table for the features of a verb: `None` if the features do not
    define one"""
    form = feats.get("VerbForm")
    if form in ("Inf", "Ger"):
        return form
    if form == "Part":
        return f"Part|{feats.get('Gender', 'Masc')}|{feats.get('Number', 'Sing')}"
    if form == "Fin" and all(name in feats for name in ("Mood", "Person", "Number")):
        tense = feats.get("Tense", "-") if feats["Mood"] in ("Ind", "Sub") else "-"
        return f"Fin|{feats['Mood']}|{tense}|{feats['Person']}|{feats['Number']}"
    return None


def verb_feats(key: str) -> dict[str, str]:
    """Features of a cell of a conjugation table"""
    kind, *rest = key.split("|")
    if kind in ("Inf", "Ger"):
        return {"VerbForm": kind}
    if kind == "Part":
        return {"VerbForm": "Part", "Gender": rest[0], "Number": rest[1]}
    mood, tense, person, number = rest
    feats = {"VerbForm": "Fin", "Mood": mood, "Person": person, "Number": number}
    if tense != "-":
        feats["Tense"] = tense
    return feats


def verb_tasks(text: list[list[Token]]) -> list[tuple[str, str, str, str]]:
    """
    Inflection tasks: verbs and auxiliaries with an infinitive as the lemma, without typos and
    foreign words

    Arguments:
        text (list[list[Token]]): Gold sentences

    Returns:
        list[tuple[str, str, str, str]]: Lemma, part of speech, cell of the table, gold form
    """
    result = []
    for sentence in text:
        for token in sentence:
            if token.pos not in ("VERB", "AUX") or {"Typo", "Foreign"} & token.feats.keys():
                continue
            lemma = token.lemma.lower()
            key = verb_key(token.feats)
            if key and lemma.endswith(("ar", "er", "ir", "ír")):
                result.append((lemma, token.pos, key, token.word.lower()))
    return result


def participles(masculine: str) -> dict[str, list[str]]:
    stem = masculine[:-1]
    return {
        "Part|Masc|Sing": [masculine],
        "Part|Fem|Sing": [stem + "a"],
        "Part|Masc|Plur": [stem + "os"],
        "Part|Fem|Plur": [stem + "as"],
    }


Inflect = Callable[[str, str, str], list[str]]


def esmorph_conjugator() -> Inflect:
    """esMorph: `inflect` of the lexeme of the lemma with the gold part of speech, else of the verb;
    one form"""
    from esmorph import Analyzer, FormNotFoundError

    morph = Analyzer()

    def inflect(lemma: str, pos: str, key: str) -> list[str]:
        analyses = [a for a in morph.analyze(lemma) if a.lemma == lemma]
        lexeme = next((a for a in analyses if a.pos == pos), None)
        lexeme = lexeme or next((a for a in analyses if a.pos == "VERB"), None)
        if lexeme is None:
            return []
        try:
            return [lexeme.inflect(**verb_feats(key))]
        except FormNotFoundError:
            return []

    return inflect


def table_conjugator(table: Callable[[str], dict[str, list[str]]]) -> Inflect:
    """Forms of a cell of the conjugation table of the lemma, tables kept by lemma"""
    tables: dict[str, dict[str, list[str]]] = {}

    def inflect(lemma: str, pos: str, key: str) -> list[str]:
        if lemma not in tables:
            try:
                tables[lemma] = table(lemma)
            except Exception:
                tables[lemma] = {}
        return tables[lemma].get(key, [])

    return inflect


def ablaut_conjugator() -> Inflect:
    """ablaut: the conjugation table of the lemma"""
    import ablaut

    series = (
        ("present", "Ind", "Pres"),
        ("preterite", "Ind", "Past"),
        ("imperfect", "Ind", "Imp"),
        ("future", "Ind", "Fut"),
        ("conditional", "Cnd", "-"),
        ("subjunctive_present", "Sub", "Pres"),
        ("subjunctive_imperfect", "Sub", "Imp"),
        ("subjunctive_future", "Sub", "Fut"),
    )

    def table(lemma: str) -> dict[str, list[str]]:
        found = ablaut.conjugate(lemma, lang="spa")
        result = {"Inf": [found.infinitive], "Ger": [found.gerund]}
        result |= participles(found.past_participle)
        for name, mood, tense in series:
            for (person, number), form in zip(PERSONS, getattr(found, name), strict=True):
                result.setdefault(f"Fin|{mood}|{tense}|{person}|{number}", []).append(form)
        for (person, number), form in zip(PERSONS[1:], found.imperative, strict=True):
            result.setdefault(f"Fin|Imp|-|{person}|{number}", []).append(form)
        return result

    return table_conjugator(table)


def verbecc_conjugator() -> Inflect:
    """verbecc: the conjugation table of the lemma, the forms without pronouns"""
    from verbecc import CompleteConjugator

    conjugator = CompleteConjugator(lang="es")
    tenses = {
        ("indicativo", "presente"): ("Ind", "Pres"),
        ("indicativo", "pretérito-imperfecto"): ("Ind", "Imp"),
        ("indicativo", "pretérito-perfecto-simple"): ("Ind", "Past"),
        ("indicativo", "futuro"): ("Ind", "Fut"),
        ("subjuntivo", "presente"): ("Sub", "Pres"),
        ("subjuntivo", "pretérito-imperfecto-1"): ("Sub", "Imp"),
        ("subjuntivo", "pretérito-imperfecto-2"): ("Sub", "Imp"),
        ("subjuntivo", "futuro"): ("Sub", "Fut"),
        ("condicional", "presente"): ("Cnd", "-"),
        ("imperativo", "afirmativo"): ("Imp", "-"),
    }

    def table(lemma: str) -> dict[str, list[str]]:
        result: dict[str, list[str]] = {}
        for mood, by_tense in conjugator.conjugate(lemma).get_data()["moods"].items():
            for tense, rows in by_tense.items():
                name = mood.value
                if name == "infinitivo":
                    result["Inf"] = [rows[0]["c"][0]]
                elif name == "gerundio":
                    result["Ger"] = [rows[0]["c"][0]]
                elif name == "participo":
                    result |= participles(rows[0]["c"][0])
                elif (name, tense.value) in tenses:
                    ud_mood, ud_tense = tenses[name, tense.value]
                    for row in rows:
                        number = {"s": "Sing", "p": "Plur"}[row["n"].value]
                        key = f"Fin|{ud_mood}|{ud_tense}|{row['p'].value}|{number}"
                        result.setdefault(key, []).extend(f.split()[-1] for f in row["c"])
        return result

    return table_conjugator(table)


INFLECTORS = {
    "esmorph": esmorph_conjugator,
    "ablaut": ablaut_conjugator,
    "verbecc": verbecc_conjugator,
}


def inflection(name: str, texts: dict[str, list[list[Token]]]) -> dict[str, float]:
    """
    Share of verbs whose gold form the system gives for the gold lemma and features

    Arguments:
        name (str): Name of the system
        texts (dict[str, list[list[Token]]]): Set - gold sentences

    Returns:
        dict[str, float]: Set - share in percent
    """
    inflect = INFLECTORS[name]()
    result = {}
    for dataset, text in texts.items():
        if dataset in AUTOMATIC:
            continue
        tasks = verb_tasks(text)
        hits = sum(
            form in {f.lower() for f in inflect(lemma, pos, key)}
            for lemma, pos, key, form in tasks
        )
        result[dataset] = 100 * hits / len(tasks)
    return result


def speed_run(name: str, data: Path, size: int) -> None:
    """
    Measurement in a clean process: the time of importing and loading the system, of the first
    analysis of the first `size` tokens of AnCora test (0 - of all) and of the best of two
    analyses

    Arguments:
        name (str): Name of the system
        data (Path): Data directory
        size (int): Number of tokens
    """
    text: list[list[str]] = []
    count = 0
    for sentence in sentences(data / DATASETS["AnCora"][1]):
        words = [token.word for token in sentence][: size - count if size else None]
        text.append(words)
        count += len(words)
        if size and count >= size:
            break
    start = time.perf_counter()
    analyze = SYSTEMS[name](data).analyze
    loaded = time.perf_counter()
    analyze(text)
    first = time.perf_counter()
    analyze(text)
    best = min(first - loaded, time.perf_counter() - first)
    result = {
        "tokens": count,
        "load": loaded - start,
        "first": first - loaded,
        "best": best,
    }
    print(json.dumps(result))


def measure(name: str, data: Path, size: int) -> dict[str, float]:
    """
    `speed_run` in a new process

    Arguments:
        name (str): Name of the system
        data (Path): Data directory
        size (int): Number of tokens, 0 - all

    Returns:
        dict[str, float]: Tokens and times in seconds
    """
    run = subprocess.run(
        [sys.executable, __file__, str(data), "--speed-run", name, str(size)],
        capture_output=True,
        text=True,
        check=True,
        env={**os.environ, "PYTHONWARNINGS": "ignore"},
    )
    return json.loads(run.stdout.strip().splitlines()[-1])


def number(value: float, spanish: bool) -> str:
    """Number for the chart: groups of thousands, as many decimals as the first significant digit
    needs"""
    text = f"{value:,}" if value >= 1 else f"{value:.{-math.floor(math.log10(value))}f}"
    return text.replace(",", " ") if spanish else text


def chart(curves: dict[str, list[tuple[int, float]]], path: Path, spanish: bool = False) -> None:
    """
    Chart in SVG: the time of analyzing a text with loading by the size of the text, both axes
    logarithmic

    Arguments:
        curves (dict[str, list[tuple[int, float]]]): System - points "tokens, seconds"
        path (Path): SVG file
        spanish (bool): Labels and numbers in Spanish
    """
    width, height, left, right, top, bottom = 780, 420, 70, 240, 20, 50
    xs = [x for points in curves.values() for x, _ in points]
    ys = [y for points in curves.values() for _, y in points]
    x_range = (math.log10(min(xs)), math.log10(max(xs)))
    y_range = (math.floor(math.log10(min(ys))), math.ceil(math.log10(max(ys))))
    plot = (width - left - right, height - top - bottom)

    def to_x(value: float) -> float:
        return left + (math.log10(value) - x_range[0]) / (x_range[1] - x_range[0]) * plot[0]

    def to_y(value: float) -> float:
        share = (math.log10(value) - y_range[0]) / (y_range[1] - y_range[0])
        return height - bottom - share * plot[1]

    def marker(shape: str, x: float, y: float, color: str) -> str:
        # Shapes tell the lines apart where colors do not
        if shape == "square":
            return f'<rect x="{x - 4:.1f}" y="{y - 4:.1f}" width="8" height="8" fill="{color}"/>'
        if shape == "triangle":
            points = f"{x:.1f},{y - 5:.1f} {x + 5:.1f},{y + 4:.1f} {x - 5:.1f},{y + 4:.1f}"
            return f'<polygon points="{points}" fill="{color}"/>'
        if shape == "diamond":
            points = f"{x:.1f},{y - 5.5:.1f} {x + 5.5:.1f},{y:.1f} {x:.1f},{y + 5.5:.1f} {x - 5.5:.1f},{y:.1f}"
            return f'<polygon points="{points}" fill="{color}"/>'
        return f'<circle cx="{x:.1f}" cy="{y:.1f}" r="4.5" fill="{color}"/>'

    styles = {
        "esmorph": ("#e53935", "circle"),
        "simplemma": ("#2a78d6", "square"),
        "spacy": ("#199e70", "triangle"),
        "stanza": ("#c98500", "diamond"),
    }
    style = 'font-family="sans-serif" font-size="13" fill="#888"'
    grid = 'stroke="#888" stroke-opacity="0.3"'
    parts = [
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width} {height}" '
        f'width="{width}" height="{height}">'
    ]
    for power in range(y_range[0], y_range[1] + 1):
        y = to_y(10**power)
        value = number(10**power if power >= 0 else 10.0**power, spanish)
        parts.append(f'<line x1="{left}" y1="{y:.1f}" x2="{width - right}" y2="{y:.1f}" {grid}/>')
        parts.append(
            f'<text x="{left - 8}" y="{y + 4:.1f}" text-anchor="end" {style}>{value}</text>'
        )
    for size in sorted(set(xs)):
        x = to_x(size)
        parts.append(f'<line x1="{x:.1f}" y1="{top}" x2="{x:.1f}" y2="{height - bottom}" {grid}/>')
        y = height - bottom + 18
        parts.append(
            f'<text x="{x:.1f}" y="{y}" text-anchor="middle" {style}>{number(size, spanish)}</text>'
        )
    tokens, seconds = ("tokens", "segundos") if spanish else ("tokens", "seconds")
    middle = (top + height - bottom) / 2
    parts.append(
        f'<text x="{left + plot[0] / 2}" y="{height - 8}" text-anchor="middle" {style}>{tokens}</text>'
    )
    parts.append(
        f'<text x="16" y="{middle}" transform="rotate(-90 16 {middle})" '
        f'text-anchor="middle" {style}>{seconds}</text>'
    )
    for index, (name, points) in enumerate(curves.items()):
        color, shape = styles[name]
        line = " ".join(f"{to_x(x):.1f},{to_y(y):.1f}" for x, y in points)
        parts.append(f'<polyline points="{line}" fill="none" stroke="{color}" stroke-width="2"/>')
        parts.extend(marker(shape, to_x(x), to_y(y), color) for x, y in points)
        y = top + 14 + index * 22
        x = width - right + 16
        parts.append(
            f'<line x1="{x}" y1="{y - 4}" x2="{x + 24}" y2="{y - 4}" stroke="{color}" stroke-width="2"/>'
        )
        parts.append(marker(shape, x + 12, y - 4, color))
        parts.append(f'<text x="{x + 32}" y="{y}" {style}>{TITLES[name]}</text>')
    parts.append("</svg>")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(parts) + "\n", encoding="utf-8")


def cell(value: float | None) -> str:
    return "-" if value is None else f"{value:.2f}"


def markdown(header: list[str], rows: list[list[str]]) -> str:
    """Markdown table"""
    lines = [header, [":--"] * len(header), *rows]
    return "\n".join("| " + " | ".join(line) + " |" for line in lines)


def evaluate(systems: list[str], data: Path) -> dict:
    """
    Accuracy, inflection and speed of the systems

    Arguments:
        systems (list[str]): Systems
        data (Path): Data directory

    Returns:
        dict: Metrics by sets, inflection, tokens per second and time on texts of different size
    """
    texts = gold(data)
    results: dict = {}
    for name in systems:
        result: dict = {}
        if name in INFLECTORS:
            result["inflection"] = inflection(name, texts)
        if name in SYSTEMS:
            prepare(name, data)
            analyze, convert = SYSTEMS[name](data)
            quality = {}
            for dataset, text in texts.items():
                words = [[token.word for token in sentence] for sentence in text]
                found = convert(words, analyze(words))
                quality[dataset] = scores(text, found, dataset in AUTOMATIC)
            # Time with loading on texts of different size and speed without loading - in a new
            # process
            curve = []
            for size in SIZES:
                run = measure(name, data, size)
                curve.append((size, run["load"] + run["first"]))
            full = measure(name, data, 0)
            speed = full["tokens"] / full["best"]
            result |= {"quality": quality, "speed": speed, "curve": curve}
            print(f"{TITLES[name]}: {speed:,.0f} tokens/s", file=sys.stderr)
        results[name] = result
    return results


def report(results: dict) -> None:
    """
    Markdown tables of the results

    Arguments:
        results (dict): Results of the systems
    """
    analyzers = [name for name in SYSTEMS if "quality" in results.get(name, {})]
    datasets = [dataset for dataset in DATASETS if dataset not in AUTOMATIC]
    print("## Lemma of the first analysis\n")
    rows = [
        [TITLES[name]] + [cell(results[name]["quality"][dataset]["lemma"]) for dataset in datasets]
        for name in analyzers
    ]
    print(markdown(["System", *datasets], rows))
    keys = [
        ("pos", "part of speech"),
        ("lemma", "lemma"),
        ("pos+lemma", "part of speech and lemma"),
        ("feats", "features"),
        ("all", "all together"),
        ("oracle", "oracle"),
    ]
    print("\n## In detail")
    for dataset in DATASETS:
        rows = [
            [TITLES[name]] + [cell(results[name]["quality"][dataset].get(key)) for key, _ in keys]
            for name in analyzers
        ]
        print(f"\n### {dataset}\n")
        print(markdown(["System"] + [title for _, title in keys], rows))
    conjugators = [name for name in CONJUGATORS if "inflection" in results.get(name, {})]
    if conjugators:
        rows = [
            [TITLES[name]] + [cell(results[name]["inflection"][dataset]) for dataset in datasets]
            for name in conjugators
        ]
        print("\n## Inflection of verbs\n")
        print(markdown(["System", *datasets], rows))
    print("\n## Speed\n")
    rows = [
        [TITLES[name], f"{results[name]['speed']:,.0f}"]
        + [f"{seconds:.3g}" for _, seconds in results[name]["curve"]]
        for name in analyzers
    ]
    print(markdown(["System", "tokens/s"] + [f"{size:,}" for size in SIZES], rows))


def main() -> None:
    names = list(SYSTEMS) + [name for name in INFLECTORS if name not in SYSTEMS]
    parser = argparse.ArgumentParser(description=__doc__.strip().splitlines()[0])
    parser.add_argument("data", type=Path, help="data directory")
    parser.add_argument("--systems", nargs="+", default=names, choices=names)
    parser.add_argument(
        "--chart",
        type=Path,
        help="SVG file of the chart; next to it, the Spanish one with .es.svg",
    )
    parser.add_argument(
        "--report",
        action="store_true",
        help="tables and chart from results.json without measuring",
    )
    parser.add_argument("--speed-run", nargs=2, metavar=("SYSTEM", "SIZE"), help=argparse.SUPPRESS)
    args = parser.parse_args()
    data: Path = args.data
    if args.speed_run:
        speed_run(args.speed_run[0], data, int(args.speed_run[1]))
        return
    saved = data / "results.json"
    if args.report:
        results = json.loads(saved.read_text(encoding="utf-8"))
    else:
        # A run of some systems updates only their results
        results = json.loads(saved.read_text(encoding="utf-8")) if saved.exists() else {}
        results |= evaluate(args.systems, data)
        # Versions of Python and the packages - for the description of the measurement
        packages = ["esmorph", "simplemma", "spacy", "es-core-news-sm", "stanza", "torch"]
        packages += ["ablaut", "verbecc"]
        environment = {"python": sys.version.split()[0]}
        environment |= {package: metadata.version(package) for package in packages}
        (data / "environment.json").write_text(json.dumps(environment, indent=2), encoding="utf-8")
        saved.write_text(json.dumps(results, ensure_ascii=False, indent=2), encoding="utf-8")
    report(results)
    if args.chart:
        curves = {name: result["curve"] for name, result in results.items() if "curve" in result}
        chart(curves, args.chart)
        chart(curves, args.chart.with_suffix(".es.svg"), spanish=True)


if __name__ == "__main__":
    main()
