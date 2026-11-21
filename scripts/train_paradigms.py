# /// script
# requires-python = ">=3.11,<3.14"
# dependencies = [
#     "numpy==2.4.6",
#     "torch==2.15.0",
# ]
#
# # The model is trained on the CPU: on Linux torch without CUDA
# [[tool.uv.index]]
# name = "pytorch-cpu"
# url = "https://download.pytorch.org/whl/cpu"
# explicit = true
#
# [tool.uv.sources]
# torch = [{ index = "pytorch-cpu", marker = "sys_platform == 'linux'" }]
# ///
"""
Training of the paradigm model: the choice of the form of an inflection table for an unknown word

Description:
    The model chooses the analyses of an unknown lowercase word and learns from the dictionary
    itself. For every lowercase word form of the export (`lexemes.tsv`) the analyzer gives the
    candidates of guessing - the forms of tables from the lists of the endings of the word as if
    the dictionary had not the word - and marks the right ones. The model is an LSTM over the
    letters of the word from its end and scores a candidate by a dot product with a vector of the
    features of its UD tag, the suffixes of its lemma and its form and the length of its ending,
    plus the number of word forms of its list. The words of lexemes whose lemma has the hash 0
    modulo 50 are the development ones; the epoch is chosen by the share of right first candidates
    on them. The weights are quantized to int8 by rows and written to `--output`, which
    `esmorph-build` puts into the dictionary; the accuracy of the quantized model is printed. On the
    CPU a seed gives the same file with the same number of threads (`--threads`), platform and
    build of torch. The versions of the packages are pinned in train_paradigms.py.lock; esMorph is
    the build of the repository (`--with-editable .`), whose candidates must be those of the
    dictionary

Example:
    uv run --with-editable . scripts/train_paradigms.py --dictionary python/esmorph/data/es.dict \\
        --export target/wiktionary --output data/es-paradigms.bin
"""

import argparse
import hashlib
import json
import math
import struct
import time
from array import array
from pathlib import Path

import numpy as np
import torch
from torch import nn

try:
    import esmorph
except ImportError:
    raise SystemExit(
        "the build of esMorph of the repository is needed: "
        "uv run --with-editable . scripts/train_paradigms.py ..."
    ) from None

MAGIC = b"ESMPAR\0\0"
FORMAT = 1
# The model reads the last MAX_CHARS letters of a word; words of MIN_LENGTH to MAX_LENGTH letters
# are guessed
MAX_CHARS = 16
MIN_LENGTH = 3
MAX_LENGTH = 64
MAX_ENDING = 5
NUMERIC = 3


def chars_of(word: str) -> str:
    """The letters of the input of the model: the last MAX_CHARS from the end of the word"""
    return word[::-1][:MAX_CHARS]


def tag_keys(pos: str, feats: list[tuple[str, str]]) -> list[str]:
    """Keys of the features of a UD tag: the part of speech and every feature"""
    return [f"P={pos}"] + [f"F={name}={value}" for name, value in sorted(feats)]


def examples_in(f: dict[str, np.ndarray]) -> int:
    return len(f["offsets"]) - 1


def percents(scores: tuple[float, float]) -> str:
    return f"tag {scores[0] * 100:.2f}, lemma and part of speech {scores[1] * 100:.2f}"


def dev_lemma(lemma: str) -> bool:
    return hashlib.md5(lemma.encode()).digest()[0] % 50 == 0


class Data:
    """Examples: the letters of a word and its candidates in a row in arrays"""

    def __init__(self) -> None:
        self.chars = array("H")
        self.char_offsets = array("I", [0])
        self.offsets = array("I", [0])
        self.ud = array("H")
        self.lemma_suffix = array("I")
        self.form_suffix = array("I")
        self.length = array("B")
        self.count = array("f")
        self.share = array("f")
        self.gold = array("B")
        self.loose = array("B")
        # The UD tag of the first right candidate of every example
        self.tag = array("H")

    def add(
        self,
        word: str,
        candidates: list,
        pos: list[str],
        chars: dict[str, int],
        suffixes: dict[str, int],
        grow: bool,
    ) -> None:
        golds = {(lemma, pos[ud]) for lemma, ud, *_, gold in candidates if gold}
        self.tag.append(next(ud for _, ud, *_, gold in candidates if gold))
        for char in chars_of(word):
            if char not in chars and grow:
                chars[char] = len(chars) + 2
            self.chars.append(chars.get(char, 1))
        self.char_offsets.append(len(self.chars))
        for lemma, ud, lemma_suffix, form_suffix, length, count, share, gold in candidates:
            self.loose.append((lemma, pos[ud]) in golds)
            for suffix, column in (
                (lemma_suffix, self.lemma_suffix),
                (form_suffix, self.form_suffix),
            ):
                if suffix not in suffixes and grow:
                    suffixes[suffix] = len(suffixes) + 1
                column.append(suffixes.get(suffix, 0))
            self.ud.append(ud)
            self.length.append(length)
            self.count.append(count)
            self.share.append(share)
            self.gold.append(gold)
        self.offsets.append(len(self.ud))

    def numpy(self) -> dict[str, np.ndarray]:
        names = (
            "chars",
            "char_offsets",
            "offsets",
            "ud",
            "lemma_suffix",
            "form_suffix",
            "length",
            "count",
            "share",
            "gold",
            "loose",
            "tag",
        )
        f = {
            name: np.frombuffer(getattr(self, name), dtype=getattr(self, name).typecode).copy()
            for name in names
        }
        # Whether the ending of a candidate is the longest among the candidates of its example
        owner = np.repeat(np.arange(examples_in(f)), np.diff(f["offsets"]))
        top = np.zeros(examples_in(f), dtype=np.uint8)
        np.maximum.at(top, owner, f["length"])
        f["longest"] = (f["length"] == top[owner]).astype(np.float32)
        return f


def collect(
    morph: "esmorph.Analyzer", words: list[str], chars: dict, suffixes: dict
) -> tuple[Data, Data]:
    """
    The training and development examples: the development ones are the words of lexemes with a
    development lemma; a word without a right candidate is no example

    Args:
        morph (esmorph.Analyzer): Analyzer of the dictionary
        words (list[str]): Words of the dictionary
        chars (dict): Numbers of letters, filled by the training words
        suffixes (dict): Numbers of suffixes, filled by the training words

    Returns:
        tuple[Data, Data]: The training and the development examples
    """
    train, dev = Data(), Data()
    pos = [pos for pos, _ in morph._ud_tags()]
    for word in words:
        candidates = morph._guess_examples(word)
        golds = [candidate for candidate in candidates if candidate[7]]
        if not golds:
            continue
        target = dev if dev_lemma(golds[0][0]) else train
        target.add(word, candidates, pos, chars, suffixes, target is train)
    return train, dev


class Model(nn.Module):
    """
    An LSTM over the letters of a word from its end and the scores of its candidates: a row of a
    table of features is a vector of the size dim and a bias in the last column
    """

    def __init__(self, n_chars: int, n_keys: int, n_suffixes: int, args: argparse.Namespace):
        super().__init__()
        self.char = nn.Embedding(n_chars, args.dchar, padding_idx=0)
        self.lstm = nn.LSTM(args.dchar, args.hidden, batch_first=True)
        self.hidden = nn.Linear(args.hidden, args.dim)
        width = args.dim + 1
        # Row 0 of the keys pads short tags and row 0 of the suffixes is an unknown suffix: they
        # stay zeros, as in the analyzer
        self.key = nn.Embedding(n_keys, width, padding_idx=0)
        self.lemma_suffix = nn.Embedding(n_suffixes, width, padding_idx=0)
        self.form_suffix = nn.Embedding(n_suffixes, width, padding_idx=0)
        self.length = nn.Embedding(MAX_ENDING, width)
        self.numeric = nn.Linear(NUMERIC, 1, bias=False)
        for table in (self.key, self.lemma_suffix, self.form_suffix):
            nn.init.normal_(table.weight, std=0.05)
            with torch.no_grad():
                table.weight[0].zero_()

    def forward(self, x: dict[str, torch.Tensor], tag_keys: torch.Tensor) -> torch.Tensor:
        packed = nn.utils.rnn.pack_padded_sequence(
            self.char(x["chars"]), x["char_lengths"], batch_first=True, enforce_sorted=False
        )
        _, (state, _) = self.lstm(packed)
        h = torch.tanh(self.hidden(state[0]))
        h = torch.cat([h, torch.ones(len(h), 1)], 1)[x["owner"]]
        # The keys of a tag: the rows of the table of keys by the number of the UD tag, 0 - none
        e = self.key(tag_keys[x["ud"]]).sum(1)
        e = e + self.lemma_suffix(x["lemma_suffix"]) + self.form_suffix(x["form_suffix"])
        e = e + self.length(x["length"] - 1)
        numeric = torch.stack([torch.log1p(x["count"]), x["share"], x["longest"]], 1)
        return (h * e).sum(1) + self.numeric(numeric)[:, 0]


def batch(f: dict[str, np.ndarray], examples: np.ndarray) -> dict[str, torch.Tensor]:
    """The examples `examples`: letters padded with zeros, candidates in a row and the number of
    the example of each in the batch"""
    starts, ends = f["char_offsets"][examples], f["char_offsets"][examples + 1]
    lengths = ends - starts
    chars = np.zeros((len(examples), int(lengths.max())), dtype=np.int64)
    for row, (start, end) in enumerate(zip(starts, ends, strict=True)):
        chars[row, : end - start] = f["chars"][start:end]
    first, last = f["offsets"][examples], f["offsets"][examples + 1]
    rows = np.concatenate([np.arange(a, b) for a, b in zip(first, last, strict=True)])
    owner = np.repeat(np.arange(len(examples)), last - first)
    t = torch.from_numpy
    return {
        "chars": t(chars),
        "char_lengths": t(lengths.astype(np.int64)),
        "owner": t(owner),
        "ud": t(f["ud"][rows].astype(np.int64)),
        "lemma_suffix": t(f["lemma_suffix"][rows].astype(np.int64)),
        "form_suffix": t(f["form_suffix"][rows].astype(np.int64)),
        "length": t(f["length"][rows].astype(np.int64)),
        "count": t(f["count"][rows]),
        "share": t(f["share"][rows]),
        "longest": t(f["longest"][rows]),
        "gold": t(f["gold"][rows].astype(bool)),
    }


def segment_logsumexp(scores: torch.Tensor, owner: torch.Tensor, n: int) -> torch.Tensor:
    top = torch.full((n,), -1e30).scatter_reduce(0, owner, scores, "amax")
    sums = torch.zeros(n).index_add(0, owner, torch.exp(scores - top[owner]))
    return top + torch.log(sums)


def first_right(f: dict[str, np.ndarray], scores: np.ndarray) -> tuple[float, float]:
    """
    Shares of the examples whose first candidate by the score is right, by the weights of the
    examples: the same form or the same lemma and tag, and with the same lemma and part of speech

    Args:
        f (dict[str, np.ndarray]): Examples
        scores (np.ndarray): Scores of the candidates

    Returns:
        tuple[float, float]: Shares of right ones by the tag and by the lemma and part of speech
    """
    exact = loose = 0.0
    for example in range(examples_in(f)):
        a, b = f["offsets"][example], f["offsets"][example + 1]
        best = a + int(np.argmax(scores[a:b]))
        exact += f["weight"][example] * bool(f["gold"][best])
        loose += f["weight"][example] * bool(f["loose"][best])
    total = float(f["weight"].sum())
    return float(exact) / total, float(loose) / total


def weigh(f: dict[str, np.ndarray], dev: dict[str, np.ndarray], text: np.ndarray, power: float):
    """
    The weights of the examples: how often texts have the tag of the example against its share
    among the examples of the training, to the power `power`, the mean of the training 1. With the
    power 1 the examples have the tags as texts have them, with 0 as the dictionary words

    Args:
        f (dict[str, np.ndarray]): The training examples
        dev (dict[str, np.ndarray]): The development examples
        text (np.ndarray): How often texts have every UD tag, plus one
        power (float): The power
    """
    words = np.bincount(f["tag"], minlength=len(text)).astype(np.float64) + 1
    ratio = (text / text.sum()) / (words / words.sum())
    scale = (ratio[f["tag"]] ** power).mean()
    for examples in (f, dev):
        examples["weight"] = (ratio[examples["tag"]] ** power / scale).astype(np.float32)


def predict(model: Model, f: dict[str, np.ndarray], tag_keys: torch.Tensor) -> np.ndarray:
    """Scores of all candidates"""
    model.eval()
    n = examples_in(f)
    with torch.no_grad():
        scores = [
            model(batch(f, np.arange(start, min(n, start + 4096))), tag_keys).numpy()
            for start in range(0, n, 4096)
        ]
    return np.concatenate(scores)


def baseline(f: dict[str, np.ndarray]) -> tuple[float, float]:
    """Shares of right ones by the numbers of word forms of the longest ending, as without the
    model (which also weighs a tag by how often texts have it)"""
    return first_right(f, f["longest"] * 1e9 + f["count"])


def quantize(weight: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    """
    Symmetric int8 quantization by rows

    Args:
        weight (np.ndarray): Matrix

    Returns:
        tuple[np.ndarray, np.ndarray]: The scales of the rows (f32) and the int8 numbers
    """
    scale = (np.abs(weight).max(1) / 127).astype(np.float32)
    divisor = np.where(scale > 0, scale, 1)[:, None]
    q = np.clip(np.round(weight / divisor), -127, 127).astype(np.int8)
    return scale, q


def quantized(values: np.ndarray) -> bool:
    """Matrices of 4096 numbers and more are kept in int8, the rest in f32"""
    return values.shape[0] > 1 and values.size >= 4096


def quantize_model(model: Model) -> None:
    """Replaces the weights of the model with the quantized ones, as in the file of weights"""
    with torch.no_grad():
        for parameter in model.parameters():
            values = parameter.detach().float().cpu().numpy()
            if values.ndim == 2 and quantized(values):
                scale, q = quantize(values)
                parameter.copy_(torch.from_numpy(q.astype(np.float32) * scale[:, None]))


def tensors(model: Model) -> dict[str, np.ndarray]:
    """Tensors of the file of weights: the biases of the LSTM summed, matrices as [output, input]"""
    state = {key: value.float().cpu().numpy() for key, value in model.state_dict().items()}
    out = {
        "char": state["char.weight"],
        "lstm.ih": state["lstm.weight_ih_l0"],
        "lstm.hh": state["lstm.weight_hh_l0"],
        "lstm.bias": (state["lstm.bias_ih_l0"] + state["lstm.bias_hh_l0"])[None],
        "hidden.weight": state["hidden.weight"],
        "hidden.bias": state["hidden.bias"][None],
    }
    for name in ("key", "lemma_suffix", "form_suffix", "length"):
        out[name] = state[f"{name}.weight"]
    out["numeric"] = state["numeric.weight"]
    return out


def write(
    path: Path,
    strings: dict[str, list[str]],
    weights: dict[str, np.ndarray],
    config: list[int],
    sources: dict,
) -> None:
    """
    The file of weights: the signature, the version, the number of entries, then the entries
    "name, kind, data", numbers little-endian. Kind 0 - strings, 1 - an f32 matrix, 2 - an int8
    matrix with an f32 scale for every row, 3 - u32 numbers. The entry sources - the data of the
    training as a JSON string

    Args:
        path (Path): File
        strings (dict[str, list[str]]): Tables of strings
        weights (dict[str, np.ndarray]): Tensors
        config (list[int]): Sizes of the model
        sources (dict): Data of the training
    """
    entries = []

    def name(text: str) -> bytes:
        data = text.encode()
        return struct.pack("<H", len(data)) + data

    entries.append(
        name("config")
        + struct.pack("<BI", 3, len(config))
        + struct.pack(f"<{len(config)}I", *config)
    )
    for key, items in {"sources": [json.dumps(sources, ensure_ascii=False)], **strings}.items():
        body = struct.pack("<BI", 0, len(items))
        for text in items:
            data = text.encode()
            body += struct.pack("<I", len(data)) + data
        entries.append(name(key) + body)
    for key, values in weights.items():
        rows, cols = values.shape
        if quantized(values):
            scale, q = quantize(values)
            entries.append(
                name(key)
                + struct.pack("<BII", 2, rows, cols)
                + scale.astype("<f4").tobytes()
                + q.tobytes()
            )
        else:
            entries.append(
                name(key) + struct.pack("<BII", 1, rows, cols) + values.astype("<f4").tobytes()
            )
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(MAGIC + struct.pack("<II", FORMAT, len(entries)) + b"".join(entries))


def words_of(export: Path) -> list[str]:
    """The lowercase word forms of the export that may be guessed, in order"""
    words = set()
    with (export / "lexemes.tsv").open(encoding="utf-8") as lines:
        for line in lines:
            form = line.split("\t", 3)[2]
            if MIN_LENGTH <= len(form) <= MAX_LENGTH and form.isalpha() and form.islower():
                words.add(form)
    return sorted(words)


def main() -> None:
    parser = argparse.ArgumentParser(description=(__doc__ or "").strip().splitlines()[0])
    parser.add_argument(
        "--dictionary",
        type=Path,
        default=Path("python/esmorph/data/es.dict"),
        help="dictionary whose candidates the model learns to choose",
    )
    parser.add_argument(
        "--export", type=Path, default=Path("target/wiktionary"), help="directory of the export"
    )
    parser.add_argument(
        "--output", type=Path, default=Path("data/es-paradigms.bin"), help="weight file"
    )
    parser.add_argument("--words", type=int, default=0, help="training words, 0 - all")
    parser.add_argument("--dim", type=int, default=64, help="size of the vector of a word")
    parser.add_argument("--hidden", type=int, default=32, help="size of the state of the LSTM")
    parser.add_argument("--dchar", type=int, default=32, help="size of a letter vector")
    parser.add_argument("--epochs", type=int, default=8)
    parser.add_argument(
        "--prior",
        type=float,
        default=0.5,
        help="power of the weights of the examples by how often texts have their tags",
    )
    parser.add_argument("--lr", type=float, default=2e-3)
    parser.add_argument("--batch", type=int, default=512)
    parser.add_argument("--seed", type=int, default=1)
    parser.add_argument(
        "--threads", type=int, default=4, help="threads of torch; others give another file"
    )
    args = parser.parse_args()
    torch.manual_seed(args.seed)
    torch.set_num_threads(args.threads)
    torch.use_deterministic_algorithms(True)
    rng = np.random.default_rng(args.seed)
    print(f"esMorph {esmorph.__version__}, torch {torch.__version__}", flush=True)

    started = time.time()
    morph = esmorph.Analyzer(args.dictionary)
    words = words_of(args.export)
    if args.words and args.words < len(words):
        words = [words[i] for i in sorted(rng.choice(len(words), args.words, replace=False))]
    chars: dict[str, int] = {}
    suffixes: dict[str, int] = {}
    train_data, dev_data = collect(morph, words, chars, suffixes)
    f, dev = train_data.numpy(), dev_data.numpy()
    ud_tags = morph._ud_tags()
    keys = sorted({key for pos, feats in ud_tags for key in tag_keys(pos, feats)})
    key_numbers = {key: number + 1 for number, key in enumerate(keys)}
    tags = [[key_numbers[key] for key in tag_keys(pos, feats)] for pos, feats in ud_tags]
    width = max(map(len, tags))
    tag_table = torch.tensor([row + [0] * (width - len(row)) for row in tags])
    weigh(f, dev, np.array(morph._tag_weights(), dtype=np.float64), args.prior)
    print(
        f"data: {examples_in(f)} examples ({len(f['ud'])} candidates), development "
        f"{examples_in(dev)}, {len(chars)} letters, {len(suffixes)} suffixes, {len(keys)} keys of "
        f"tags, {time.time() - started:.0f} s; without the model: development "
        + percents(baseline(dev)),
        flush=True,
    )

    model = Model(len(chars) + 2, len(keys) + 1, len(suffixes) + 1, args)
    opt = torch.optim.AdamW(model.parameters(), lr=args.lr, weight_decay=1e-5)
    n = examples_in(f)
    steps = args.epochs * math.ceil(n / args.batch)
    sched = torch.optim.lr_scheduler.LambdaLR(
        opt, lambda s: min(1.0, s / 300) * 0.5 * (1 + math.cos(math.pi * min(s, steps) / steps))
    )
    best, best_state, best_epoch, best_scores = -1.0, None, 0, (0.0, 0.0)
    for epoch in range(args.epochs):
        model.train()
        started, loss_sum, count = time.time(), 0.0, 0
        order = rng.permutation(n)
        for start in range(0, n, args.batch):
            examples = order[start : start + args.batch]
            x = batch(f, examples)
            scores = model(x, tag_table)
            every = segment_logsumexp(scores, x["owner"], len(examples))
            right = segment_logsumexp(
                scores.masked_fill(~x["gold"], -1e30), x["owner"], len(examples)
            )
            weights = torch.from_numpy(f["weight"][examples])
            loss = ((every - right) * weights).mean()
            opt.zero_grad()
            loss.backward()
            nn.utils.clip_grad_norm_(model.parameters(), 5.0)
            opt.step()
            sched.step()
            loss_sum, count = loss_sum + loss.item(), count + 1
        scores = first_right(dev, predict(model, dev, tag_table))
        print(
            f"epoch {epoch + 1}: loss {loss_sum / count:.4f}, {time.time() - started:.0f} s, "
            "development " + percents(scores),
            flush=True,
        )
        if scores[0] > best:
            best, best_epoch, best_scores = scores[0], epoch + 1, scores
            best_state = {key: value.detach().clone() for key, value in model.state_dict().items()}
    assert best_state is not None
    model.load_state_dict(best_state)
    quantize_model(model)
    int8_scores = first_right(dev, predict(model, dev, tag_table))
    print("development (int8): " + percents(int8_scores))
    info = morph.dictionary_info
    sources = {
        "snapshots": info.get("snapshots"),
        "words": len(words),
        "examples": examples_in(f),
        "seed": args.seed,
        "epochs": args.epochs,
        "epoch": best_epoch,
        "prior": args.prior,
        "dev": {"float": list(best_scores), "int8": list(int8_scores)},
        "license": "CC BY-SA 4.0",
    }

    def by_number(table: dict[str, int]) -> list[str]:
        return [item for item, _ in sorted(table.items(), key=lambda pair: pair[1])]

    write(
        args.output,
        {"chars": by_number(chars), "keys": keys, "suffixes": by_number(suffixes)},
        tensors(model),
        [args.dim, args.hidden, args.dchar, MAX_CHARS],
        sources,
    )
    print(f"{args.output}: {args.output.stat().st_size / 1e6:.2f} MB")


if __name__ == "__main__":
    main()
