# /// script
# requires-python = ">=3.11,<3.14"
# dependencies = [
#     "numpy==2.4.6",
#     "torch==2.15.0",
# ]
# ///
"""
Training of the context model: the choice of the analysis of a token by its sentence

Description:
    Downloads the training and development parts of UD_Spanish-AnCora (CC BY 4.0) and
    UD_Spanish-GSD (CC BY-SA 4.0) of a pinned UD release with their checksums checked, analyzes
    their tokens with the dictionary `--dictionary` and trains a model to choose among the analyses
    of a token the one of its gold word: in AnCora by the part of speech, the lemma and all
    features or, if no analysis has all of them, the core features, and in GSD by the part of
    speech alone, whose lemmas and features are automatic. The
    parts of speech of GSD are first brought to the conventions of AnCora and the dictionary by the
    gold syntax (`gsd_pos`), and the forms the two treebanks tag differently are left out of its
    loss. The unit is a surface token: a contraction or a verb with clitics is one token, and its
    gold word is the first one. The model is a BiLSTM over the features of a token (shape, word,
    endings, tags of its analyses) that scores every analysis by a dot product with the vector of
    the token. The epoch is chosen by "all" on the development part of AnCora, scored by gold words
    as in compare.py. The weights are quantized to int8 by rows and written to `--output` (`write`
    describes the format); the accuracy of the quantized model is printed. Test parts are not
    read. On the CPU a seed gives the same file. The versions of the packages are pinned in
    train_context.py.lock; esMorph is the build of the repository (`--with-editable .`)

    A dictionary with new parts of speech or features needs a new model: the dictionary to train
    with is built without the model (`esmorph-build` with three arguments), and then the
    dictionary is built with the new model

Example:
    uv run --with-editable . scripts/train_context.py --data target/context --output data/es-context.bin
"""

import argparse
import json
import math
import struct
import time
import unicodedata
from collections import Counter, defaultdict
from dataclasses import dataclass
from itertools import chain
from pathlib import Path

import numpy as np
import torch
from torch import nn

from conventions import parse_features
from export_ud import LICENSES, RELEASE, SOURCES as TREEBANKS
from export_wiktionary import download

try:
    import esmorph
except ImportError:
    raise SystemExit(
        "esMorph of the repository is needed: uv run --with-editable . scripts/train_context.py"
    ) from None

# Development parts of the release: the commits and the training parts are those of export_ud.py
DEV_FILES = {
    "UD_Spanish-AnCora": (
        "es_ancora-ud-dev.conllu",
        "1a132068062edf2654fb45b41cd4c59ba7e5c18e7c5f642ed5a9cc351fd9f0fe",
    ),
    "UD_Spanish-GSD": (
        "es_gsd-ud-dev.conllu",
        "704fb19afc0a34cff476e1a70351b7026c4364f862e8e8388b02873852567c22",
    ),
}
CORE = ("Number", "Gender", "Person", "Tense", "Mood", "VerbForm")
NO_FEATURES = {"PROPN", "NUM"}
SOURCE = {"dictionary": 0, "rule": 1, "guess": 2, "clitics": 3}
# The model sees the first KMAX analyses of a token; W= tags count up to MAX_WORDS words
KMAX = 32
SUFFIX_MAX = 4
MAX_WORDS = 4
SHAPES = 64
# Bits of an analysis against the gold word: FEATS - the core features, FULL - the part of speech,
# the lemma and all features; MAPPED - the part of speech in AnCora conventions
UPOS, LEMMA, FEATS, MAPPED, FULL = 1, 2, 4, 8, 16
ALL = UPOS | LEMMA | FEATS
METRICS = {
    "pos": UPOS,
    "lemma": LEMMA,
    "pos+lemma": UPOS | LEMMA,
    "feats": FEATS,
    "all": ALL,
    "full": FULL,
    "pos_ancora": MAPPED,
}
# Matrices kept in int8 by rows (with the LSTM weights); the rest is f32
INT8 = {"word", "suffix", "tag_in", "tag_out", "input.weight", "query.weight"}
MAGIC = b"ESMCTX\0\0"
FORMAT = 1

# Analysis for the model: part of speech, lemma, features, number of words, source
Candidate = tuple[str, str, dict[str, str], int, str]


@dataclass
class Word:
    form: str
    lemma: str
    upos: str
    feats: dict[str, str]


@dataclass
class Token:
    """Surface token: its words and the gold syntax of the first of them (the host)"""

    text: str
    words: list[Word]
    deprel: str
    head_deprel: str
    # Lemmas of the aux and cop dependents of the host
    function: set[str]


def sentences(path: Path) -> list[list[Token]]:
    """
    Sentences of a CoNLL-U file as surface tokens; empty nodes are left out

    Arguments:
        path (Path): File in CoNLL-U

    Returns:
        list[list[Token]]: Sentences
    """
    result, rows = [], []
    with path.open(encoding="utf-8") as lines:
        for line in chain(lines, ["\n"]):
            if not line.strip():
                if rows:
                    result.append(tokens(rows))
                rows = []
            elif line[0].isdigit():
                rows.append(line.rstrip("\n").split("\t"))
    return result


def tokens(rows: list[list[str]]) -> list[Token]:
    """
    Surface tokens of a sentence: a multiword token is one token with its words

    Arguments:
        rows (list[list[str]]): Rows of the sentence in CoNLL-U

    Returns:
        list[Token]: Tokens
    """
    words = {row[0]: row for row in rows if row[0].isdigit()}
    function: dict[str, set[str]] = defaultdict(set)
    for row in words.values():
        if row[7] in ("aux", "aux:pass", "cop"):
            function[row[6]].add(row[2])
    result, covered = [], 0
    for row in rows:
        if "-" in row[0]:
            first, last = map(int, row[0].split("-"))
            ids = [str(i) for i in range(first, last + 1)]
            covered = last
        elif row[0].isdigit() and int(row[0]) > covered:
            ids = [row[0]]
        else:
            continue
        host = words[ids[0]]
        head = words.get(host[6])
        members = [Word(w[1], w[2], w[3], parse_features(w[5])) for w in map(words.get, ids) if w]
        result.append(
            Token(row[1], members, host[7], head[7] if head else "root", function[host[0]])
        )
    return result


def gsd_pos(token: Token, first: bool, before: str, after: str) -> str:
    """
    Part of speech of a GSD token in the conventions of AnCora and the dictionary, by the form, the
    lemma and the gold syntax of its host:

    1. `ser` is AUX, not VERB;
    2. a relative `que` (mark of acl:relcl) is PRON;
    3. a conjunction `que` is SCONJ, not CCONJ;
    4. `como` is SCONJ, not ADP or CCONJ;
    5. a participle VERB without `haber` or `ser` that modifies a noun (acl, amod) or goes with
       `estar` is ADJ;
    6. a NOUN with a capital letter and the rest in lower case, not first in the sentence, is
       PROPN;
    7. `ambos` is NUM, `pesar` and `partir` of "a ... de" are NOUN, `debido` before `a` is ADJ,
       `cómo` is PRON

    Arguments:
        token (Token): Token
        first (bool): The token is the first in its sentence
        before (str): The previous token in lower case, "" for none
        after (str): The next token in lower case, "" for none

    Returns:
        str: Part of speech
    """
    host = token.words[0]
    text, upos, low = token.text, host.upos, token.text.lower()
    if host.lemma == "ser" and upos == "VERB":
        return "AUX"
    relative = token.deprel == "mark" and token.head_deprel == "acl:relcl"
    if low == "que" and upos in ("SCONJ", "CCONJ") and relative:
        return "PRON"
    if low == "que" and upos == "CCONJ":
        return "SCONJ"
    if low == "como" and upos in ("ADP", "CCONJ"):
        return "SCONJ"
    if upos == "VERB" and host.feats.get("VerbForm") == "Part":
        modifier = token.deprel.split(":")[0] in ("acl", "amod") or "estar" in token.function
        if modifier and not token.function & {"haber", "ser"}:
            return "ADJ"
    if upos == "NOUN" and not first and text[:1].isupper() and text[1:].islower():
        return "PROPN"
    if low == "ambos":
        return "NUM"
    if low in ("pesar", "partir") and before == "a" and after in ("de", "del"):
        return "NOUN"
    if low == "debido" and after in ("a", "al"):
        return "ADJ"
    if low == "cómo":
        return "PRON"
    return upos


def gsd_parts(sents: list[list[Token]]) -> list[str]:
    """Parts of speech of all tokens of GSD sentences in the conventions of AnCora (`gsd_pos`)"""
    result = []
    for sentence in sents:
        lower = ["", *(token.text.lower() for token in sentence), ""]
        for k, token in enumerate(sentence):
            result.append(gsd_pos(token, k == 0, lower[k], lower[k + 2]))
    return result


def divergent(ancora: list[list[Token]], gsd: list[list[Token]], gsd_upos: list[str]) -> set[str]:
    """
    Forms in lower case whose parts of speech the training parts of AnCora and GSD (in the
    conventions of AnCora) share differently: the total variation distance is at least 0.3, and
    both have the form at least 20 times; forms with digits are left out

    Arguments:
        ancora (list[list[Token]]): Sentences of AnCora
        gsd (list[list[Token]]): Sentences of GSD
        gsd_upos (list[str]): Parts of speech of the tokens of GSD (`gsd_parts`)

    Returns:
        set[str]: Forms (`normal`)
    """
    counts = []
    for sents, upos in ((ancora, None), (gsd, gsd_upos)):
        texts = [token for sentence in sents for token in sentence]
        parts = upos or [token.words[0].upos for token in texts]
        count: dict[str, Counter[str]] = defaultdict(Counter)
        for token, pos in zip(texts, parts, strict=True):
            if token.text == token.text.lower():
                count[normal(token.text)][pos] += 1
        counts.append(count)
    first, second = counts
    result = set()
    for form in first.keys() & second.keys():
        n1, n2 = first[form].total(), second[form].total()
        if n1 < 20 or n2 < 20 or any(ch.isdigit() for ch in form):
            continue
        tags = sorted(first[form].keys() | second[form].keys())
        if sum(abs(first[form][u] / n1 - second[form][u] / n2) for u in tags) / 2 >= 0.3:
            result.add(form)
    return result


def normal(text: str) -> str:
    """Word for the tables of words and endings: NFC, lower case"""
    return unicodedata.normalize("NFC", text).lower()


def suffixes(word: str) -> list[str]:
    """Keys of the endings of a word (`normal`) of 1-4 characters after a start mark "^": the
    length and the ending"""
    marked = "^" + word
    return [f"{n}{marked[-n:]}" for n in range(1, SUFFIX_MAX + 1)]


def shape(token: str, first: bool) -> int:
    """
    Shape of a token: kind (no letters or digits, all ASCII digits, some digits, lower case, upper
    case, a capital letter, mixed) x first in the sentence x hyphen x dot

    Arguments:
        token (str): Token
        first (bool): The token is the first in its sentence

    Returns:
        int: Number of the shape, 0-63
    """
    digits = sum("0" <= ch <= "9" for ch in token)
    letters = [ch for ch in token if ch.isalpha()]
    if not digits and not letters:
        kind = 0
    elif digits == len(token):
        kind = 1
    elif digits:
        kind = 2
    elif not any(ch.isupper() for ch in letters):
        kind = 3
    elif not any(ch.islower() for ch in letters) and len(letters) > 1:
        kind = 4
    elif letters[0].isupper() and not any(ch.isupper() for ch in letters[1:]):
        kind = 5
    else:
        kind = 6
    dot = "." in token and len(token) > 1
    return kind * 8 + 4 * first + 2 * ("-" in token) + dot


def tag_names(pos: str, feats: dict[str, str], words: int) -> tuple[list[str], str]:
    """
    Tags of an analysis for the model: the part of speech, every feature, the number of words of a
    token of several, and the full tag

    Arguments:
        pos (str): UD part of speech
        feats (dict[str, str]): UD features
        words (int): Number of the words of the token

    Returns:
        tuple[list[str], str]: Tags of the part of speech, features and words; the full tag
    """
    pairs = [f"{name}={value}" for name, value in sorted(feats.items())]
    n = min(max(words, 1), MAX_WORDS)
    names = [f"P={pos}", *(f"F={pair}" for pair in pairs), *([f"W={n}"] if n > 1 else [])]
    return names, f"C={pos}|{'|'.join(pairs)}|{n}"


def match(pos: str, lemma: str, feats: dict[str, str], gold: Word) -> int:
    """
    Bits of an analysis against a gold word, as compare.py scores it: part of speech, lemma (NFC,
    lower case) and core features, which gold PROPN and NUM do not compare; and whether the part
    of speech, the lemma and all features are those of the gold word

    Arguments:
        pos (str): Part of speech
        lemma (str): Lemma
        feats (dict[str, str]): Features
        gold (Word): Gold word

    Returns:
        int: Bits UPOS, LEMMA, FEATS, FULL
    """
    bits = UPOS if pos == gold.upos else 0
    bits |= LEMMA if normal(lemma) == normal(gold.lemma) else 0
    if gold.upos in NO_FEATURES or all(feats.get(k) == gold.feats.get(k) for k in CORE):
        bits |= FEATS
    if bits & (UPOS | LEMMA) == UPOS | LEMMA and feats == gold.feats:
        bits |= FULL
    return bits


class Part:
    """Tokens of a part of a treebank, their analyses and how these match the gold words"""

    def __init__(
        self,
        sents: list[list[Token]],
        morph: esmorph.Analyzer,
        gsd: bool = False,
        excluded: frozenset[str] = frozenset(),
    ) -> None:
        """
        Arguments:
            sents (list[list[Token]]): Sentences
            morph (esmorph.Analyzer): Analyzer
            gsd (bool): A part of GSD: only the part of speech is right, in the conventions of
                AnCora (`gsd_parts`)
            excluded (frozenset[str]): Forms in lower case left out of the loss (`divergent`)
        """
        texts = [token for sentence in sents for token in sentence]
        self.forms = [token.text for token in texts]
        self.sent_off = np.cumsum([0] + [len(sentence) for sentence in sents])
        upos = gsd_parts(sents) if gsd else [token.words[0].upos for token in texts]
        self.candidates: list[list[Candidate]] = [
            [(a.pos, a.lemma, a.feats, len(a.words), a.source) for a in found[:KMAX]]
            for found in morph.analyze_many(self.forms)
        ]
        bits, right, off = [], [], [0]
        # A token without analyses keeps its lowercase form as the lemma, as `lemmatize` gives it
        self.fallback = np.zeros(len(texts), dtype=np.int64)
        for i, (token, pos, found) in enumerate(zip(texts, upos, self.candidates, strict=True)):
            host = token.words[0]
            self.fallback[i] = match("", token.text.lower(), {}, host)
            skip = token.text == token.text.lower() and normal(token.text) in excluded
            found_bits = [
                match(p, lemma, feats, host) | (MAPPED if p == pos else 0)
                for p, lemma, feats, _, _ in found
            ]
            bits.extend(found_bits)
            # AnCora: all features of the gold word if an analysis has them, else the core ones
            want = MAPPED if gsd else FULL if any(b & FULL for b in found_bits) else ALL
            right.extend(b & want == want and not skip for b in found_bits)
            off.append(len(bits))
        self.bits = np.array(bits, dtype=np.int64)
        self.right = np.array(right, dtype=bool)
        self.off = np.array(off)
        self.evaluated = np.array([token.words[0].upos != "PUNCT" for token in texts])
        # Other words of multiword tokens: the first analysis of the word alone, as in compare.py
        rest = [w for token in texts for w in token.words[1:] if w.upos != "PUNCT"]
        self.extra = Counter[str]()
        for w, found in zip(rest, morph.analyze_many([w.form for w in rest]), strict=True):
            a = found[0] if found else None
            b = match(a.pos, a.lemma, a.feats, w) if a else match("", w.form.lower(), {}, w)
            b |= MAPPED if b & UPOS else 0
            self.extra.update(name for name, mask in METRICS.items() if b & mask == mask)
        self.extra_count = len(rest)

    def scores(self, chosen: np.ndarray) -> dict[str, float]:
        """
        Accuracy by gold words without punctuation, as compare.py scores it

        Arguments:
            chosen (np.ndarray): Chosen analysis of every token: its row, -1 for none

        Returns:
            dict[str, float]: Metric - percent
        """
        bits = np.where(chosen >= 0, self.bits[np.maximum(chosen, 0)], self.fallback)
        bits = bits[self.evaluated]
        total = len(bits) + self.extra_count
        return {
            name: 100 * (int(((bits & mask) == mask).sum()) + self.extra[name]) / total
            for name, mask in METRICS.items()
        }

    def first(self) -> np.ndarray:
        """Rows of the first analyses of the tokens, -1 for a token without analyses"""
        return np.where(np.diff(self.off) > 0, self.off[:-1], -1)


class Vocabulary:
    """Tables of words, endings and tags by the training data"""

    def __init__(
        self, parts: list[Part], words: int, suffix_count: int, inventory: list[str]
    ) -> None:
        """
        Arguments:
            parts (list[Part]): Training parts
            words (int): Most frequent words in the table
            suffix_count (int): Most frequent endings in the table
            inventory (list[str]): Tags of parts of speech and features of the dictionary
        """
        self.inventory = inventory
        counts = Counter(normal(form) for part in parts for form in part.forms)
        ranked = sorted(counts.items(), key=lambda item: (-item[1], item[0]))
        self.words = [word for word, _ in ranked[:words]]
        suffix_counts: Counter[str] = Counter()
        for word, count in counts.items():
            for key in suffixes(word):
                suffix_counts[key] += count
        ranked = sorted(
            (item for item in suffix_counts.items() if item[1] >= 3),
            key=lambda item: (-item[1], item[0]),
        )
        self.suffixes = [key for key, _ in ranked[:suffix_count]]
        # The full tag is a tag of its own if it is right in training at least 5 times
        full_counts: Counter[str] = Counter()
        tags = set()
        for part in parts:
            right = iter(part.right)
            for found in part.candidates:
                for pos, _, feats, n, _ in found:
                    names, full = tag_names(pos, feats, n)
                    tags.update(names)
                    full_counts[full] += int(next(right))
        self.tags = sorted(tags) + sorted(full for full, n in full_counts.items() if n >= 5)
        self.word_id = {word: i + 2 for i, word in enumerate(self.words)}
        self.suffix_id = {key: i + 2 for i, key in enumerate(self.suffixes)}
        self.tag_id = {tag: i + 1 for i, tag in enumerate(self.tags)}
        # Different tag lists of analyses: the model sums their rows once per step
        self.sets: dict[tuple, int] = {}
        self.set_tags: list[list[int]] = []

    def tag_set(self, pos: str, feats: dict[str, str], words: int) -> int:
        """Number of the list of tags of an analysis in `set_tags`; tags the training data has not
        are left out"""
        key = (pos, tuple(sorted(feats.items())), min(max(words, 1), MAX_WORDS))
        if key not in self.sets:
            names, full = tag_names(pos, feats, words)
            self.sets[key] = len(self.set_tags)
            self.set_tags.append([self.tag_id[t] for t in [*names, full] if t in self.tag_id])
        return self.sets[key]

    def set_table(self) -> torch.Tensor:
        """Lists of tags as rows padded by the empty tag 0"""
        width = max(len(tags) for tags in self.set_tags)
        return torch.tensor([tags + [0] * (width - len(tags)) for tags in self.set_tags])


def featurize(part: Part, vocab: Vocabulary) -> dict[str, np.ndarray]:
    """Features of the tokens and analyses of a part: the analyses of token `i` are rows
    `off[i]:off[i + 1]`"""
    first = np.zeros(len(part.forms), dtype=bool)
    first[part.sent_off[:-1]] = True
    words = [normal(form) for form in part.forms]
    rows = [c for found in part.candidates for c in found]
    return {
        "word": np.array([vocab.word_id.get(w, 1) for w in words]),
        "suffix": np.array([[vocab.suffix_id.get(k, 1) for k in suffixes(w)] for w in words]),
        "shape": np.array([shape(f, s) for f, s in zip(part.forms, first, strict=True)]),
        "off": part.off,
        "set": np.array([vocab.tag_set(c[0], c[2], c[3]) for c in rows], dtype=np.int64),
        "source": np.array([SOURCE[c[4]] for c in rows], dtype=np.int64),
        "right": part.right,
        "sent_off": part.sent_off,
    }


def merge(parts: list[dict[str, np.ndarray]]) -> dict[str, np.ndarray]:
    """Features of several parts as one: rows of analyses and sentences are shifted"""
    out = {
        key: np.concatenate([f[key] for f in parts])
        for key in ("word", "suffix", "shape", "set", "source", "right")
    }
    tokens = np.cumsum([0] + [len(f["word"]) for f in parts])
    rows = np.cumsum([0] + [len(f["set"]) for f in parts])
    out["off"] = np.concatenate(
        [f["off"][:-1] + rows[i] for i, f in enumerate(parts)] + [rows[-1:]]
    )
    out["sent_off"] = np.concatenate(
        [f["sent_off"][:-1] + tokens[i] for i, f in enumerate(parts)] + [tokens[-1:]]
    )
    return out


class Model(nn.Module):
    """Scores of the analyses of the tokens of sentences"""

    def __init__(self, vocab: Vocabulary, args: argparse.Namespace) -> None:
        super().__init__()
        d, dc = args.d, args.dc
        self.register_buffer("sets", vocab.set_table(), persistent=False)
        self.word = nn.Embedding(len(vocab.words) + 2, args.dw, padding_idx=0)
        self.suffix = nn.Embedding(len(vocab.suffixes) + 2, args.ds, padding_idx=0)
        self.shape = nn.Embedding(SHAPES, args.dshape)
        self.tag_in = nn.Embedding(len(vocab.tags) + 1, dc, padding_idx=0)
        self.tag_out = nn.Embedding(len(vocab.tags) + 1, dc, padding_idx=0)
        self.tag_bias = nn.Embedding(len(vocab.tags) + 1, 1, padding_idx=0)
        self.source = nn.Embedding(len(SOURCE), dc)
        self.input = nn.Linear(args.dshape + dc + args.dw + SUFFIX_MAX * args.ds, d)
        self.drop = nn.Dropout(args.dropout)
        self.lstm = nn.LSTM(
            d,
            d // 2,
            num_layers=args.layers,
            bidirectional=True,
            batch_first=True,
            dropout=args.dropout if args.layers > 1 else 0,
        )
        self.norm = nn.LayerNorm(d)
        self.query = nn.Linear(d, dc)

    def forward(self, x: dict[str, torch.Tensor]) -> torch.Tensor:
        cmask = x["cmask"]
        tags_in = self.tag_in(self.sets).sum(1)[x["set"]]
        weight = cmask.float()[..., None]
        mean = (tags_in * weight).sum(2) / weight.sum(2).clamp(min=1)
        parts = [
            self.shape(x["shape"]),
            mean,
            self.word(x["word"]),
            self.suffix(x["suffix"]).flatten(2),
        ]
        h = self.drop(torch.relu(self.input(torch.cat(parts, -1))))
        h = h * x["mask"].float()[..., None]
        lengths = x["mask"].sum(1).cpu()
        packed = nn.utils.rnn.pack_padded_sequence(
            h, lengths, batch_first=True, enforce_sorted=False
        )
        out, _ = self.lstm(packed)
        h, _ = nn.utils.rnn.pad_packed_sequence(out, batch_first=True, total_length=h.shape[1])
        q = self.query(self.norm(self.drop(h)))
        c = self.tag_out(self.sets).sum(1)[x["set"]] + self.source(x["source"])
        bias = self.tag_bias(self.sets).sum(1)[x["set"]][..., 0]
        scores = (q[:, :, None, :] * c).sum(-1) + bias
        return scores.masked_fill(~cmask, -1e9)


def batches(
    f: dict[str, np.ndarray], budget: int, rng: np.random.Generator | None
) -> list[np.ndarray]:
    """Batches of sentences of close length, no more than `budget` tokens with the padding;
    `rng` gives a random order for training, None - the order by length"""
    lengths = np.diff(f["sent_off"])
    order = (
        np.lexsort((rng.random(len(lengths)), lengths // 4))
        if rng
        else np.argsort(lengths, kind="stable")
    )
    out, current, longest = [], [], 0
    for s in order:
        longest = max(longest, lengths[s])
        if current and longest * (len(current) + 1) > budget:
            out.append(np.array(current))
            current, longest = [], lengths[s]
        current.append(s)
    if current:
        out.append(np.array(current))
    if rng:
        rng.shuffle(out)
    return out


def batch(
    f: dict[str, np.ndarray],
    sents: np.ndarray,
    device: torch.device,
    word_drop: float = 0.0,
    rng: np.random.Generator | None = None,
) -> tuple[dict[str, torch.Tensor], np.ndarray, np.ndarray]:
    """
    Tensors of a batch: tokens [B, L], analyses [B, L, K]; `word_drop` of the words are replaced
    by the unknown word (training). Also the numbers of the tokens [B, L] (-1 for padding) and the
    rows of the analyses [B, L, K] (-1 for none)
    """
    so = f["sent_off"]
    lengths = so[sents + 1] - so[sents]
    tok = np.full((len(sents), lengths.max()), -1)
    for b, (s, n) in enumerate(zip(sents, lengths, strict=True)):
        tok[b, :n] = np.arange(so[s], so[s] + n)
    mask = tok >= 0
    t = tok.clip(min=0)
    off = f["off"]
    count = np.where(mask, off[t + 1] - off[t], 0)
    cmask = np.arange(max(count.max(), 1)) < count[..., None]
    rows = np.where(cmask, off[t][..., None] + np.arange(cmask.shape[-1]), -1)
    c = rows.clip(min=0)
    word = f["word"][t]
    if word_drop and rng:
        word = np.where(rng.random(word.shape) < word_drop, 1, word)
    arrays = {
        "mask": mask,
        "word": word,
        "suffix": f["suffix"][t],
        "shape": f["shape"][t],
        "cmask": cmask,
        "set": f["set"][c],
        "source": f["source"][c],
        "right": f["right"][c] & cmask,
    }
    tensors = {
        key: torch.from_numpy(np.ascontiguousarray(value)).to(device)
        for key, value in arrays.items()
    }
    return tensors, tok, rows


@torch.no_grad()
def predict(model: Model, f: dict[str, np.ndarray], device: torch.device) -> np.ndarray:
    """Chosen analysis of every token: its row in the features, -1 for a token without analyses"""
    model.eval()
    chosen = np.full(len(f["word"]), -1)
    for sents in batches(f, 4000, None):
        x, tok, rows = batch(f, sents, device)
        best = model(x).argmax(-1).cpu().numpy()
        valid = tok >= 0
        chosen[tok[valid]] = np.take_along_axis(rows, best[..., None], -1)[..., 0][valid]
    return chosen


def evaluate(
    model: Model | None, dev: dict[str, tuple[Part, dict]], device: torch.device
) -> dict[str, dict[str, float]]:
    """Accuracy of the model (None - of the first analyses) on the development parts: all metrics
    on AnCora, the part of speech on GSD (in its conventions and in those of AnCora)"""
    result = {}
    for name, (part, f) in dev.items():
        scores = part.scores(predict(model, f, device) if model is not None else part.first())
        keep = ("pos", "pos_ancora") if name == "GSD" else tuple(METRICS)[:6]
        result[name] = {key: round(scores[key], 2) for key in keep}
    return result


def train(
    model: Model,
    f: dict[str, np.ndarray],
    dev: dict[str, tuple[Part, dict]],
    args: argparse.Namespace,
    device: torch.device,
) -> tuple[int, dict[str, dict[str, float]]]:
    """
    Training with the epoch chosen by "all" on the development part of AnCora; the model keeps the
    weights of that epoch

    Arguments:
        model (Model): Model
        f (dict[str, np.ndarray]): Features of the training data
        dev (dict[str, tuple[Part, dict]]): Development parts: name - tokens and features
        args (argparse.Namespace): Parameters
        device (torch.device): Device

    Returns:
        tuple[int, dict[str, dict[str, float]]]: The epoch and its accuracy
    """
    rng = np.random.default_rng(args.seed)
    opt = torch.optim.AdamW(model.parameters(), lr=args.lr, weight_decay=1e-5)
    total = max(1, args.epochs * len(batches(f, args.batch_tokens, rng)))
    sched = torch.optim.lr_scheduler.LambdaLR(
        opt, lambda s: min(1.0, s / 300) * 0.5 * (1 + math.cos(math.pi * min(s, total) / total))
    )
    best: tuple[float, int, dict] = (-1.0, 0, {})
    best_state = None
    for epoch in range(1, args.epochs + 1):
        model.train()
        started, loss_sum, steps = time.time(), 0.0, 0
        for sents in batches(f, args.batch_tokens, rng):
            x, *_ = batch(f, sents, device, args.word_drop, rng)
            right = x["right"]
            has = right.any(-1) & (x["cmask"].sum(-1) > 1)
            if not has.any():
                continue
            logp = model(x).log_softmax(-1)
            # Several right analyses: their probabilities add up
            loss = -(logp.masked_fill(~right, -1e9).logsumexp(-1))[has].mean()
            opt.zero_grad()
            loss.backward()
            nn.utils.clip_grad_norm_(model.parameters(), 5.0)
            opt.step()
            sched.step()
            loss_sum, steps = loss_sum + loss.item(), steps + 1
        scores = evaluate(model, dev, device)
        print(
            f"epoch {epoch}: loss {loss_sum / max(steps, 1):.4f}, {time.time() - started:.0f} s, "
            + report(scores),
            flush=True,
        )
        if scores["AnCora"]["all"] > best[0]:
            best = (scores["AnCora"]["all"], epoch, scores)
            best_state = {key: value.detach().clone() for key, value in model.state_dict().items()}
    if best_state is not None:
        model.load_state_dict(best_state)
    return best[1], best[2]


def report(scores: dict[str, dict[str, float]]) -> str:
    """Accuracy on the development parts in one line"""
    ancora, gsd = scores["AnCora"], scores["GSD"]
    return (
        f"AnCora dev all {ancora['all']:.2f}, full {ancora['full']:.2f} "
        f"(pos {ancora['pos']:.2f}), "
        f"GSD dev pos {gsd['pos']:.2f} (AnCora conventions {gsd['pos_ancora']:.2f})"
    )


def tensors(model: Model, layers: int) -> dict[str, tuple[str, np.ndarray]]:
    """
    Matrices of the weight file in the order of writing, [output, input]

    Arguments:
        model (Model): Model
        layers (int): LSTM layers

    Returns:
        dict[str, tuple[str, np.ndarray]]: Name - parameter of the model ("" for the sum of the two
        LSTM biases) and f32 matrix
    """
    state = {key: value.detach().float().cpu().numpy() for key, value in model.named_parameters()}
    tables = ("word", "suffix", "shape", "tag_in", "tag_out", "tag_bias", "source", "input")
    out = {name: (f"{name}.weight", state[f"{name}.weight"]) for name in tables}
    out["input.weight"] = out.pop("input")
    out["input.bias"] = ("input.bias", state["input.bias"][None])
    for layer in range(layers):
        for direction, suffix in (("forward", ""), ("backward", "_reverse")):
            prefix, tail = f"lstm.{layer}.{direction}", f"_l{layer}{suffix}"
            out[f"{prefix}.ih"] = (f"lstm.weight_ih{tail}", state[f"lstm.weight_ih{tail}"])
            out[f"{prefix}.hh"] = (f"lstm.weight_hh{tail}", state[f"lstm.weight_hh{tail}"])
            bias = state[f"lstm.bias_ih{tail}"] + state[f"lstm.bias_hh{tail}"]
            out[f"{prefix}.bias"] = ("", bias[None])
    for key in ("norm.weight", "norm.bias", "query.weight", "query.bias"):
        out[key] = (key, state[key] if state[key].ndim == 2 else state[key][None])
    return out


def int8(name: str) -> bool:
    """Whether the matrix of the weight file is kept in int8"""
    return name in INT8 or name.endswith((".ih", ".hh"))


def quantize(weight: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    """Symmetric int8 quantization by rows: scale = max |row| / 127, a zero row has scale 0;
    scales of the rows (f32) and int8 values"""
    scale = (np.abs(weight).max(1) / 127).astype(np.float32)
    divisor = np.where(scale > 0, scale, 1)[:, None]
    return scale, np.clip(np.round(weight / divisor), -127, 127).astype(np.int8)


def quantize_model(model: Model, weights: dict[str, tuple[str, np.ndarray]]) -> dict:
    """
    Quantization of the int8 matrices of the weight file; the model gets their values, so its
    accuracy is that of the file

    Arguments:
        model (Model): Model
        weights (dict[str, tuple[str, np.ndarray]]): Matrices (`tensors`)

    Returns:
        dict: Name - f32 matrix or (scales, int8 values)
    """
    out: dict = {}
    with torch.no_grad():
        for name, (key, array) in weights.items():
            if not int8(name):
                out[name] = array
                continue
            scale, q = quantize(array)
            out[name] = (scale, q)
            restored = torch.from_numpy(q.astype(np.float32) * scale[:, None])
            model.get_parameter(key).copy_(restored)
    return out


def write(path: Path, vocab: Vocabulary, weights: dict, config: list[int], sources: dict) -> None:
    """
    Weight file: signature, format, number of entries, then entries "name, kind, data",
    numbers little-endian. A name is u16 length and UTF-8; kind 0 - strings (u32 count, each u32
    length and UTF-8), 1 - f32 matrix (u32 rows, u32 columns, values by rows), 2 - int8 matrix
    (u32 rows, u32 columns, f32 scale of every row, int8 values by rows; a value is int8 x scale of
    its row), 3 - u32 numbers (u32 count, values)

    Arguments:
        path (Path): File
        vocab (Vocabulary): Tables of strings
        weights (dict): Matrices (`quantize_model`)
        config (list[int]): Sizes of the model
        sources (dict): Sources of training, kept as a JSON string
    """

    def entry(name: str, kind: int, body: bytes) -> bytes:
        data = name.encode()
        return struct.pack("<H", len(data)) + data + struct.pack("<B", kind) + body

    def strings(items: list[str]) -> bytes:
        encoded = [item.encode() for item in items]
        return struct.pack("<I", len(encoded)) + b"".join(
            struct.pack("<I", len(item)) + item for item in encoded
        )

    entries = [entry("config", 3, struct.pack(f"<I{len(config)}I", len(config), *config))]
    for name, items in (
        ("sources", [json.dumps(sources, ensure_ascii=False, sort_keys=True)]),
        ("words", vocab.words),
        ("suffixes", vocab.suffixes),
        ("tags", vocab.tags),
        ("inventory", vocab.inventory),
    ):
        entries.append(entry(name, 0, strings(items)))
    for name, value in weights.items():
        if isinstance(value, tuple):
            scale, q = value
            size = struct.pack("<II", *q.shape)
            entries.append(entry(name, 2, size + scale.astype("<f4").tobytes() + q.tobytes()))
        else:
            size = struct.pack("<II", *value.shape)
            entries.append(entry(name, 1, size + value.astype("<f4").tobytes()))
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(MAGIC + struct.pack("<II", FORMAT, len(entries)) + b"".join(entries))


def fetch(data: Path) -> dict[str, tuple[Path, str]]:
    """Training and development parts of the treebanks, downloaded into the data directory: file
    name - path and SHA-256"""
    result = {}
    for treebank, (commit, train_file, train_digest) in TREEBANKS.items():
        base = f"https://raw.githubusercontent.com/UniversalDependencies/{treebank}/{commit}"
        for name, digest in ((train_file, train_digest), DEV_FILES[treebank]):
            result[name] = (download(f"{base}/{name}", data / name, digest), digest)
    return result


def main() -> None:
    parser = argparse.ArgumentParser(description=(__doc__ or "").strip().splitlines()[0])
    parser.add_argument(
        "--dictionary",
        type=Path,
        default=Path("python/esmorph/data/es.dict"),
        help="dictionary whose analyses the model learns to choose",
    )
    parser.add_argument(
        "--data", type=Path, default=Path("target/context"), help="directory of the UD data"
    )
    parser.add_argument(
        "--output", type=Path, default=Path("data/es-context.bin"), help="weight file"
    )
    parser.add_argument("--d", type=int, default=128, help="size of the token vector")
    parser.add_argument("--layers", type=int, default=1, help="BiLSTM layers")
    parser.add_argument("--dw", type=int, default=32, help="size of a word vector")
    parser.add_argument("--ds", type=int, default=16, help="size of an ending vector")
    parser.add_argument("--dc", type=int, default=32, help="size of a tag vector")
    parser.add_argument("--dshape", type=int, default=16, help="size of a shape vector")
    parser.add_argument("--words", type=int, default=8000, help="words in the table")
    parser.add_argument("--suffixes", type=int, default=4000, help="endings in the table")
    parser.add_argument("--dropout", type=float, default=0.2)
    parser.add_argument("--word-drop", type=float, default=0.1, help="share of words hidden")
    parser.add_argument("--epochs", type=int, default=12)
    parser.add_argument("--lr", type=float, default=2e-3)
    parser.add_argument("--batch-tokens", type=int, default=4000)
    parser.add_argument("--seed", type=int, default=1)
    parser.add_argument(
        "--device", default="cpu", help="torch device; only the CPU gives the same file each run"
    )
    args = parser.parse_args()
    torch.manual_seed(args.seed)
    device = torch.device(args.device)
    # Gradients summed by index (index_put_) are in a fixed order only so: a seed gives one file
    if device.type == "cpu":
        torch.use_deterministic_algorithms(True)
    print(f"esMorph {esmorph.__version__}, torch {torch.__version__}, {device}", flush=True)

    started = time.time()
    files = fetch(args.data)
    morph = esmorph.Analyzer(args.dictionary)
    read = {name: sentences(path) for name, (path, _) in files.items()}
    ancora, gsd = read["es_ancora-ud-train.conllu"], read["es_gsd-ud-train.conllu"]
    excluded = frozenset(divergent(ancora, gsd, gsd_parts(gsd)))
    train_parts = [Part(ancora, morph), Part(gsd, morph, gsd=True, excluded=excluded)]
    dev_parts = {
        "AnCora": Part(read["es_ancora-ud-dev.conllu"], morph),
        "GSD": Part(read["es_gsd-ud-dev.conllu"], morph, gsd=True),
    }
    # Every tag of the dictionary and its rules: the builder refuses a dictionary with others
    ud_tags = morph._ud_tags()
    inventory = sorted(
        {f"P={pos}" for pos, _ in ud_tags}
        | {f"F={name}={value}" for _, feats in ud_tags for name, value in feats}
    )
    vocab = Vocabulary(train_parts, args.words, args.suffixes, inventory)
    f = merge([featurize(part, vocab) for part in train_parts])
    dev = {name: (part, featurize(part, vocab)) for name, part in dev_parts.items()}
    print(
        f"data: {len(f['word'])} training tokens, {len(excluded)} divergent forms of GSD, "
        f"{len(vocab.words)} words, {len(vocab.suffixes)} endings, {len(vocab.tags)} tags, "
        f"{len(vocab.inventory)} tags of the dictionary, {time.time() - started:.0f} s",
        flush=True,
    )

    first_scores = evaluate(None, dev, device)
    print("first analysis: " + report(first_scores), flush=True)
    started = time.time()
    model = Model(vocab, args).to(device)
    epoch, float_scores = train(model, f, dev, args, device)
    print(f"training: {time.time() - started:.0f} s, epoch {epoch}: " + report(float_scores))
    weights = quantize_model(model, tensors(model, args.layers))
    int8_scores = evaluate(model, dev, device)
    print("int8: " + report(int8_scores))
    sources = {
        "release": RELEASE,
        "treebanks": LICENSES,
        "files": {name: digest for name, (_, digest) in files.items()},
        "esmorph": esmorph.__version__,
        "seed": args.seed,
        "epochs": args.epochs,
        "epoch": epoch,
        "dev": {"first": first_scores, "float": float_scores, "int8": int8_scores},
    }
    config = [args.d, args.layers, args.dw, args.ds, args.dc, args.dshape, KMAX, SUFFIX_MAX]
    write(args.output, vocab, weights, config, sources)
    print(f"{args.output}: {args.output.stat().st_size / 1e6:.2f} MB")


if __name__ == "__main__":
    main()
