# spaCy

The `esmorph` component plugs esMorph into a [spaCy](https://spacy.io/) pipeline: it annotates the tokens of a document with the part of speech (`pos_`), the [Universal Dependencies](https://universaldependencies.org/) features (`morph`) and the lemma (`lemma_`) of the [analysis in context](analyzer.md#analyze_sentence). In a pipeline it replaces the tagger, the morphologizer and the lemmatizer.

``` bash
pip install "esmorph[spacy]"
```

The component registers with spaCy when the package is installed and is added by name, without importing esmorph. spaCy has no prebuilt wheels for Python 3.14t without the GIL.

=== "Code"

    ``` python
    import spacy

    nlp = spacy.blank("es")
    _ = nlp.add_pipe("esmorph")
    doc = nlp("Ella la vio del balcón.")
    [(token.lemma_, token.pos_) for token in doc]
    str(doc[1].morph)
    ```

=== "Result"

    ```
    [('él', 'PRON'), ('él', 'PRON'), ('ver', 'VERB'), ('de', 'ADP'), ('balcón', 'NOUN'), ('.', 'PUNCT')]
    'Case=Acc|Gender=Fem|Number=Sing|Person=3|PrepCase=Npr|PronType=Prs'
    ```

On the gold words of the test parts of the treebanks ([Comparison](comparison.md)) the part of speech, features and lemma of the component are right together for 95.62, 87.50 and 89.50 % of the words of AnCora, PUD and COSER, against 95.92, 83.59 and 85.79 % of `es_core_news_sm`, and a pipeline of the component alone analyzes 90 thousand tokens per second against 45 thousand.

## es_core_news models { #models }

In a pipeline of the `es_core_news_*` models the component replaces the morphologizer and the lemmatizer, while the parser and named entity recognition stay; the component takes the sentences the parser marks and goes before `attribute_ruler`, which fills `tag_` by the part of speech. The morphologizer and the lemmatizer are small layers over the vectors that the parser computes anyway, so the component, more accurate than them, makes such a pipeline about 15 % slower.

``` python
nlp = spacy.load("es_core_news_sm", exclude=["morphologizer", "lemmatizer"])
nlp.add_pipe("esmorph", before="attribute_ruler")
```

## Tokens of several words { #multiword }

A spaCy token is a token of the text: a contraction (`del`, `al`) or a verb with clitics (`dámelo`) is one token. It gets the analysis of its first word: `del` is ADP `de`, `dámelo` is VERB `dar` in the imperative. With the tokenizer of anySeg, which gives the words of a token, it is the most probable analysis with these words, as [`annotate`](analyzer.md#annotate) chooses it, and with another tokenizer the most probable analysis of the token in the sentence. The words of the token are in its [esMorph analysis](#extension).

## Sentences { #sentences }

The context model analyzes a whole sentence. The component takes the sentences from the annotation of the document - the parser, `senter` or `sentencizer`, and without it splits the document after the tokens of sentence end marks («.», «!», «?», «…») with the closing quotes and brackets after them, and at a blank line. Whitespace tokens and tokens of invisible separators only (BOM, U+200B, U+2060, the soft hyphen) get the part of speech `SPACE`, as in spaCy models, and the token itself as the lemma: the model does not see them. A token without an analysis of esMorph gets `X` and the token in lower case as the lemma, as in the [esmorph command](cli.md).

Tokens, their words and sentence starts by the conventions of Universal Dependencies, on which the context model is trained, come from the segmenter [anySeg](https://sergeyshk.github.io/anySeg/) (`pip install "anyseg[spacy]"`): the tokenizer `anyseg.Tokenizer.v1` and the component `anyseg_sentencizer` go before `esmorph`. The words of a token of several are then also in `token._.anyseg_words`.

=== "Code"

    ``` python
    tokenizer = {"@tokenizers": "anyseg.Tokenizer.v1"}
    nlp = spacy.blank("es", config={"nlp": {"tokenizer": tokenizer}})
    _ = nlp.add_pipe("anyseg_sentencizer")
    _ = nlp.add_pipe("esmorph")
    [(token.text, token.lemma_) for token in nlp("Dámelo ya, Sr. García.")]
    ```

=== "Result"

    ```
    [('Dámelo', 'dar'), ('ya', 'ya'), (',', ','), ('Sr.', 'Sr.'), ('García', 'García'), ('.', '.')]
    ```

## Parameters { #config }

| Parameter | Type | Default | Description |
| :-------- | :--- | :------ | :---------- |
| `dictionary` | `str` | `None` | Path to the dictionary file; by default - the dictionary of the package |
| `context` | `bool` | `True` | Choose the analysis by the tokens around; `False` - the most probable analysis of the word form, as with [`analyze`](analyzer.md#analyze) |

`nlp.evaluate` and `spacy evaluate` score the component as the morphologizer and the lemmatizer: `pos_acc`, `morph_acc`, `morph_per_feat` and `lemma_acc`. `spacy package` does not find esmorph among the requirements of a pipeline: add `"requirements": ["esmorph>=0.3,<0.4"]` to its `meta.json` before packaging. The parameter `dictionary` keeps a path, so a dictionary of its own must be at that path where the package is installed.

=== "Code"

    ``` python
    nlp = spacy.blank("es")
    _ = nlp.add_pipe("esmorph", config={"context": False})
    [(token.lemma_, token.pos_) for token in nlp("Ella la vio")]
    ```

=== "Result"

    ```
    [('él', 'PRON'), ('el', 'DET'), ('ver', 'VERB')]
    ```

## esMorph analysis { #extension }

`token._.esmorph` is the esMorph [analysis](analysis.md) with the lemma, the part of speech and the features of the token, by the dictionary of the component that annotated the document: it gives the words of the token and the [forms of the word](inflection.md). Its probability does not take the context into account. If the word form has no such analysis - for example, another component changed the annotation of the token - the value is `None`. A document keeps the path of a dictionary other than the one of the package in `user_data`, so `DocBin(store_user_data=True)` and `span.as_doc(copy_user_data=True)` preserve it, while `Doc.from_docs` does not carry it over.

=== "Code"

    ``` python
    nlp = spacy.blank("es")
    _ = nlp.add_pipe("esmorph")
    analysis = nlp("Dámelo ya")[0]._.esmorph
    analysis.lemma, analysis.words
    analysis.inflect(Mood="Ind", Tense="Pres", Person="1")
    ```

=== "Result"

    ```
    ('dar', ('da', 'me', 'lo'))
    'doy'
    ```
