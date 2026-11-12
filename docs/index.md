# esMorph

![esMorph](img/esmorph.svg)

**esMorph** is a morphological analyzer of Spanish: a Rust core with a Python API. For a word form the analyzer finds its analyses in a dictionary built from Wiktionary - the lemma, the part of speech and the [Universal Dependencies](https://universaldependencies.org/) features - orders them by probability and builds the forms of the word: conjugation, gender and number.

!!! note "Early version"
    Before 1.0 the API may change between 0.x versions; the changes are described in the release notes.

## Features

<div class="grid cards" markdown>

-   :material-magnify:{ .lg .middle } __Analysis__

    ---

    Lemma, UD part of speech and features, probability of the analysis; the analysis of a sentence in context, lists of words, lemmatization and documents of the anySeg segmenter without the GIL

    [:octicons-arrow-right-24: Analyzer](analyzer.md)

-   :material-tag-text-outline:{ .lg .middle } __Universal Dependencies__

    ---

    Parts of speech, features and lemmas by the UD guidelines and the AnCora conventions; verbs with clitics, numbers, signs and names by rules, unknown words by their ending

    [:octicons-arrow-right-24: Word analysis](analysis.md)

-   :material-format-letter-case:{ .lg .middle } __Inflection__

    ---

    Forms of the lexeme and the form by UD features: conjugation, gender and number

    [:octicons-arrow-right-24: Inflection](inflection.md)

-   :material-console:{ .lg .middle } __Command line__

    ---

    The `esmorph` command: lemmas, CoNLL-U, TSV or JSON Lines of text, tokens or CoNLL-U files

    [:octicons-arrow-right-24: The esmorph command](cli.md)

-   :material-puzzle-outline:{ .lg .middle } __spaCy__

    ---

    The `esmorph` component: the analysis in context in place of the tagger, the morphologizer and the lemmatizer of a pipeline

    [:octicons-arrow-right-24: spaCy](spacy.md)

-   :material-speedometer:{ .lg .middle } __Comparison__

    ---

    Accuracy, inflection and speed compared with simplemma, spaCy, Stanza, ablaut and verbecc, loading and memory

    [:octicons-arrow-right-24: Comparison](comparison.md)

-   :material-alert-circle-outline:{ .lg .middle } __Exceptions__

    ---

    Built-in Python exceptions and `FormNotFoundError` for missing forms

    [:octicons-arrow-right-24: Exceptions](exceptions.md)

</div>

## Installation

Python 3.11 or newer is required.

``` bash
pip install esmorph
```

Prebuilt wheels, building from source and the Rust crate are described on the [Installation](installation.md) page.

## Quick start

=== "Code"

    ``` python
    from esmorph import Analyzer

    morph = Analyzer()
    morph.lemmatize(["Los", "niños", "cantaban", "canciones"])
    [(a.lemma, a.pos, round(a.prob, 3)) for a in morph.analyze("casa")][:2]
    morph.analyze("gatos")[0].inflect(Gender="Fem")
    morph.analyze("hablar")[0].inflect(Tense="Past", Person="3", Number="Plur")
    ```

=== "Result"

    ```
    ['el', 'niño', 'cantar', 'canción']
    [('casa', 'NOUN', 0.986), ('casar', 'VERB', 0.013)]
    'gatas'
    'hablaron'
    ```

??? note "Project structure"

    *   **crates/esmorph** - the Rust core of the analyzer: dictionary format, building, analysis, rules, guessing and inflection
    *   **crates/esmorph-build** - building the dictionary from the export of Wiktionary and the counts of the treebanks
    *   **crates/esmorph-dict-es** - the crate with the dictionary for `esmorph` on crates.io
    *   **crates/esmorph-py** - the Python module `esmorph._esmorph` on PyO3
    *   **python/esmorph** - the Python package
    *   **scripts** - exporting Wiktionary and the counts of the treebanks, the conventions of the dictionary, checking the installed package before publishing and the comparison with other tools
    *   **data** - the tables of closed classes after UD Spanish AnCora and of the symbols of units
    *   **docs** - MkDocs documentation in English and Spanish
    *   **tests** - tests of the Python package
