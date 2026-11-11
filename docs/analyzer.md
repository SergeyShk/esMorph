# Analyzer

``` {.python .signature}
Analyzer(path=None, *, verify=False)
```

Morphological analyzer: for a word form it finds all [analyses](analysis.md) in a dictionary built from Wiktionary - the lemma, the part of speech and the [Universal Dependencies](https://universaldependencies.org/) features - and orders them by probability. A token that the dictionary does not have gets the analyses of [rules](analysis.md#rules), and an unknown lowercase word is [guessed](analysis.md#guess). The dictionary is loaded when the analyzer is created.

Exceptions:

- `FileNotFoundError` - the dictionary file does not exist;
- `OSError` - the dictionary file cannot be read: no access, the path points to a directory, a pipe or a device;
- `ValueError` - the file is not an esMorph dictionary or is damaged (with `verify=True`, a deliberately crafted file too), the path has a NUL character;
- `TypeError` - a path that is neither a string nor `PathLike`.

=== "Code"

    ``` python
    from esmorph import Analyzer

    morph = Analyzer()
    morph.analyze("hablamos")[0]
    ```

=== "Result"

    ```
    Analysis(word='hablamos', lemma='hablar', pos='VERB', feats={'Mood': 'Ind', 'Number': 'Plur', 'Person': '1', 'Tense': 'Pres', 'VerbForm': 'Fin'}, prob=0.978261, words=('hablamos',), source='dictionary')
    ```

## Parameters

| Parameter | Type | Default | Description |
| :-------- | :--- | :------ | :---------- |
| `path` | `str` / `PathLike` | `None` | Path to the dictionary file; by default, the dictionary of the package |
| `verify` | `bool` | `False` | Check the whole dictionary, for a file of an untrusted source; takes tens of milliseconds |

The object keeps the absolute dictionary path in `path` of type `Path`, read-only. The dictionary file is mapped into memory, so processes with the same file share its memory. While an analyzer exists, the file must not be overwritten in place, but it can be replaced with a new file; on Windows an open file can be neither replaced nor deleted. A dictionary is trusted data, like code: loading rejects a damaged file but not every deliberately crafted one, which may make the analysis fail. A dictionary of an untrusted source is loaded with `verify=True`: a deliberately crafted file is then rejected with `ValueError`, and pickle keeps the check.

## Methods

<div class="metric-card" markdown>

### Analyses of a word form { #analyze }

`analyze` · `list[Analysis]`
{ .metric-meta }

``` {.python .signature}
analyze(word)
```

Analyses of the word form, the most probable first; the probabilities sum to 1. Case does not matter for finding the word in the dictionary, though a token with a capital letter may also be a name, and a word absent as written is looked for without its accents and with a lost diaeresis ([spelling](analysis.md#spelling)). A verb with enclitic pronouns is analyzed as its verb ([clitics](analysis.md#clitics)), a token that the dictionary does not have - a sign, a number, a name - [by rules](analysis.md#rules), and an unknown lowercase word is [guessed](analysis.md#guess). A token that nothing analyzes - with a space or a control character, a lowercase word without a sample such as `qzxw` - has no analyses.

| Parameter | Type | Default | Description |
| :-------- | :--- | :------ | :---------- |
| `word` | `str` | - | Word form |

Exceptions:

- `TypeError` - a word that is not a string.

=== "Code"

    ``` python
    [(a.lemma, a.pos, round(a.prob, 3)) for a in morph.analyze("casa")]
    [(a.lemma, a.source) for a in morph.analyze("zumbeábamos")][:1]
    ```

=== "Result"

    ```
    [('casa', 'NOUN', 0.986), ('casar', 'VERB', 0.013), ('casar', 'VERB', 0.0)]
    [('zumbear', 'guess')]
    ```

</div>

<div class="metric-card" markdown>

### Analyses of a list of word forms { #analyze_many }

`analyze_many` · `list[list[Analysis]]`
{ .metric-meta }

``` {.python .signature}
analyze_many(words)
```

Analyses of every word form, as with [`analyze`](#analyze).

| Parameter | Type | Default | Description |
| :-------- | :--- | :------ | :---------- |
| `words` | `Iterable[str]` | - | Word forms: a list, a tuple, a generator |

Exceptions:

- `TypeError` - a string or bytes instead of a collection of words, a word that is not a string.

=== "Code"

    ``` python
    [len(analyses) for analyses in morph.analyze_many(["el", "gato", "come"])]
    ```

=== "Result"

    ```
    [2, 1, 2]
    ```

</div>

<div class="metric-card" markdown>

### Lemmatization { #lemmatize }

`lemmatize` · `list[str]`
{ .metric-meta }

``` {.python .signature}
lemmatize(words)
```

Lemmas of the most probable analyses of the word forms; a token without analyses comes back in lower case. Equal lemmas are one string object, so the lemmas of a large text take little memory; the method releases the GIL.

| Parameter | Type | Default | Description |
| :-------- | :--- | :------ | :---------- |
| `words` | `Iterable[str]` | - | Word forms: a list, a tuple, a generator |

Exceptions:

- `TypeError` - a string or bytes instead of a collection of words, a word that is not a string.

=== "Code"

    ``` python
    morph.lemmatize(["Los", "niños", "cantaban", "canciones"])
    ```

=== "Result"

    ```
    ['el', 'niño', 'cantar', 'canción']
    ```

</div>

<div class="metric-card" markdown>

### Analysis of a document { #annotate }

`annotate` · `None`
{ .metric-meta }

``` {.python .signature}
annotate(document)
```

Fills the columns `lemma`, `upos` and `feats` of the words of a document of the [anySeg](https://sergeyshk.github.io/anySeg/) segmenter with the most probable analysis of every word, as `analyze` orders them; the earlier values of these columns are replaced. The words of a multiword token are analyzed by the token: `dime` split into `di` + `me` is the imperative of `decir` with the pronoun `me`, and clitics after a verb are pronouns. A token that the segmenter leaves as one word gets the analysis of the whole token: `devuélvenos` is the imperative of `devolver`. A word without analyses gets its lower case as the lemma and the part of speech `X`. The method releases the GIL. esMorph does not depend on anySeg: the document is an `anyseg.Document` or an object with the same `column("form")`, `multiword_tokens` (a token has `text` and `words`, a word has `index`) and `set_columns`.

| Parameter | Type | Default | Description |
| :-------- | :--- | :------ | :---------- |
| `document` | `anyseg.Document` | - | Document of the segmenter (`pip install anyseg`) |

Exceptions:

- `ValueError` - the words of the multiword tokens are not consecutive words of the document in order;
- `TypeError` - a form or the text of a token that is not a string, the index of a word that is not an integer.

=== "Code"

    ``` python
    from anyseg import Segmenter

    document = Segmenter("es").segment("Dime la verdad.")
    morph.annotate(document)
    [(word.form, word.lemma, word.upos) for word in document.words]
    ```

=== "Result"

    ```
    [('Di', 'decir', 'VERB'), ('me', 'yo', 'PRON'), ('la', 'el', 'DET'), ('verdad', 'verdad', 'NOUN'), ('.', '.', 'PUNCT')]
    ```

</div>

<div class="metric-card" markdown>

### Word form in the dictionary { #is_known }

`is_known` · `bool`
{ .metric-meta }

``` {.python .signature}
is_known(word)
```

Whether the dictionary has the word form, as written or as a verb of the dictionary with clitics; as with `analyze`, case and lost accents do not matter. Tokens analyzed by rules and guessed words are not in the dictionary.

| Parameter | Type | Default | Description |
| :-------- | :--- | :------ | :---------- |
| `word` | `str` | - | Word form |

Exceptions:

- `TypeError` - a word that is not a string.

=== "Code"

    ``` python
    morph.is_known("Gatos"), morph.is_known("zumbeábamos")
    ```

=== "Result"

    ```
    (True, False)
    ```

</div>

## Dictionary { #dictionary }

The dictionary of the package is built from the English [Wiktionary](https://en.wiktionary.org) of October 2026: 676 thousand word forms of 117 thousand lexemes. Symbols of units (`km`, `kg`) come from a table of their own, and determiners, pronouns, prepositions, conjunctions and numerals in words from a table after the treebank [UD Spanish AnCora](https://github.com/UniversalDependencies/UD_Spanish-AnCora), whose conventions the lemmas and features follow ([Word analysis](analysis.md#ud)). The probabilities of analyses come from the training parts of UD Spanish AnCora and [UD Spanish GSD](https://github.com/UniversalDependencies/UD_Spanish-GSD); word forms they do not have get their order by their ending and the most common features. The dictionary is distributed under the [CC BY-SA 4.0](https://creativecommons.org/licenses/by-sa/4.0/) license.

The property `dictionary_info` describes the dictionary of the analyzer: the source and its snapshot with its SHA-256, the license, the release of Universal Dependencies and the treebanks of the probabilities with their licenses, the number of word forms and the checksum of the file. Every access returns a new dict.

=== "Code"

    ``` python
    info = morph.dictionary_info
    info["source"], info["license"], sorted(info["treebanks"])
    ```

=== "Result"

    ```
    ('Wiktionary', 'CC BY-SA 4.0', ['UD_Spanish-AnCora', 'UD_Spanish-GSD'])
    ```

## Threads and processes { #parallel }

One analyzer can be used from several threads. `lemmatize` and `annotate` release the GIL, so threads speed them up. `analyze_many` creates the analyses as Python objects, so threads speed it up little on any Python, the free-threaded 3.14t included: for a large text, `lemmatize` or processes are faster. The analyzer and analyses are pickled, so they work with `multiprocessing` and joblib: processes share the dictionary mapped into memory. Pickling an analysis costs more than finding it, so workers that return lemmas or tuples of fields are faster than workers that return analyses. A pickle keeps the absolute path of the dictionary file (none for the dictionary of the package) and the place of the analysis in the tables of that dictionary, which is looked up anew only with another dictionary: unpickling loads each dictionary once per process, so analyses unpickled one by one share one analyzer, an analysis whose dictionary file is gone takes the dictionary of the package, and a damaged file raises `ValueError`. An analysis that the dictionary does not have, as one pickled with another version of it, keeps its fields, but `forms()` gives an empty list and `inflect` raises `FormNotFoundError`.

=== "Code"

    ``` python
    from concurrent.futures import ThreadPoolExecutor

    texts = [["los", "niños", "cantaban"], ["el", "gato", "come"]]
    with ThreadPoolExecutor(2) as pool:
        lemmas = list(pool.map(morph.lemmatize, texts))
    lemmas
    ```

=== "Result"

    ```
    [['el', 'niño', 'cantar'], ['el', 'gato', 'comer']]
    ```
