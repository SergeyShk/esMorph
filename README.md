<p align="center">
  <img src="https://raw.githubusercontent.com/SergeyShk/esMorph/master/docs/img/esmorph.png" alt="esMorph" width="480">
</p>

<h1 align="center">esMorph</h1>

<p align="center">
  A fast Spanish morphological analyzer written in Rust with a Python API
</p>

<p align="center">
  <a href="https://sergeyshk.github.io/esMorph/">Documentation</a> ·
  <a href="https://pypi.org/project/esmorph/">PyPI</a> ·
  <a href="https://crates.io/crates/esmorph">crates.io</a> ·
  <a href="https://github.com/SergeyShk/esMorph/blob/master/README.es.md">Español</a>
</p>

<p align="center">
  <a href="https://pypi.org/project/esmorph/"><img src="https://img.shields.io/pypi/v/esmorph?logo=pypi&logoColor=FFE873" alt="Version"></a>
  <a href="https://pypi.org/project/esmorph/"><img src="https://img.shields.io/pypi/pyversions/esmorph.svg?logo=python&logoColor=FFE873" alt="Supported Python versions"></a>
  <a href="https://crates.io/crates/esmorph"><img src="https://img.shields.io/crates/v/esmorph?logo=rust" alt="Crate"></a>
  <a href="https://github.com/SergeyShk/esMorph/actions/workflows/ci.yml"><img src="https://github.com/SergeyShk/esMorph/actions/workflows/ci.yml/badge.svg" alt="Build"></a>
  <a href="https://github.com/astral-sh/ruff"><img src="https://img.shields.io/endpoint?url=https://raw.githubusercontent.com/astral-sh/ruff/main/assets/badge/v2.json" alt="Ruff"></a>
  <a href="https://github.com/SergeyShk/esMorph/blob/master/LICENSE.txt"><img src="https://img.shields.io/github/license/sergeyshk/esmorph.svg" alt="License"></a>
  <a href="https://doi.org/10.5281/zenodo.24151767"><img src="https://zenodo.org/badge/DOI/10.5281/zenodo.24151767.svg" alt="DOI"></a>
</p>

---

**esMorph** is a morphological analyzer of Spanish: a Rust core with a Python API. For a word form it finds the analyses in a dictionary built from [Wiktionary](https://en.wiktionary.org) - the lemma, the part of speech and the [Universal Dependencies](https://universaldependencies.org/) features - orders them by probability and builds the forms of the word: conjugation, gender and number.

It chooses the analysis of a word by its sentence with a small context model, almost as accurately as spaCy on AnCora and more accurately than spaCy and Stanza on PUD and COSER, and analyzes 177 thousand tokens per second in context and 1.9 million without, 3.5 and 37 times faster than spaCy; it lemmatizes almost as fast as simplemma, whose lemmas are right less often on the UD test sets of AnCora, PUD and COSER. Parts of speech, features and lemmas follow the UD guidelines and the conventions of the AnCora treebank, so the results can be compared with spaCy, Stanza and other UD tools.

* **[Analyzer](https://sergeyshk.github.io/esMorph/analyzer/)** - the analyses of a word form by probability, the analyses of a sentence in context, lists of words and lemmatization, documents of the [anySeg](https://sergeyshk.github.io/anySeg/) segmenter, checking a word form against the dictionary
* **[Universal Dependencies](https://sergeyshk.github.io/esMorph/analysis/#ud)** - parts of speech, features and lemmas by the UD guidelines and the AnCora conventions
* **[Verbs with clitics](https://sergeyshk.github.io/esMorph/analysis/#clitics)** - a verb with enclitic pronouns and its words: `dámelo` is `da` + `me` + `lo`
* **[Tokens out of the dictionary](https://sergeyshk.github.io/esMorph/analysis/#rules)** - punctuation, numbers, ordinals, Roman numerals, names, addresses and abbreviations by rules
* **[Unknown words](https://sergeyshk.github.io/esMorph/analysis/#guess)** - analyses by the ending, a hyphen or a prefix, and the forms of guessed words
* **[Inflection](https://sergeyshk.github.io/esMorph/inflection/)** - the forms of a lexeme and the form with given UD features
* **[The esmorph command](https://sergeyshk.github.io/esMorph/cli/)** - lemmas, CoNLL-U, TSV or JSON Lines of text, tokens or CoNLL-U from files and the standard input
* **[spaCy](https://sergeyshk.github.io/esMorph/spacy/)** - the component `esmorph` with the analysis in context in place of the tagger, the morphologizer and the lemmatizer
* **[Comparison](https://sergeyshk.github.io/esMorph/comparison/)** - accuracy, inflection and speed compared with simplemma, spaCy, Stanza, ablaut and verbecc

> [!NOTE]
> Early version: before 1.0 the API may change between 0.x versions; the changes are described in the release notes.

## Installation

Python 3.11 or newer is required.

```bash
pip install esmorph
```

Or with [uv](https://docs.astral.sh/uv/):

```bash
uv add esmorph
```

Wheels are built for Linux (Alpine included), macOS and Windows and for the free-threaded Python 3.14t; Rust is not needed to install them, and the dictionary is in the package. On other platforms pip builds the package from source, which requires [Rust](https://www.rust-lang.org/tools/install) 1.88 or newer. See the [Installation](https://sergeyshk.github.io/esMorph/installation/) page.

For Rust there is the crate [`esmorph`](https://crates.io/crates/esmorph) with the dictionary (`cargo add esmorph`; the documentation is on [docs.rs](https://docs.rs/esmorph)).

## Quick start

```python
>>> from esmorph import Analyzer

>>> morph = Analyzer()
>>> morph.lemmatize("Los niños cantaban en la plaza".split())
['el', 'niño', 'cantar', 'en', 'el', 'plaza']
>>> analysis = morph.analyze("camiones")[0]
>>> analysis.lemma, analysis.pos, analysis.feats
('camión', 'NOUN', {'Gender': 'Masc', 'Number': 'Plur'})
>>> morph.analyze("hablar")[0].inflect(Tense="Past", Person="3", Number="Plur")
'hablaron'
>>> [form.word for form in morph.analyze("bonitas")[0].forms()]
['bonito', 'bonita', 'bonitos', 'bonitas']
```

The analyzer is created once: it loads the dictionary, and then it can be used in the whole program.

## Features

### Analysis of a word form

`analyze` gives the analyses of a word form, the most probable first: objects `Analysis` with the lemma, the part of speech, the UD features, the probability and the source of the analysis. Case does not matter for finding a word, though a word with a capital letter may also be a name, and a word without its accents is found too. Analyses are immutable: they can be compared, put into sets and pickled.

```python
>>> [(a.lemma, a.pos, round(a.prob, 3)) for a in morph.analyze("canto")]
[('canto', 'NOUN', 0.999), ('cantar', 'VERB', 0.001)]
>>> analysis = morph.analyze("dámelo")[0]
>>> analysis.lemma, analysis.pos, analysis.words
('dar', 'VERB', ('da', 'me', 'lo'))
>>> morph.is_known("Camion"), morph.is_known("qzxw")
(True, False)
```

`analyze_many` analyzes a list of words; `lemmatize` gives their lemmas, and `annotate` fills the lemmas, parts of speech and features of the words of a document of the [anySeg](https://sergeyshk.github.io/anySeg/) segmenter, both without the GIL. The analyzer and the analyses are pickled, so they work with `multiprocessing` and joblib.

The `esmorph` command, installed with the package, analyzes text from files and the standard input:

```bash
echo "Dime la verdad del caso." | esmorph
```

```
decir yo el verdad de el caso .
```

See the documentation: [analyzer](https://sergeyshk.github.io/esMorph/analyzer/), [word analysis](https://sergeyshk.github.io/esMorph/analysis/), [the esmorph command](https://sergeyshk.github.io/esMorph/cli/).

### Analysis in context

`analyze_sentence` orders the analyses of the tokens of a sentence with a small neural network trained on the treebanks UD Spanish AnCora and GSD: the first is the analysis the context chooses, and its probability is the score of the model. `annotate` and the command analyze in context by default; `context=False` and `--no-context` order the analyses as `analyze` does, about nine times faster. The `esmorph` component puts the analysis in context into a [spaCy](https://spacy.io/) pipeline (`pip install "esmorph[spacy]"`).

```python
>>> sentence = morph.analyze_sentence(["Ella", "la", "vio"])
>>> [(analyses[0].lemma, analyses[0].pos) for analyses in sentence]
[('él', 'PRON'), ('él', 'PRON'), ('ver', 'VERB')]
>>> morph.analyze("la")[0].pos
'DET'
>>> import spacy
>>> nlp = spacy.blank("es")
>>> _ = nlp.add_pipe("esmorph")
>>> [(token.lemma_, token.pos_) for token in nlp("Ella la vio del balcón.")]
[('él', 'PRON'), ('él', 'PRON'), ('ver', 'VERB'), ('de', 'ADP'), ('balcón', 'NOUN'), ('.', 'PUNCT')]
```

See the documentation: [analysis in context](https://sergeyshk.github.io/esMorph/analyzer/#analyze_sentence), [spaCy](https://sergeyshk.github.io/esMorph/spacy/).

<details>
<summary><b>Parts of speech and features of Universal Dependencies</b></summary>

<br>

Parts of speech, features and lemmas follow the [Universal Dependencies](https://universaldependencies.org/u/pos/) guidelines and, where they are silent, the conventions of the largest Spanish treebank, [UD Spanish AnCora](https://github.com/UniversalDependencies/UD_Spanish-AnCora):

*   determiners, pronouns, prepositions and conjunctions have the lemmas and features of AnCora: `la` is the article `el` or the pronoun `él`, `se` is `él` with `Reflex=Yes`, `su` is `DET` with `Poss=Yes`
*   `ser`, `estar`, `haber` and the other auxiliaries have both `AUX` and `VERB` analyses
*   a participle is a verb and an adjective, verbs have no `-se` in the lemma, a proper name has a lemma with a capital letter

```python
>>> [(w, morph.analyze(w)[0].lemma, morph.analyze(w)[0].pos) for w in ["la", "su", "hay", "madrid"]]
[('la', 'el', 'DET'), ('su', 'su', 'DET'), ('hay', 'haber', 'VERB'), ('madrid', 'Madrid', 'PROPN')]
>>> morph.analyze("la")[0].feats
{'Definite': 'Def', 'Gender': 'Fem', 'Number': 'Sing', 'PronType': 'Art'}
```

See the [documentation](https://sergeyshk.github.io/esMorph/analysis/#ud).

</details>

<details>
<summary><b>Tokens out of the dictionary and unknown words</b></summary>

<br>

Tokens that the dictionary does not have get the analyses of rules, with `source` equal to `"rule"`: punctuation, symbols, numbers in digits, ordinals (`2.ª`), Roman numerals, names, addresses and abbreviations. A lowercase word that neither the dictionary nor the rules know is guessed, with `source` equal to `"guess"`: by its last part after a hyphen, by the word without a known prefix, as a verb with clitics or by the dictionary words with its ending. A guessed word has its forms too.

```python
>>> [(a.lemma, a.pos, a.source) for w in ["2.ª", "10.000", "xii"] for a in morph.analyze(w)]
[('2.º', 'ADJ', 'rule'), ('10000', 'NUM', 'rule'), ('xii', 'NUM', 'rule')]
>>> [(morph.analyze(w)[0].lemma, morph.analyze(w)[0].source) for w in ["supergallos", "esbeltísima"]]
[('supergallo', 'guess'), ('esbeltísimo', 'guess')]
>>> morph.analyze("zumbeábamos")[0].inflect(Tense="Pres", Person="3", Number="Sing")
'zumbea'
```

See the documentation: [tokens out of the dictionary](https://sergeyshk.github.io/esMorph/analysis/#rules), [unknown words](https://sergeyshk.github.io/esMorph/analysis/#guess).

</details>

<details>
<summary><b>Inflection</b></summary>

<br>

`forms()` gives the forms of the lexeme of an analysis in the order of its paradigm, and `inflect` gives the form with the given UD features, keeping the other features of the analysis. A variant, such as a form of `vos` or an older spelling, comes only from a variant.

```python
>>> morph.analyze("gata")[0].inflect(Number="Plur")
'gatas'
>>> morph.analyze("hablás")[0].inflect(Mood="Imp")
'hablá'
```

See the [documentation](https://sergeyshk.github.io/esMorph/inflection/).

</details>

<details>
<summary><b>Comparison</b></summary>

<br>

The share of words whose first analysis has the right lemma, and the right part of speech, features and lemma all together, % (test parts of the UD 2.18 treebanks), and the speed of text analysis:

| System | Lemma, AnCora | Lemma, PUD | Lemma, COSER | All together, AnCora | All together, PUD | All together, COSER | Tokens per second |
| :----- | :------------ | :--------- | :----------- | :------------------- | :---------------- | :------------------ | :---------------- |
| esMorph | 97.80 | 97.74 | 94.49 | 90.05 | **87.90** | 86.87 | 1,877,869 |
| esMorph in context | **99.29** | 97.71 | **95.66** | 95.67 | 87.52 | **89.76** | 177,200 |
| simplemma | 96.94 | 96.96 | 93.50 | - | - | - | **2,455,730** |
| spaCy es_core_news_sm | 98.66 | 96.69 | 93.39 | 95.92 | 83.59 | 85.79 | 50,175 |
| Stanza | 99.16 | **97.80** | 94.98 | **97.50** | 86.34 | 89.16 | 3,171 |

In context esMorph chooses the analysis by the sentence, as spaCy and Stanza do; they are trained on AnCora and lead on it in features and all together, while on PUD and COSER, new to all of them, the part of speech, features and lemma of esMorph are right together more often. An analysis with the right part of speech and lemma is among the analyses of esMorph for 96-99 % of words. It gives the gold form of 99.65, 93.29 and 95.55 % of the verbs of AnCora, PUD and COSER, against 98.87, 92.71 and 93.61 % of ablaut. A short text together with loading takes 3.0 ms against 0.33-1.7 s, and every further process with the same dictionary adds 3.6 MB of memory. Measured on Apple M3 Pro, Python 3.11.

See the [documentation](https://sergeyshk.github.io/esMorph/comparison/): all the scores, verb inflection, the chart of speed by the size of the text, memory and how it was measured.

</details>

## Development

The project uses [uv](https://docs.astral.sh/uv/) to manage dependencies, [maturin](https://www.maturin.rs/) to build the Rust module and [ruff](https://docs.astral.sh/ruff/) for linting and formatting. Python 3.11 or newer and Rust 1.88 or newer are required.

```bash
git clone https://github.com/SergeyShk/esMorph.git
cd esMorph

make deps        # create the environment, install the dependencies and build the Rust module
make dictionary  # build the dictionary from the snapshot of Wiktionary, the counts of treebanks and the context model
make test        # run the Python tests, the docstring examples (doctest) and the Rust tests
make lint        # ruff, mypy, rustfmt and clippy
```

The full list of commands is `make help`. The documentation is bilingual: English pages `docs/*.md` with Spanish `docs/*.es.md` next to them; update both versions when editing a page.

Before submitting changes, install the hooks that run the linters on commit and the tests on push:

```bash
uv run pre-commit install
```

## Contributing

Bug reports, ideas and pull requests are welcome, in English or Spanish - the [issues](https://github.com/SergeyShk/esMorph/issues) are open. The workflow and the checks to run before submitting a pull request are in [CONTRIBUTING.md](https://github.com/SergeyShk/esMorph/blob/master/CONTRIBUTING.md).

<details>
<summary><b>Project structure</b></summary>

<br>

*   **crates/esmorph** - the analyzer core in Rust
*   **crates/esmorph-build** - the builder of the dictionary
*   **crates/esmorph-dict-es** - the dictionary as a crate
*   **crates/esmorph-py** - the Python module `esmorph._esmorph` built with PyO3
*   **python/esmorph** - the Python package
*   **scripts** - exporting Wiktionary and the counts of the treebanks, the conventions of the dictionary, training the context model, checking the installed package before publishing and the comparison with other tools
*   **data** - the tables of closed classes after UD Spanish AnCora and of the symbols of units, the weights of the context model
*   **docs** - MkDocs documentation in English and Spanish
*   **tests** - tests of the Python package

</details>

## Authors

*   Sergey Shkarin (kouki.sergey@gmail.com)

## License

The code is distributed under the [MIT](https://github.com/SergeyShk/esMorph/blob/master/LICENSE.txt) license. The dictionary is built from [Wiktionary](https://en.wiktionary.org) and the treebanks [UD Spanish AnCora](https://github.com/UniversalDependencies/UD_Spanish-AnCora) and [UD Spanish GSD](https://github.com/UniversalDependencies/UD_Spanish-GSD), and is distributed under the [CC BY-SA 4.0](https://github.com/SergeyShk/esMorph/blob/master/LICENSE-DICTIONARY.txt) license.

## Citation

If you use **esMorph** in research or software, please cite it with the following BibTeX entry: this helps the project grow and be maintained. The same metadata is in [CITATION.cff](https://github.com/SergeyShk/esMorph/blob/master/CITATION.cff) - GitHub shows it under "Cite this repository". The Concept DOI [10.5281/zenodo.24151767](https://doi.org/10.5281/zenodo.24151767) on Zenodo points to all versions of the library; the DOI of a version is on the page of its release.

```bibtex
@software{esMorph,
  author = {Sergey Shkarin},
  title = {{esMorph, a fast morphological analyzer for Spanish}},
  year = 2026,
  doi = {10.5281/zenodo.24151767},
  url = {https://github.com/SergeyShk/esMorph}
}
```
