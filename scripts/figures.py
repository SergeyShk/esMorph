"""
Figures of the documentation: how esMorph analyzes a sentence

Description:
    Draws by the real analyses of the dictionary and the models of the package: the pipeline
    "sentence → analyses of the dictionary → choice by the neighboring words" (docs/img/pipeline.svg),
    the context model with the sizes from its file of weights (docs/img/context.svg), the
    probabilities of the analyses of the ambiguous word "vino" without context and in two sentences
    (docs/img/choice.svg), the paradigm model with the sizes from its file of weights
    (docs/img/paradigm.svg) and the analyses of an unknown word by the frequency of its ending and
    by the model (docs/img/guess.svg); each in English and in Spanish (*.es.svg). The probabilities
    are those the package gives: `analyze`, `analyze_sentence`, and for the guess by the ending the
    lists of the dictionary as guessing without the model reads them. Runs in the environment of
    the project, with esMorph built from the repository.

Example:
    uv run python scripts/figures.py
"""

import struct
from pathlib import Path

from esmorph import Analysis, Analyzer

ROOT = Path(__file__).parents[1]
CONTEXT = ROOT / "data" / "es-context.bin"
PARADIGMS = ROOT / "data" / "es-paradigms.bin"
IMG = ROOT / "docs" / "img"
EXAMPLE = ["Ella", "vino", "ayer", "."]
OTHER = ["Bebimos", "un", "vino", "tinto", "."]
WORD = "vino"
UNKNOWN = "rechispeantes"
# Without the model the analyses come from this many first forms of the list of the longest ending
MAX_COUNTED = 10
TIMES = "\N{MULTIPLICATION SIGN}"

FONT = "Roboto, Source Sans Pro, Arial, sans-serif"
# The colors of the logo
DARK, LIGHT, SOFT = "#c62828", "#ef9a9a", "#fdecea"
INK, MUTED = "#212121", "#6b6b6b"

TEXTS = {
    "en": {
        "pos": {
            "NOUN": "noun",
            "VERB": "verb",
            "AUX": "auxiliary",
            "ADJ": "adjective",
            "ADV": "adverb",
            "PRON": "pronoun",
            "PROPN": "name",
            "DET": "determiner",
            "ADP": "preposition",
            "SCONJ": "conjunction",
            "NUM": "numeral",
            "PUNCT": "punctuation",
        },
        "feats": {
            ("Tense", "Past"): "past",
            ("Tense", "Pres"): "present",
            ("Person", "1"): "1st person",
            ("Person", "2"): "2nd person",
            ("Person", "3"): "3rd person",
            ("Gender", "Masc"): "masc.",
            ("Gender", "Fem"): "fem.",
            ("Number", "Sing"): "sing.",
            ("Number", "Plur"): "plur.",
        },
        "steps": (
            "1. Sentence",
            "2. The dictionary gives every analysis of every word and its frequency",
            "3. The context model chooses the analysis by the neighboring words",
        ),
        "context": (
            ("shape", "word", "endings 1-{suffix_max}", "analyses of the word"),
            "token vector: {din}",
            "linear layer and ReLU: {d}",
            "BiLSTM {layers} {x} {half}, both directions: {d} per token",
            "normalization and query vector: {dc}",
            "analysis score: query · (tag + source) + bias",
            "softmax over the analyses of the token: P(analysis | sentence)",
            "the first {kmax} analyses of a token · {params} million parameters, int8 by rows",
        ),
        "paradigm": (
            ("the letters of the word from the end", "{max_chars} {x} {dchar}"),
            ("candidates: forms of the lists of endings 1-5", "up to 30 forms per ending"),
            "LSTM {hidden}",
            (("UD", "tag"), ("lemma", "suffix"), ("form", "suffix"), ("ending", "length")),
            "word vector: {dim}",
            "candidate vector: {dim}",
            "score: word vector · candidate vector + count and share of the form in the list",
            "softmax over the candidates: P(form | word)",
            "the weight of a lexeme goes to its equal forms by their counts",
            "up to 10 analyses with at least 1 % of the probability of the first",
            "{params} million parameters, int8 by rows · {keys} tag features, {suffixes} suffixes",
        ),
        "guess": (
            "The unknown word «{word}»",
            "by the ending",
            "model",
            (
                "Shares of the analyses with the same lemma and part of speech: by the ending - the",
                "forms of the longest ending without the model, model - the probabilities of analyze",
            ),
        ),
        "choice": (
            "Analyses of «vino»",
            "no context",
            "Ella vino ayer.",
            "Bebimos un vino tinto.",
            "Probabilities: without context - the frequencies of the dictionary, in a sentence - "
            "the context model",
        ),
    },
    "es": {
        "pos": {
            "NOUN": "sustantivo",
            "VERB": "verbo",
            "AUX": "auxiliar",
            "ADJ": "adjetivo",
            "ADV": "adverbio",
            "PRON": "pronombre",
            "PROPN": "nombre propio",
            "DET": "determinante",
            "ADP": "preposición",
            "SCONJ": "conjunción",
            "NUM": "numeral",
            "PUNCT": "puntuación",
        },
        "feats": {
            ("Tense", "Past"): "pasado",
            ("Tense", "Pres"): "presente",
            ("Person", "1"): "1.ª persona",
            ("Person", "2"): "2.ª persona",
            ("Person", "3"): "3.ª persona",
            ("Gender", "Masc"): "masc.",
            ("Gender", "Fem"): "fem.",
            ("Number", "Sing"): "sing.",
            ("Number", "Plur"): "pl.",
        },
        "steps": (
            "1. Oración",
            "2. El diccionario da todos los análisis de cada palabra y su frecuencia",
            "3. El modelo de contexto elige el análisis por las palabras vecinas",
        ),
        "context": (
            ("aspecto", "palabra", "terminaciones 1-{suffix_max}", "análisis de la palabra"),
            "vector del token: {din}",
            "capa lineal y ReLU: {d}",
            "BiLSTM {layers} {x} {half}, en ambos sentidos: {d} por token",
            "normalización y vector de consulta: {dc}",
            "puntuación del análisis: consulta · (etiqueta + fuente) + sesgo",
            "softmax sobre los análisis del token: P(análisis | oración)",
            "los primeros {kmax} análisis de un token · {params} millones de parámetros, int8 por filas",
        ),
        "paradigm": (
            ("las letras de la palabra desde el final", "{max_chars} {x} {dchar}"),
            (
                "candidatos: formas de las listas de terminaciones 1-5",
                "hasta 30 formas por terminación",
            ),
            "LSTM {hidden}",
            (
                ("etiqueta", "UD"),
                ("sufijo", "del lema"),
                ("sufijo", "de la forma"),
                ("longitud", "de la terminación"),
            ),
            "vector de la palabra: {dim}",
            "vector del candidato: {dim}",
            "puntuación: vector de la palabra · vector del candidato + recuento y parte de la forma en la lista",
            "softmax sobre los candidatos: P(forma | palabra)",
            "el peso de un lexema se reparte entre sus formas iguales según sus recuentos",
            "hasta 10 análisis con al menos el 1 % de la probabilidad del primero",
            "{params} millones de parámetros, int8 por filas · {keys} rasgos de etiquetas, "
            "{suffixes} sufijos",
        ),
        "guess": (
            "La palabra desconocida «{word}»",
            "por la terminación",
            "modelo",
            (
                "Partes de los análisis con el mismo lema y categoría: por la terminación - las formas",
                "de la terminación más larga sin el modelo, modelo - las probabilidades de analyze",
            ),
        ),
        "choice": (
            "Análisis de «vino»",
            "sin contexto",
            "Ella vino ayer.",
            "Bebimos un vino tinto.",
            "Probabilidades: sin contexto - las frecuencias del diccionario, en una oración - "
            "el modelo de contexto",
        ),
    },
}


def read_weights(path: Path) -> tuple[list[int], dict[str, int], dict[str, int]]:
    """
    The sizes of a model of esMorph from its file of weights: the numbers of the entry config,
    the numbers of strings of the tables and the numbers of weights of the matrices

    Args:
        path (Path): File of weights

    Returns:
        tuple[list[int], dict[str, int], dict[str, int]]: Config, strings and weights by entry
    """
    data = path.read_bytes()
    count = struct.unpack_from("<I", data, 12)[0]
    at, config, strings, weights = 16, [], {}, {}
    for _ in range(count):
        size = struct.unpack_from("<H", data, at)[0]
        name = data[at + 2 : at + 2 + size].decode()
        at += 2 + size
        kind = data[at]
        at += 1
        if kind == 0:
            items = struct.unpack_from("<I", data, at)[0]
            at += 4
            for _ in range(items):
                at += 4 + struct.unpack_from("<I", data, at)[0]
            strings[name] = items
        elif kind in (1, 2):
            rows, cols = struct.unpack_from("<II", data, at)
            at += 8 + (rows * cols * 4 if kind == 1 else rows * 4 + rows * cols)
            weights[name] = rows * cols
        else:
            items = struct.unpack_from("<I", data, at)[0]
            config = list(struct.unpack_from(f"<{items}I", data, at + 4))
            at += 4 + 4 * items
    return config, strings, weights


def width(value: str, size: float) -> float:
    return len(value) * size * 0.56 + 6


def text(
    x: float,
    y: float,
    value: str,
    size: float = 16,
    color: str = INK,
    bold: bool = False,
    anchor: str = "middle",
) -> str:
    weight = ' font-weight="bold"' if bold else ""
    return (
        f'<text x="{x:.1f}" y="{y:.1f}" font-size="{size}" fill="{color}"{weight} '
        f'text-anchor="{anchor}">{value}</text>'
    )


def arrow(x: float, y: float, size: float = 7, color: str = DARK) -> str:
    return (
        f'<path d="M{x - size:.1f} {y - 1.6 * size:.1f}L{x + size:.1f} {y - 1.6 * size:.1f}'
        f'L{x:.1f} {y:.1f}Z" fill="{color}"/>'
    )


def down(x: float, y1: float, y2: float) -> list[str]:
    return [
        f'<path d="M{x:.1f} {y1:.1f}L{x:.1f} {y2 - 9:.1f}" stroke="{DARK}" stroke-width="2"/>',
        arrow(x, y2, 6),
    ]


def box(
    x: float,
    y: float,
    w: float,
    h: float,
    label: str,
    fill: str = SOFT,
    color: str = INK,
    size: float = 15,
) -> list[str]:
    return [
        f'<rect x="{x:.1f}" y="{y:.1f}" width="{w:.1f}" height="{h:.1f}" rx="8" fill="{fill}" '
        f'stroke="{DARK}" stroke-width="1.5"/>',
        text(x + w / 2, y + h / 2 + size * 0.35, label, size, color),
    ]


def save(name: str, lang: str, w: float, h: float, body: list[str]) -> None:
    """A figure on a white background: it stays readable in the dark theme of the site"""
    suffix = "" if lang == "en" else f".{lang}"
    svg = (
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {w:.0f} {h:.0f}" width="{w:.0f}" '
        f'height="{h:.0f}" font-family="{FONT}" role="img">\n'
        f'<rect width="{w:.0f}" height="{h:.0f}" rx="12" fill="white"/>\n'
        + "\n".join(body)
        + "\n</svg>\n"
    )
    (IMG / f"{name}{suffix}.svg").write_text(svg, encoding="utf-8")


def describe(analysis: Analysis, lang: str) -> str:
    """The part of speech and the main features of an analysis in plain words: "verb, past, 3rd
    person" """
    t = TEXTS[lang]
    parts = [t["pos"].get(analysis.pos, analysis.pos)]
    for key in ("Tense", "Person", "Gender", "Number"):
        value = analysis.feats.get(key)
        if (key, value) in t["feats"] and len(parts) < 3:
            parts.append(t["feats"][key, value])
    return ", ".join(parts)


def percent(value: float, lang: str) -> str:
    # An analysis that the package does not give is 0, not "<0.01"
    shown = "0" if value == 0 else f"{value:.2f}" if value >= 0.005 else "&lt;0.01"
    return shown if lang == "en" else shown.replace(".", ",")


def number(value: float, lang: str) -> str:
    shown = f"{value:.2f}"
    return shown if lang == "en" else shown.replace(".", ",")


def draw_pipeline(
    words: list[str], dictionary: list[list[Analysis]], chosen: list[Analysis], lang: str
) -> None:
    steps = TEXTS[lang]["steps"]
    column, top = 210, 3
    xs = [20 + column * k + column / 2 for k in range(len(words))]
    body = [
        text(20, 30, steps[0], 15, DARK, True, "start"),
        text(20, 62, " ".join(words[:-1]) + words[-1], 22, INK, False, "start"),
        text(20, 104, steps[1], 15, DARK, True, "start"),
    ]
    rows = max(min(len(found), top) for found in dictionary)
    for x, word, found in zip(xs, words, dictionary, strict=True):
        body.append(text(x, 136, word, 19, INK, True))
        for k, analysis in enumerate(found[:top]):
            y = 150 + k * 48
            body.append(
                f'<rect x="{x - column / 2 + 8:.1f}" y="{y}" width="{column - 16}" height="42" '
                f'rx="6" fill="white" stroke="{LIGHT}" stroke-width="1.5"/>'
            )
            body.append(text(x, y + 18, f"{analysis.lemma} · {percent(analysis.prob, lang)}", 15))
            body.append(text(x, y + 35, describe(analysis, lang), 12, MUTED))
        if len(found) > top:
            body.append(text(x, 150 + top * 48 + 6, f"+ {len(found) - top}", 13, MUTED))
    y0 = 150 + rows * 48 + 40
    body.append(text(20, y0, steps[2], 15, DARK, True, "start"))
    for x, word, analysis in zip(xs, words, chosen, strict=True):
        body.append(text(x, y0 + 32, word, 19, INK, True))
        body.append(
            f'<rect x="{x - column / 2 + 8:.1f}" y="{y0 + 46}" width="{column - 16}" height="42" '
            f'rx="6" fill="{DARK}"/>'
        )
        body.append(text(x, y0 + 64, analysis.lemma, 15, "white", True))
        body.append(text(x, y0 + 81, describe(analysis, lang), 12, "white"))
    save("pipeline", lang, max(40 + column * len(words), width(steps[1], 15) + 40), y0 + 110, body)


def draw_context(config: list[int], params: float, lang: str) -> None:
    d, layers, dw, ds, dc, dshape, kmax, suffix_max = config
    t = TEXTS[lang]["context"]
    fmt = {
        "din": dshape + dc + dw + suffix_max * ds,
        "d": d,
        "half": d // 2,
        "layers": layers,
        "dc": dc,
        "kmax": kmax,
        "suffix_max": suffix_max,
        "params": number(params, lang),
        "x": TIMES,
    }
    body = []
    dims = (str(dshape), str(dw), f"{suffix_max} {TIMES} {ds}", str(dc))
    for k, (name, dim) in enumerate(zip(t[0], dims, strict=True)):
        x = 40 + k * 205
        body += box(x, 20, 185, 54, "", "white")
        body += [text(x + 92, 43, name.format(**fmt), 15), text(x + 92, 63, dim, 13, MUTED)]
        body += down(x + 92, 74, 104)
    y = 104
    for k, label in enumerate(t[1:7]):
        dark = k == 2
        fill, color = (DARK, "white") if dark else (SOFT, INK)
        body += box(40, y, 800, 44, label.format(**fmt), fill, color)
        if k < 5:
            body += down(440, y + 44, y + 70)
        y += 70
    body.append(text(440, y + 14, t[7].format(**fmt), 13, MUTED))
    save("context", lang, 880, y + 34, body)


def draw_choice(candidates: list[Analysis], columns: list[list[float]], lang: str) -> None:
    t = TEXTS[lang]["choice"]
    # An SVG in <img> does not see the fonts of the site: the width of a caption is for Arial
    cell = max(150, *(round(width(name, 14)) + 20 for name in t[1:4]))
    left, topy = 260, 92
    chosen = [max(range(len(candidates)), key=lambda i: probs[i]) for probs in columns]
    body = [text(30, 34, t[0], 17, DARK, True, "start")]
    for j, name in enumerate(t[1:4]):
        body.append(text(left + j * cell + cell / 2, topy - 14, name, 14, INK, True))
    for i, analysis in enumerate(candidates):
        y = topy + i * 52
        body.append(text(left - 14, y + 22, analysis.lemma, 16, INK, True, "end"))
        body.append(text(left - 14, y + 40, describe(analysis, lang), 12, MUTED, False, "end"))
        for j, probs in enumerate(columns):
            x = left + j * cell
            p = probs[i]
            body.append(
                f'<rect x="{x + 2}" y="{y + 2}" width="{cell - 4}" height="48" fill="{DARK}" '
                f'fill-opacity="{min(1.0, p) ** 0.6:.3f}"/>'
            )
            color = "white" if p > 0.35 else INK
            body.append(text(x + cell / 2, y + 32, percent(p, lang), 16, color, True))
            if chosen[j] == i:
                body.append(
                    f'<rect x="{x + 4}" y="{y + 4}" width="{cell - 8}" height="44" fill="none" '
                    f'stroke="{INK}" stroke-width="3" rx="4"/>'
                )
    bottom = topy + len(candidates) * 52
    body.append(text(30, bottom + 30, t[4], 13, MUTED, False, "start"))
    save("choice", lang, max(left + 3 * cell + 30, width(t[4], 13) + 40), bottom + 50, body)


def draw_paradigm(config: list[int], strings: dict[str, int], params: float, lang: str) -> None:
    dim, hidden, dchar, max_chars = config
    t = TEXTS[lang]["paradigm"]
    fmt = {
        "dim": dim,
        "hidden": hidden,
        "dchar": dchar,
        "max_chars": max_chars,
        "params": number(params, lang),
        "keys": strings["keys"],
        "suffixes": f"{strings['suffixes']:,}".replace(",", " " if lang == "es" else ","),
        "x": TIMES,
    }
    left, right, half = 40, 460, 380
    body = []
    for x, (name, dim_text) in ((left, t[0]), (right, t[1])):
        body += box(x, 20, half, 54, "", "white")
        body += [
            text(x + half / 2, 43, name.format(**fmt), 15),
            text(x + half / 2, 63, dim_text.format(**fmt), 13, MUTED),
        ]
        body += down(x + half / 2, 74, 104)
    body += box(left, 104, half, 44, t[2].format(**fmt), DARK, "white", 16)
    for k, (first, second) in enumerate(t[3]):
        x = right + k * 96
        body += box(x, 104, 92, 44, "")
        body += [text(x + 46, 122, first, 13), text(x + 46, 139, second, 11)]
        body += down(x + 46, 148, 174)
    body += down(left + half / 2, 148, 174)
    body += box(left, 174, half, 44, t[4].format(**fmt))
    body += box(right, 174, half, 44, t[5].format(**fmt))
    body += down(left + half / 2, 218, 244) + down(right + half / 2, 218, 244)
    y = 244
    for k, label in enumerate(t[6:10]):
        dark = k == 3
        fill, color = (DARK, "white") if dark else (SOFT, INK)
        body += box(left, y, 800, 44, label.format(**fmt), fill, color, 14)
        if k < 3:
            body += down(440, y + 44, y + 70)
        y += 70
    body.append(text(440, y + 14, t[10].format(**fmt), 13, MUTED))
    save("paradigm", lang, 880, y + 34, body)


def draw_guess(word: str, rows: list[tuple[str, str, float, float]], lang: str) -> None:
    t = TEXTS[lang]["guess"]
    cell, left, topy = 190, 270, 92
    body = [text(30, 34, t[0].format(word=word), 17, DARK, True, "start")]
    for j, name in enumerate(t[1:3]):
        body.append(text(left + j * cell + cell / 2, topy - 14, name, 14, INK, True))
    best = [max(range(len(rows)), key=lambda i: rows[i][2 + j]) for j in range(2)]
    for i, (lemma, pos, by_ending, by_model) in enumerate(rows):
        y = topy + i * 52
        body.append(text(left - 14, y + 22, lemma, 16, INK, True, "end"))
        body.append(
            text(left - 14, y + 40, TEXTS[lang]["pos"].get(pos, pos), 12, MUTED, False, "end")
        )
        for j, p in enumerate((by_ending, by_model)):
            x = left + j * cell
            body.append(
                f'<rect x="{x + 2}" y="{y + 2}" width="{cell - 4}" height="48" fill="{DARK}" '
                f'fill-opacity="{min(1.0, p) ** 0.6:.3f}"/>'
            )
            color = "white" if p > 0.35 else INK
            body.append(text(x + cell / 2, y + 32, percent(p, lang), 16, color, True))
            if best[j] == i:
                body.append(
                    f'<rect x="{x + 4}" y="{y + 4}" width="{cell - 8}" height="44" fill="none" '
                    f'stroke="{INK}" stroke-width="3" rx="4"/>'
                )
    bottom = topy + len(rows) * 52
    for k, line in enumerate(t[3]):
        body.append(text(30, bottom + 30 + k * 18, line, 13, MUTED, False, "start"))
    w = max(left + 2 * cell + 30, *(width(line, 13) + 40 for line in t[3]))
    save("guess", lang, w, bottom + 68, body)


def guess_rows(morph: Analyzer, word: str) -> list[tuple[str, str, float, float]]:
    """
    The analyses of an unknown word by lemma and part of speech: the share by the forms of the
    longest ending as guessing without the model gives it (the number of word forms times how
    often the treebanks have the tag) and the probability of the package

    Args:
        morph (Analyzer): Analyzer
        word (str): Unknown word

    Returns:
        list[tuple[str, str, float, float]]: Lemma, part of speech, share by the ending, probability
    """
    candidates = morph._guess_examples(word)
    tags, weights = morph._ud_tags(), morph._tag_weights()
    longest = max(candidate[4] for candidate in candidates)
    counted = [c for c in candidates if c[4] == longest][:MAX_COUNTED]
    total = sum(c[5] * weights[c[1]] for c in counted)
    shares: dict[tuple[str, str], list[float]] = {}
    for candidate in counted:
        key = (candidate[0], tags[candidate[1]][0])
        shares.setdefault(key, [0.0, 0.0])[0] += candidate[5] * weights[candidate[1]] / total
    for analysis in morph.analyze(word):
        assert analysis.source == "guess", analysis
        shares.setdefault((analysis.lemma, analysis.pos), [0.0, 0.0])[1] += analysis.prob
    rows = sorted(
        (item for item in shares.items() if max(item[1]) >= 0.005), key=lambda item: -max(item[1])
    )[:5]
    return [(lemma, pos, by_ending, by_model) for (lemma, pos), (by_ending, by_model) in rows]


def main() -> None:
    morph = Analyzer()
    dictionary = [morph.analyze(word) for word in EXAMPLE]
    chosen = [found[0] for found in morph.analyze_sentence(EXAMPLE)]
    # The analyses of the word in the order without context, and their probabilities everywhere
    alone = morph.analyze(WORD)
    columns = [[analysis.prob for analysis in alone]]
    for sentence in (EXAMPLE, OTHER):
        found = morph.analyze_sentence(sentence)[sentence.index(WORD)]
        probs = {(a.lemma, a.pos, tuple(a.feats.items())): a.prob for a in found}
        columns.append([probs.get((a.lemma, a.pos, tuple(a.feats.items())), 0.0) for a in alone])
    context, _, context_weights = read_weights(CONTEXT)
    paradigms, strings, paradigm_weights = read_weights(PARADIGMS)
    rows = guess_rows(morph, UNKNOWN)
    for lang in TEXTS:
        draw_pipeline(EXAMPLE, dictionary, chosen, lang)
        draw_context(context, sum(context_weights.values()) / 1e6, lang)
        draw_choice(alone, columns, lang)
        draw_paradigm(paradigms, strings, sum(paradigm_weights.values()) / 1e6, lang)
        draw_guess(UNKNOWN, rows, lang)
    print("docs/img: pipeline, context, choice, paradigm, guess (.svg, .es.svg)")


if __name__ == "__main__":
    main()
