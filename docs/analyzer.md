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

## Words with clitics { #clitics }

A verb with enclitic pronouns is analyzed as its verb: the analysis describes the infinitive, the gerund or the imperative, and `words` gives the words of the token, as Universal Dependencies has them. `del` and `al` also have their two words. A word of the dictionary is not split: `comete` is a form of `cometer`.

=== "Code"

    ``` python
    [(a.lemma, a.pos, a.words, a.source) for a in morph.analyze("dámelo")]
    ```

=== "Result"

    ```
    [('dar', 'VERB', ('da', 'me', 'lo'), 'clitics')]
    ```

## Spelling { #spelling }

Case does not matter. A word spelled as in the dictionary gives the analyses of that spelling: `publico` is not `público`. A word absent as written is looked for without its accents and with a lost diaeresis, and `word` shows the spelling of the dictionary; for a verb with clitics `word` is the token as written, in lower case.

=== "Code"

    ``` python
    [morph.analyze(word)[0].word for word in ["CAMIÓN", "camion", "pinguino"]]
    ```

=== "Result"

    ```
    ['camión', 'camión', 'pingüino']
    ```

## Tokens out of the dictionary { #rules }

A token that the dictionary does not have gets the analyses of rules, with `source` equal to `"rule"` and `word` as written:

- a sign of punctuation is `PUNCT` with its `PunctType`, and `PunctSide` for paired signs; a symbol such as `%` or `€` is `SYM`;
- a number in digits is `NUM` with `NumForm=Digit|NumType=Card`, a year too; the lemma drops the periods between thousands: `10.000` is `10000`;
- a Roman numeral is `NUM` with `NumForm=Roman`, less often a name;
- a token with a capital letter is a name, `PROPN`, with itself as the lemma: `Josep`, `CiU`, `W.`, `TV-3`.

A word in capitals that the dictionary knows may be a name too: a word of up to four letters gets the name first, as it is mostly an acronym (`ETA`, `PP`), while a longer word and a word of a closed class get it after the analyses of the dictionary, as they are mostly words of a headline (`CAMIONES`, `EL`). An analysis of a rule has no other forms: `forms()` gives the analysis itself. `is_known` tells only the words of the dictionary.

=== "Code"

    ``` python
    [(a.lemma, a.pos, a.feats, a.source) for a in morph.analyze("10.000")]
    [(a.lemma, a.pos, round(a.prob, 2)) for a in morph.analyze("ETA")]
    ```

=== "Result"

    ```
    [('10000', 'NUM', {'NumForm': 'Digit', 'NumType': 'Card'}, 'rule')]
    [('ETA', 'PROPN', 0.82), ('eta', 'NOUN', 0.18)]
    ```

## Methods { #methods }

| Method | Result |
| :-- | :-- |
| `Analyzer(path=None)` | An analyzer with the dictionary of the package or from a file |
| `analyze(word)` | The list of `Analysis`; empty for a lowercase word out of the dictionary |
| `analyze_many(words)` | The same for a collection of words, without the GIL |
| `lemmatize(words)` | The lemma of the first analysis of every word; a word without analyses in lower case, without the GIL |
| `is_known(word)` | Whether the dictionary has the word, as written or as a verb with clitics |

`Analysis` has the fields `word`, `lemma`, `pos`, `feats`, `prob`, `words` and `source` and the methods `forms()` and `inflect()` - see [Inflection](inflection.md). It is immutable and hashable and is compared without the probability. An analyzer and an analysis can be pickled: unpickling takes the analyzer of the same dictionary that the process already has, so analyses unpickled one by one share it, and an analysis whose dictionary file is gone takes the dictionary of the package.

!!! note "Work in progress"
    A lowercase word out of the dictionary has no analyses yet: guessing comes in later versions.
