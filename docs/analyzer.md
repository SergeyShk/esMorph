# Analyzer

`Analyzer` finds the analyses of a word form in a dictionary built from Wiktionary: the lemma, the part of speech and the [Universal Dependencies](https://universaldependencies.org/) features.

=== "Code"

    ``` python
    from esmorph import Analyzer

    morph = Analyzer()
    analysis = morph.analyze("camiones")[0]
    analysis.word, analysis.lemma, analysis.pos, analysis.feats
    ```

=== "Result"

    ```
    ('camiones', 'camión', 'NOUN', {'Gender': 'Masc', 'Number': 'Plur'})
    ```

## Order of analyses { #order }

Analyses go from the most probable to the least. `prob` is the share of an analysis among the analyses of the word, by how often the treebanks UD Spanish AnCora and GSD have it; for a word they have not seen, the shares are equal and the order is a guess by the ending of the word and the most common features.

=== "Code"

    ``` python
    [(a.lemma, a.pos, round(a.prob, 2)) for a in morph.analyze("casa")][:2]
    ```

=== "Result"

    ```
    [('casa', 'NOUN', 0.99), ('casar', 'VERB', 0.01)]
    ```

Lemmas and features follow the conventions of these treebanks: `la` is a form of `el`, `se` of `él`, `muy` of `mucho`, and verbs have no `-se` in the lemma. An auxiliary verb has both `AUX` and `VERB` analyses, and a participle is also an adjective.

## Spelling { #spelling }

Case does not matter. A word spelled as in the dictionary gives the analyses of that spelling: `publico` is not `público`. A word absent as written is looked for without its accents and with a lost diaeresis, and `word` shows the spelling of the dictionary.

=== "Code"

    ``` python
    [morph.analyze(word)[0].word for word in ["CAMIÓN", "camion", "pinguino"]]
    ```

=== "Result"

    ```
    ['camión', 'camión', 'pingüino']
    ```

## Methods { #methods }

| Method | Result |
| :-- | :-- |
| `Analyzer(path=None)` | An analyzer with the dictionary of the package or from a file |
| `analyze(word)` | The list of `Analysis`; empty for an unknown word |
| `analyze_many(words)` | The same for a collection of words, without the GIL |
| `lemmatize(words)` | The lemma of the first analysis of every word; an unknown word in lower case, without the GIL |
| `is_known(word)` | Whether `analyze` finds the word in the dictionary |

`Analysis` has the fields `word`, `lemma`, `pos`, `feats`, `prob` and `source`. It is immutable and hashable, and an analyzer and an analysis can be pickled.

!!! note "Work in progress"
    Words with clitics and unknown words have no analyses yet: they come in later versions.
