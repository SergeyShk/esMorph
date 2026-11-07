# esMorph

**esMorph** is a morphological analyzer of Spanish: a Rust core with a Python API. For a word form it will find the analyses in a dictionary - the lemma, the part of speech and the [Universal Dependencies](https://universaldependencies.org/) features - and give the forms of a lexeme by their features: conjugation, gender and number.

!!! warning "Work in progress"
    So far the package contains only the scaffolding; the analyzer comes in the next versions. Versions `0.1.0.devN` are published on TestPyPI.

## Installation

Python 3.11 or newer is required. Pre-release versions are published only on TestPyPI:

``` bash
pip install --index-url https://test.pypi.org/simple/ esmorph
```

Prebuilt wheels and building from source are described on the [Installation](installation.md) page.
