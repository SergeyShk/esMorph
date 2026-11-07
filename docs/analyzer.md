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

`Analysis` has the fields `word`, `lemma`, `pos`, `feats` and `source`. It is immutable and hashable, and an analyzer and an analysis can be pickled.

!!! note "Work in progress"
    Analyses do not have a meaningful order yet, words with clitics and unknown words have no analyses, and the lemmas of closed classes do not follow the conventions of treebanks: this comes in the next versions.
