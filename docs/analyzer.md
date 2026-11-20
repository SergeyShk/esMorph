# Analyzer

``` {.python .signature}
Analyzer(path=None, *, verify=False, words=None)
```

Morphological analyzer: for a word form it finds all [analyses](analysis.md) in a dictionary built from Wiktionary - the lemma, the part of speech and the [Universal Dependencies](https://universaldependencies.org/) features - and orders them by probability. A token that the dictionary does not have gets the analyses of [rules](analysis.md#rules), and an unknown lowercase word is [guessed](analysis.md#guess). The dictionary is loaded when the analyzer is created.

Exceptions:

- `FileNotFoundError` - the dictionary file does not exist;
- `OSError` - the dictionary file cannot be read: no access, the path points to a directory, a pipe or a device;
- `ValueError` - the file is not an esMorph dictionary, is of another version of the format (a dictionary built by an earlier release is built again) or is damaged (with `verify=True`, a deliberately crafted file too), the path has a NUL character, an [own word](#words) does not follow its sample;
- `TypeError` - a path that is neither a string nor `PathLike`, `verify` that is not `bool`, `words` that is no mapping of strings to strings or pairs of strings.

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

## Parameters { #parameters }

| Parameter | Type | Default | Description |
| :-------- | :--- | :------ | :---------- |
| `path` | `str` / `PathLike` | `None` | Path to the dictionary file; by default, the dictionary of the package |
| `verify` | `bool` | `False` | Check the whole dictionary, for a file of an untrusted source; takes tens of milliseconds |
| `words` | `Mapping[str, str \| tuple[str, str]]` | `None` | [Own words](#words) and their samples |

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

### Analyses of a sentence in context { #analyze_sentence }

`analyze_sentence` · `list[list[Analysis]]`
{ .metric-meta }

``` {.python .signature}
analyze_sentence(words)
```

The analyses of the tokens of a sentence, as [`analyze`](#analyze) gives them, ordered by the context model of the dictionary: the first is the one the model chose by the tokens around, and `prob` is its score in this sentence. A few rules of Spanish then put another analysis first where only it fits, with the scores of the model kept:

- an article before a finite verb is the clitic pronoun (`no la descubras`), though not after a preposition or a determiner (`de la luthiere`);
- a noun right after a verb that takes a bare infinitive is that infinitive (`puede ser`, `parecía pesar`), but not after another verb (`siento pesar`);
- a form of `ser` and `ir` is `ir`:
    - after a reflexive clitic, unless a predicate follows (`se fue`, but `me fue muy difícil`);
    - before `a` (`á` in older writing) and its goal, unless a prepositional or an adverbial phrase or an hour follows (`fue a casa`, but `fue a partir de 1990`, `fue al principio`, `fue a las cinco`);
    - before a gerund (`fue cayendo`);
- a word after a clitic, the negation or `tú` is a verb (`que te vayas`, `no vayas`), though not after a preposition (`punto de no retorno`); after a clitic so is an adverb or an adjective (`se acerca`, `te quedas`);
- a capitalized imperative opening a sentence is no name (`Indique si desea`), unless the model ranks a noun of the dictionary above it (`Sangre de Abel`);
- a word of the dictionary that is mostly a verb with clitics is one (`¡Cállate!`);
- a rare verb gives way to a frequent one with the same form (`aunque vengan` is `venir`, not `vengar`), unless the model chose a participle (`había entrevisto`).

The model and the rules read an older or colloquial spelling as the word it stands for: `fué` as `fue`, `podia` as `podía`, `trabajao` as `trabajado`. A word with a capital letter keeps its spelling, as a name does (`Ramon`), unless it opens the sentence and the dictionary reads it as a word rather than a name (`Fué`).

The tokens are those of one sentence in order, as `analyze` takes them: a contraction (`del`) or a verb with clitics (`dámelo`) is one token. A sentence of one token is analyzed as `analyze`, and one longer than 256 tokens by parts of about 256. The model - a small neural network trained on the treebanks UD Spanish AnCora and GSD - is read on the first call, in a couple of milliseconds; the method releases the GIL.

| Parameter | Type | Default | Description |
| :-------- | :--- | :------ | :---------- |
| `words` | `Iterable[str]` | - | Tokens of the sentence: a list, a tuple, a generator |

Exceptions:

- `TypeError` - a string or bytes instead of a collection of tokens, a token that is not a string;
- `ValueError` - the dictionary has no context model.

=== "Code"

    ``` python
    sentence = morph.analyze_sentence(["Ella", "la", "vio"])
    [(analyses[0].lemma, analyses[0].pos) for analyses in sentence]
    [analysis.pos for analysis in morph.analyze("la")]
    ```

=== "Result"

    ```
    [('él', 'PRON'), ('él', 'PRON'), ('ver', 'VERB')]
    ['DET', 'PRON', 'DET', 'NOUN']
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
annotate(document, *, context=True)
```

Fills the columns `lemma`, `upos` and `feats` of the words of a document of the [anySeg](https://sergeyshk.github.io/anySeg/) segmenter with the most probable analysis of every word in the context of its sentence, as [`analyze_sentence`](#analyze_sentence) orders the analyses of its tokens; with `context=False`, as `analyze` orders them, about nine times faster. The earlier values of these columns are replaced. The words of a multiword token are analyzed by the token: `dime` split into `di` + `me` is the imperative of `decir` with the pronoun `me`, and clitics after a verb are pronouns. A token that the segmenter leaves as one word gets the analysis of the whole token: `devuélvenos` is the imperative of `devolver`. A word without analyses gets its lower case as the lemma and the part of speech `X`. The method releases the GIL. esMorph does not depend on anySeg: the document is an `anyseg.Document` or an object with the same `column("form")`, `multiword_tokens` (a token has `text` and `words`, a word has `index`), `sentences` (a sentence has `words`) and `set_columns`; an object without `sentences` is one sentence, and with `context=False` they are not read.

| Parameter | Type | Default | Description |
| :-------- | :--- | :------ | :---------- |
| `document` | `anyseg.Document` | - | Document of the segmenter (`pip install anyseg`) |
| `context` | `bool` | `True` | Order the analyses by the context model |

Exceptions:

- `ValueError` - the words of the multiword tokens are not consecutive words of the document in order, the first words of the sentences are not words of the document in order, the analysis is in context and the dictionary has no context model;
- `TypeError` - an object that is not a document, a form or the text of a token that is not a string, a token without `text` or `words`, a word without `index` or with an index that is not an integer.

=== "Code"

    ``` python
    from anyseg import Segmenter

    document = Segmenter("es").segment("Dime la verdad.")
    morph.annotate(document)
    [(word.form, word.lemma, word.upos) for word in document.words]
    document = Segmenter("es").segment("Ella la vio.")
    morph.annotate(document, context=False)
    document.words[1].upos
    morph.annotate(document)
    document.words[1].upos
    ```

=== "Result"

    ```
    [('Di', 'decir', 'VERB'), ('me', 'yo', 'PRON'), ('la', 'el', 'DET'), ('verdad', 'verdad', 'NOUN'), ('.', '.', 'PUNCT')]
    'DET'
    'PRON'
    ```

</div>

<div class="metric-card" markdown>

### Word form in the dictionary { #is_known }

`is_known` · `bool`
{ .metric-meta }

``` {.python .signature}
is_known(word)
```

Whether the dictionary has the word form, as written or as a verb of the dictionary with clitics; as with `analyze`, case and lost accents do not matter. Tokens analyzed by rules, guessed words and colloquial spellings that only rules read as words of the dictionary (`trabajao`, [Word analysis](analysis.md#spelling)) are not in the dictionary.

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

The dictionary of the package is built from the English [Wiktionary](https://en.wiktionary.org) of October 2026 with the words and forms that only the Spanish [Wiktionary](https://es.wiktionary.org) of November 2026 has: 717 thousand word forms of 136 thousand lexemes. The Spanish edition adds nouns, adjectives, adverbs and interjections and new forms of the words of the English one, but no names and no new verbs; its word gives way to a colloquial spelling of a word of the English edition (`juera` is `fuera`) and to a verb with clitics (`abrázala` is `abraza` + `la`), and with a capital letter it is mostly a name (`Navas`). Symbols of units (`km`, `kg`) come from a table of their own, and determiners, pronouns, prepositions, conjunctions and numerals in words from a table after the treebank [UD Spanish AnCora](https://github.com/UniversalDependencies/UD_Spanish-AnCora), whose conventions the lemmas and features follow ([Word analysis](analysis.md#ud)). The probabilities of analyses come from the training parts of UD Spanish AnCora and [UD Spanish GSD](https://github.com/UniversalDependencies/UD_Spanish-GSD); word forms they do not have get their order by their ending and the most common features. The dictionary is distributed under the [CC BY-SA 4.0](https://creativecommons.org/licenses/by-sa/4.0/) license.

The property `dictionary_info` describes the dictionary of the analyzer: the source and its main snapshot with its SHA-256, the snapshots of all its sources with theirs (`snapshots`), the license, the release of Universal Dependencies and the treebanks of the probabilities with their licenses, for a dictionary with a context model the release and the treebanks of its training (`context`), the number of word forms and the checksum of the file. Every access returns a new dict; its type for type checkers is `esmorph.DictionaryInfo`, and of `context` - `esmorph.ContextInfo`.

=== "Code"

    ``` python
    info = morph.dictionary_info
    info["source"], info["license"], sorted(info["treebanks"])
    ```

=== "Result"

    ```
    ('Wiktionary', 'CC BY-SA 4.0', ['UD_Spanish-AnCora', 'UD_Spanish-GSD'])
    ```

## Own words { #words }

The parameter `words` adds words that the dictionary does not have. Each word comes with its sample: a word of the dictionary in the same form, usually the lemma.

- The word takes the forms of the lexemes of its sample with their features, order and shares: `{"tiktokear": "bloguear"}` gives `tiktokeo`, `tiktokeó` and `tiktokeando`.
- A sample with lexemes of several parts of speech needs the part of speech, a UD tag in capitals, in a pair: `{"zajo": ("bajo", "NOUN")}`; the pair may be a list, and several parts of speech are separated by commas: `{"tiktokero": ("bloguero", "ADJ,NOUN")}`.
- An own word may have letters that the dictionary has not.
- A word of the dictionary given as an own word keeps its analyses, and the new ones join them.
- The analyses of own words are those of words of the dictionary, with `source` equal to `"dictionary"`, and they are [inflected](inflection.md) like their samples.

=== "Code"

    ``` python
    own = Analyzer(words={"tiktokear": "bloguear", "zajo": ("bajo", "NOUN")})
    [
        (analysis.lemma, analysis.pos)
        for analysis in (own.analyze(word)[0] for word in ["tiktokeó", "zajos"])
    ]
    ```

=== "Result"

    ```
    [('tiktokear', 'VERB'), ('zajo', 'NOUN')]
    ```

A sample that is not in the dictionary, a sample with lexemes of several parts of speech without one, a part of speech that the sample has not (`("bajo", "noun")`), a word without the ending of its sample (`"tiuteo"` as `"bloguear"`) and a word with a space raise `ValueError`. The analyzer shares the dictionary mapped into memory with the analyzers of the same file. Twenty thousand own words are added in under half a second, and the analysis is a few percent slower. Every analyzer keeps the forms of its own words in memory: twenty thousand words of the treebanks take about 50 MB, twenty thousand verbs about 220 MB. Pickle keeps the own words, and a process that unpickles an analyzer with own words or its analyses many times, as a worker of a pool does, makes the analyzer once and keeps the last two such analyzers until the process ends. The [command](cli.md#options) takes own words from a file with `-w`.

## Threads and processes { #parallel }

One analyzer can be used from several threads. `lemmatize`, `analyze_sentence` and `annotate` release the GIL, so threads speed them up. `analyze_many` creates the analyses as Python objects, so threads speed it up little on any Python, the free-threaded one (3.14t, 3.15t) included: for a large text, `lemmatize` or processes are faster. The analyzer and analyses are pickled, so they work with `multiprocessing` and joblib: processes share the dictionary mapped into memory. Pickling an analysis costs more than finding it, so workers that return lemmas or tuples of fields are faster than workers that return analyses. A pickle keeps the absolute path of the dictionary file (none for the dictionary of the package) and the place of the analysis in the tables of that dictionary, which is looked up anew only with another dictionary: unpickling loads each dictionary once per process, so analyses unpickled one by one share one analyzer, an analysis whose dictionary file is gone or of another version of the format takes the dictionary of the package, and a damaged file raises `ValueError`. An analysis that the dictionary does not have, as one pickled with another version of it, keeps its fields, but `forms()` gives an empty list and `inflect` raises `FormNotFoundError`.

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
