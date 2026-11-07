<h1 align="center">esMorph</h1>

<p align="center">
  A fast Spanish morphological analyzer written in Rust with a Python API
</p>

<p align="center">
  <a href="https://github.com/SergeyShk/esMorph/blob/master/README.es.md">Español</a>
</p>

<p align="center">
  <a href="https://github.com/SergeyShk/esMorph/actions/workflows/ci.yml"><img src="https://github.com/SergeyShk/esMorph/actions/workflows/ci.yml/badge.svg" alt="Build"></a>
  <a href="https://github.com/astral-sh/ruff"><img src="https://img.shields.io/endpoint?url=https://raw.githubusercontent.com/astral-sh/ruff/main/assets/badge/v2.json" alt="Ruff"></a>
  <a href="https://github.com/SergeyShk/esMorph/blob/master/LICENSE.txt"><img src="https://img.shields.io/github/license/sergeyshk/esmorph.svg" alt="License"></a>
</p>

---

**esMorph** is a morphological analyzer of Spanish: a Rust core with a Python API. For a word form it will find the analyses in a dictionary - the lemma, the part of speech and the [Universal Dependencies](https://universaldependencies.org/) features - and give the forms of a lexeme by their features: conjugation, gender and number.

> [!WARNING]
> Work in progress: the analyses of word forms already work, but their order, words with clitics, unknown words and inflection come in the next versions. Versions `0.1.0.devN` are published on TestPyPI, and the documentation site appears with the first release.

## Installation

Python 3.11 or newer is required. Pre-release versions are published only on TestPyPI:

```bash
pip install --index-url https://test.pypi.org/simple/ esmorph
```

Wheels are built for Linux (Alpine included), macOS and Windows and for the free-threaded Python 3.14t; on other platforms building from source requires [Rust](https://www.rust-lang.org/tools/install) 1.88 or newer. See the [Installation](https://github.com/SergeyShk/esMorph/blob/master/docs/installation.md) page.

## Usage

```python
>>> from esmorph import Analyzer
>>> morph = Analyzer()
>>> analysis = morph.analyze("camiones")[0]
>>> analysis.lemma, analysis.pos, analysis.feats
('camión', 'NOUN', {'Gender': 'Masc', 'Number': 'Plur'})
>>> morph.is_known("hablamos")
True
```

## Development

The project uses [uv](https://docs.astral.sh/uv/) to manage dependencies, [maturin](https://www.maturin.rs/) to build the Rust module and [ruff](https://docs.astral.sh/ruff/) for linting and formatting. Python 3.11 or newer and Rust 1.88 or newer are required.

```bash
git clone https://github.com/SergeyShk/esMorph.git
cd esMorph

make deps        # create the environment, install the dependencies and build the Rust module
make dictionary  # build the dictionary from the snapshot of Wiktionary
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
*   **docs** - MkDocs documentation in English and Spanish
*   **tests** - tests of the Python package

</details>

## Authors

*   Sergey Shkarin (kouki.sergey@gmail.com)

## License

The code is distributed under the [MIT](https://github.com/SergeyShk/esMorph/blob/master/LICENSE.txt) license. The dictionary is built from [Wiktionary](https://en.wiktionary.org) and is distributed under the [CC BY-SA 4.0](https://github.com/SergeyShk/esMorph/blob/master/LICENSE-DICTIONARY.txt) license.
