# Word analysis

An analysis is an `Analysis` object returned by the `analyze` and `analyze_many` methods of the [analyzer](analyzer.md): the lemma, the part of speech and the [Universal Dependencies](https://universaldependencies.org/) features, the probability, the words of the token and the source of the analysis. Analyses are immutable; they can be compared (the probability takes no part in the comparison, so an analysis equals its form from `forms()`), put into sets and pickled. The forms of a word are given by the `forms` and `inflect` methods - see [Inflection](inflection.md).

=== "Code"

    ``` python
    from esmorph import Analyzer

    morph = Analyzer()
    analysis = morph.analyze("cantaban")[0]
    analysis.word, analysis.lemma, analysis.pos, analysis.feats, analysis.words, analysis.source
    ```

=== "Result"

    ```
    ('cantaban',
     'cantar',
     'VERB',
     {'Mood': 'Ind', 'Number': 'Plur', 'Person': '3', 'Tense': 'Imp', 'VerbForm': 'Fin'},
     ('cantaban',),
     'dictionary')
    ```

## Attributes { #attributes }

| Attribute | Type | Description |
| :-------- | :--- | :---------- |
| `word` | `str` | The word form as the dictionary spells it, with its accents; a verb with [clitics](#clitics) as written, in lower case; a token of the [rules](#rules) as written, without invisible characters, with combining marks joined to their letters and with `-` for the hyphens U+2010 and U+2011 |
| `lemma` | `str` | The lemma |
| `pos` | `str` | UD part of speech: `NOUN`, `VERB`, `ADJ`, `DET`, `ADP` and so on |
| `feats` | `dict[str, str]` | UD features: `{"Gender": "Fem", "Number": "Plur"}`; a new dictionary each time, changing it does not change the analysis |
| `prob` | `float` | Share of the analysis among the analyses of the word; it sums to 1 over all of them |
| `words` | `tuple[str, ...]` | Words of the token: a verb and its clitics, the words of a contraction (`del`, `al`, the older `desta`), or the word itself; the analysis describes the first of them |
| `source` | `str` | Where the analysis comes from: `"dictionary"` - the word form is in the dictionary, `"clitics"` - a verb of the dictionary with [clitics](#clitics), `"rule"` - an [analysis by rule](#rules), `"guess"` - a [guessed analysis](#guess) of an unknown word |

## Parts of speech and features { #ud }

Parts of speech, features and lemmas follow the [Universal Dependencies](https://universaldependencies.org/u/pos/) guidelines and, where they are silent, the conventions of the largest Spanish treebank, [UD Spanish AnCora](https://github.com/UniversalDependencies/UD_Spanish-AnCora), so the output can be compared with spaCy, Stanza and other UD tools:

- determiners, pronouns, prepositions and conjunctions have the lemmas and features of AnCora: `la` is the article `el` (`Definite=Def`, `PronType=Art`) or the pronoun `él`, `se` is `él` with `Reflex=Yes`, `su` is `DET` with `Poss=Yes`, `muy` is a form of `mucho`, and so are their older and spoken spellings, as variants: `usté` is a form of `tú`; a demonstrative pronoun with an accent (`éste`) is a variant of the one without;
- `ser`, `estar`, `haber`, `poder`, `deber`, `soler`, `querer` and `saber` have both `AUX` and `VERB` analyses; `hay` is first of all the verb `haber`;
- a participle is a verb with the infinitive as the lemma (`VerbForm=Part`, `Tense=Past`) and an adjective with the masculine singular as the lemma (`VerbForm=Part` without `Tense`);
- verbs have no `-se` in the lemma: `quejarse` is `quejar` with the clitic `se`;
- the conditional and the imperative have no `Tense`; a proper name has no features, and its lemma has a capital letter: `madrid` and `Madrid` are `Madrid`, `PSOE` stays `PSOE`;
- an adjective in `-ísimo` is its own lemma with `Degree=Abs`.

If the part of speech of a word depends on context, the word has both analyses: `que` is `PRON` and `SCONJ`, `la` is `DET` and `PRON`, a participle is `VERB` and `ADJ`. Features: gender, number, person, tense, mood, verb form, degree, negation (`Polarity=Neg` of `no`), definiteness (`Definite`), the type of a pronoun or a determiner (`PronType`), possessiveness (`Poss`), reflexivity (`Reflex`), the case of personal pronouns (`Case`) and whether they follow a preposition (`PrepCase`), politeness (`Polite=Form` of `usted`), the type of a numeral (`NumType`) and how it is written (`NumForm`), and the kind of a sign of punctuation (`PunctType`, `PunctSide`).

## Order of analyses { #order }

Analyses go from the most probable to the least. `prob` is the share of an analysis among the analyses of the word, by how often the treebanks UD Spanish AnCora and GSD have it; for a word they have not seen, the shares are equal and the order is a guess by the ending of the word and the most common features, and a word goes before its older spelling with other accents: `á` is first of all `a`, `tambien` is `también`.

=== "Code"

    ``` python
    [(a.lemma, a.pos, round(a.prob, 2)) for a in morph.analyze("casa")][:2]
    ```

=== "Result"

    ```
    [('casa', 'NOUN', 0.99), ('casar', 'VERB', 0.01)]
    ```

## Words with clitics { #clitics }

A verb with enclitic pronouns is analyzed as its verb: the analysis describes the infinitive, the gerund or the imperative, and `words` gives the words of the token, as Universal Dependencies has them. In older and literary writing a finite form takes enclitics too, when the accent of the token marks it: `díjole` is `dijo` + `le`. `del` and `al` also have their two words, as do the contractions of `de` with a pronoun or a demonstrative in older writing: `desta` is `de` + `esta`. A word of the dictionary is not split: `comete` is a form of `cometer`, though a few are mostly verbs with clitics: `verse` is first of all `ver` + `se`.

=== "Code"

    ``` python
    [(a.lemma, a.pos, a.words, a.source) for a in morph.analyze("dámelo")]
    ```

=== "Result"

    ```
    [('dar', 'VERB', ('da', 'me', 'lo'), 'clitics')]
    ```

## Spelling { #spelling }

Case does not matter for finding a word in the dictionary, though a token with a capital letter may also be a [name](#rules). A word spelled as in the dictionary gives the analyses of that spelling: `publico` is not `público`. A word absent as written may be a verb with clitics (`verte` is `ver` + `te`, not `verté`); otherwise it is looked for without its accents and with a lost diaeresis, and `word` shows the spelling of the dictionary; for a verb with clitics `word` is the token as written, in lower case.

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

- a sign of punctuation is `PUNCT` with the `PunctType` of its signs if they are of one kind, and `PunctSide` for brackets and question and exclamation marks; a symbol such as `%`, `€` or `²` is `SYM`;
- a number in digits is `NUM` with `NumForm=Digit|NumType=Card`, a year too, also with a sign, a fraction, a power or spaces between thousands (`-5`, `37½`, `10²`, `10 000`); the lemma drops the periods between thousands (`10.000` is `10000`) and a unit after the number (`500cc` is `500`);
- an ordinal number in digits is `ADJ` with `NumForm=Digit|NumType=Ord`, its gender and number, and the masculine singular as the lemma: `2.ª` is `2.º`;
- a Roman numeral is `NUM` with `NumForm=Roman`, less often a name; in lower case it is a numeral only: `xii`;
- a token with a capital letter is a name, `PROPN`, with itself as the lemma: `Josep`, `CiU`, `W.`, `TV-3`;
- a link, an email, a hashtag or a mention in lower case is `SYM`: `www.aeat.es`, `#hola`;
- an abbreviation with a period in lower case that the dictionary does not have is a noun with itself as the lemma (`fig.`), and so is a lowercase letter (`b`).

A lowercase abbreviation with a period that the dictionary has without the period gets the nouns and the symbols of that word, with `source` equal to `"dictionary"`: `kg.` is `kg`. A word of the dictionary with a period of another part of speech gets no analyses: `come.` is a verb with the period of a sentence.

A word with a capital letter that the dictionary knows may be a name too, with its share. An initial (`D.`) and a word without accents that the dictionary has only with them (`Angel`, `Perez`) are mostly names, so their name goes first, while another word with a capital letter is a name less often (`Estados`, `Comisión`), so its name usually goes after the analyses of the dictionary. Of the words in capitals, a word of up to four letters is mostly an acronym (`ETA`, `PP`), so its name usually goes first, while a longer word and a function word - a preposition, a determiner, a pronoun, a conjunction, an auxiliary - are mostly words of a headline (`CAMIONES`, `EL`), so their name usually goes after the analyses of the dictionary. A Roman numeral that the dictionary knows as a word, such as `VI`, `XI` or `X`, is a numeral first. A token with a capital letter that splits as a verb with clitics may be a name too: an imperative without an accent mostly is one (`Mandela` is first of all `PROPN`), while an infinitive, a gerund, an imperative with an accent and a finite form mostly are verbs (`Hacerlo`, `Déjame`, `Levantóse`). A lowercase word is a name rarely: `miró` is first of all `mirar`, `Miró` a name. An analysis of a rule has no other forms: `forms()` gives the analysis itself. `is_known` tells only the words of the dictionary.

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

## Unknown words { #guess }

A lowercase word that neither the dictionary nor the rules know gets guessed analyses, with `source` equal to `"guess"`:

- a word with a hyphen is analyzed by its last part of an open class or a numeral: `palestino-israelíes` is a form of `palestino-israelí`;
- a word with a known prefix is analyzed by the dictionary word without it, of four letters at least after a prefix of three: `supergallos` is a form of `supergallo`, `sobreandan` of `sobreandar`;
- a verb with clitics is analyzed by its guessed gerund, or by its guessed infinitive before `se` or a clitic of the third person: `autogenerarse` is `autogenerar` + `se`;
- any other word is analyzed by the dictionary words with the same ending of up to five letters: their parts of speech, features and inflection, by how many words have them and how often AnCora train has their features; a word in `-ísimo` is an adjective of `Degree=Abs`: `esbeltísima` is a form of `esbeltísimo`.

A guessed word has its forms, guessed too: `forms()` and `inflect()` work as for a word of the dictionary. `is_known` stays false for it.

=== "Code"

    ``` python
    [(a.lemma, a.pos, round(a.prob, 2)) for a in morph.analyze("zumbeábamos")]
    morph.analyze("zumbeábamos")[0].inflect(Tense="Pres", Person="1", Number="Sing")
    ```

=== "Result"

    ```
    [('zumbear', 'VERB', 0.9), ('zumbeábar', 'VERB', 0.09), ('zumbeábar', 'VERB', 0.01)]
    'zumbeo'
    ```
