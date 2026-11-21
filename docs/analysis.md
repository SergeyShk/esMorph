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
| `word` | `str` | The word form as the dictionary spells it, with its accents; for a [colloquial spelling](#spelling) its standard spelling (`trabajado` of `trabajao`); a verb with [clitics](#clitics) as written, in lower case; a token of the [rules](#rules) as written, without invisible characters, with combining marks joined to their letters and with `-` for the hyphens U+2010 and U+2011 |
| `lemma` | `str` | The lemma |
| `pos` | `str` | UD part of speech: `NOUN`, `VERB`, `ADJ`, `DET`, `ADP` and so on |
| `feats` | `dict[str, str]` | UD features: `{"Gender": "Fem", "Number": "Plur"}`; a new dictionary each time, changing it does not change the analysis |
| `prob` | `float` | Share of the analysis among the analyses of the word; it sums to 1 over all of them. From [`analyze_sentence`](analyzer.md#analyze_sentence) - the score of the context model in the sentence |
| `words` | `tuple[str, ...]` | Words of the token: a verb and its clitics, the words of a contraction (`del`, `al`, the older `desta`), a number and its sign or unit (`60%`), a unit and its number (`km.24`), or the word itself; the analysis describes the first of them |
| `source` | `str` | Where the analysis comes from: `"dictionary"` - the word form is in the dictionary, `"clitics"` - a verb of the dictionary with [clitics](#clitics), `"rule"` - an [analysis by rule](#rules), `"guess"` - a [guessed analysis](#guess) of an unknown word |
| `diminutive_of` | `str \| None` | The word of which the lemma is a [diminutive](#diminutives): `casa` for `casitas`; `None` for other words |

## Parts of speech and features { #ud }

Parts of speech, features and lemmas follow the [Universal Dependencies](https://universaldependencies.org/u/pos/) guidelines and, where they are silent, the conventions of the largest Spanish treebank, [UD Spanish AnCora](https://github.com/UniversalDependencies/UD_Spanish-AnCora), so the output can be compared with spaCy, Stanza and other UD tools:

- determiners, pronouns, prepositions and conjunctions have the lemmas and features of AnCora: `la` is the article `el` (`Definite=Def`, `PronType=Art`) or the pronoun `él`, `se` is `él` with `Reflex=Yes`, `su` is `DET` with `Poss=Yes`, `muy` is a form of `mucho`, and so are their older and spoken spellings, as variants: `usté` is a form of `tú`; a demonstrative pronoun with an accent (`éste`) is a variant of the one without;
- `ser`, `estar`, `haber`, `poder`, `deber`, `soler`, `querer` and `saber` have both `AUX` and `VERB` analyses; `hay` is first of all the verb `haber`;
- a participle is a verb with the infinitive as the lemma (`VerbForm=Part`, `Tense=Past`) and an adjective with the masculine singular as the lemma (`VerbForm=Part` without `Tense`);
- verbs have no `-se` in the lemma: `quejarse` is `quejar` with the clitic `se`;
- older and nonstandard forms of a verb are its variants: `trujo` is `traer`, `vido` is `ver`, `habemos` is `haber`;
- the conditional and the imperative have no `Tense`; a proper name has no features, and its lemma has a capital letter: `madrid` and `Madrid` are `Madrid`, `PSOE` stays `PSOE`;
- an adjective in `-ísimo` is its own lemma with `Degree=Abs`.

If the part of speech of a word depends on context, the word has both analyses: `que` is `PRON` and `SCONJ`, `la` is `DET` and `PRON`, a participle is `VERB` and `ADJ`. Features: gender, number, person, tense, mood, verb form, degree, negation (`Polarity=Neg` of `no`), definiteness (`Definite`), the type of a pronoun or a determiner (`PronType`), possessiveness (`Poss`), reflexivity (`Reflex`), the case of personal pronouns (`Case`) and whether they follow a preposition (`PrepCase`), politeness (`Polite=Form` of `usted`), the type of a numeral (`NumType`) and how it is written (`NumForm`), and the kind of a sign of punctuation (`PunctType`, `PunctSide`).

## Order of analyses { #order }

Analyses go from the most probable to the least. `prob` is the share of an analysis among the analyses of the word, by how often the treebanks UD Spanish AnCora and GSD have it; of the verbs of a form the one AnCora has more often as a verb in all its forms has the larger share where the treebanks have the form itself rarely: `vengo` is first of all `venir`, not `vengar`, while `sienta`, which they have as `sentar` more often, is first of all `sentar`. For a word they have not seen, the shares are equal and the order is a guess by the ending of the word and the most common features; a form of several verbs they have seen in other forms gets shares by the frequency of the verbs and of the features instead (`vengamos` is first of all the subjunctive of `venir`). A word goes before its older spelling with other accents: `á` is first of all `a`, `tambien` is `también`. [`analyze_sentence`](analyzer.md#analyze_sentence) orders the same analyses by the sentence.

=== "Code"

    ``` python
    [(a.lemma, a.pos, round(a.prob, 2)) for a in morph.analyze("casa")][:2]
    ```

=== "Result"

    ```
    [('casa', 'NOUN', 0.99), ('casar', 'VERB', 0.01)]
    ```

## Diminutives { #diminutives }

A diminutive keeps its own lemma, as in the treebanks: `casitas` is a form of `casita`. `diminutive_of` gives the word it is a diminutive of, as Wiktionary gives it in the first sense of the word: `casita` is a diminutive of `casa`, `poquito` of `poco`, `ahorita` of `ahora`. A word that is mostly a word of its own, though Wiktionary also has it as a diminutive, has none: `señorita`, `granito`. About two thousand nouns, adjectives and adverbs of the dictionary have it.

=== "Code"

    ``` python
    [
        morph.analyze(word)[0].diminutive_of
        for word in ("casitas", "poquito", "ahorita", "señorita", "granito")
    ]
    ```

=== "Result"

    ```
    ['casa', 'poco', 'ahora', None, None]
    ```

## Words with clitics { #clitics }

A verb with enclitic pronouns is analyzed as its verb: the analysis describes the infinitive, the gerund or the imperative, and `words` gives the words of the token, as Universal Dependencies has them. In older and literary writing a finite form takes enclitics too, when the accent of the token marks it: `díjole` is `dijo` + `le`, `quedéme` is `quedé` + `me` (the imperative would be `quédeme`). Since 2010 such a token follows the general rules of accents, so a form stressed on its last syllable loses its accent: a lowercase word that is no word of the dictionary, no other verb with clitics and is such a form with one enclitic is first of all that verb, with guesses of the word after it (`quedose` is `quedó` + `se`, `fuime` is `fui` + `me`), and [`is_known`](analyzer.md#is_known) does not tell it. The enclitic is no `te` or `os`, nor `nos` after the first person, which end nouns more often: `anisete` and `macarenos` are nouns. A token that is also an imperative that would take an accent with the enclitic is first of all such a form too: `acerqueme` is `acerqué` + `me`, and then the imperative `acerque` + `me`, which the spelling writes `acérqueme`. `del` and `al` also have their two words, as do the contractions of `de` with a pronoun or a demonstrative in older writing: `desta` is `de` + `esta`. A word of the dictionary is not split: `comete` is a form of `cometer`, though a few are mostly verbs with clitics: `verse` is first of all `ver` + `se`, `cállate` is `calla` + `te`, and in a sentence they are always such verbs.

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

A lowercase word that the dictionary has not, even without its accents, may be a colloquial or an older spelling of a word of the dictionary. Such a word gets the analyses of that word, and `word` is its standard spelling. The spellings are:

- a participle without its `d`: `trabajao` is `trabajado`, `sío` is `sido`; its accent is not lost, so `perdío` is `perdido`, not `perdió`;
- a final `-á`, `-é` or `-ú` for `-ad`, `-ed` or `-ud`: `caridá` is `caridad`, `virtú` is `virtud`;
- an aspirated `f` or `h`: `juerza` is `fuerza`, `juevo` is `huevo`, `juyendo` is `huyendo`;
- `es` for `ex` before a consonant, as older writing has it: `estranjero` is `extranjero`, `esplicar` is `explicar`.

A word with a capital letter is no such spelling: `Trabajao` and `Julita` are names, not `trabajado` and `fulita`.

The dictionary also has spellings of words as they sound and common shortened forms: `na` and `naa` are `nada`, `pos` is `pues`, `to` is `todo`, `mu` is `muy`, `güeno` is `bueno`, `er` is `el`. They have the analyses of the standard word with its lemma and features, and `word` is the standard spelling; the context chooses among them and the other words of the same spelling (`mu` is also an interjection). As words of the dictionary, they are found in any case: `Güeno` is `bueno` too. The analyses of all these spellings have `source` equal to `"dictionary"`, while [`is_known`](analyzer.md#is_known) tells only the spellings of the dictionary: `na` is known, `trabajao` is not.

=== "Code"

    ``` python
    [
        (analysis.word, analysis.lemma)
        for analysis in (
            morph.analyze(word)[0] for word in ["trabajao", "perdío", "caridá", "juerza", "na", "er"]
        )
    ]
    ```

=== "Result"

    ```
    [('trabajado', 'trabajar'), ('perdido', 'perder'), ('caridad', 'caridad'), ('fuerza', 'fuerza'), ('nada', 'nada'), ('el', 'el')]
    ```

## Tokens out of the dictionary { #rules }

A token that the dictionary does not have gets the analyses of rules, with `source` equal to `"rule"` and `word` as written:

- a sign of punctuation is `PUNCT` with the `PunctType` of its signs if they are of one kind, and `PunctSide` for brackets and question and exclamation marks; a symbol such as `%`, `€` or `²` is `SYM`;
- a number in digits is `NUM` with `NumForm=Digit|NumType=Card`, a year too, also with a sign, a fraction, a power or spaces between thousands (`-5`, `37½`, `10²`, `10 000`); the lemma drops the periods between thousands (`10.000` is `10000`); a number with a percent sign, a degree sign or a unit is two words, and the analysis is that of the number (`60%` is `60` and `%`, `500cc` is `500` and `cc`, `-5ºC` is `-5` and `ºC`); a unit of one letter is one only as written (`3G` is a name), a longer one in any case (`5Kg`, `8GB`);
- an ordinal number in digits is `ADJ` with `NumForm=Digit|NumType=Ord`, its gender and number, and the masculine singular as the lemma: `2.ª` is `2.º`;
- a Roman numeral is `NUM` with `NumForm=Roman`, less often a name; in lower case it is a numeral only: `xii`;
- a token with a capital letter is a name, `PROPN`, with itself as the lemma: `Josep`, `CiU`, `W.`, `TV-3`;
- a link, an email, a hashtag or a mention in lower case is `SYM`: `www.aeat.es`, `#hola`;
- an abbreviation with a period in lower case that the dictionary does not have is a noun with itself as the lemma (`fig.`), and so is a lowercase letter (`b`).

A lowercase abbreviation with a period that the dictionary has without the period gets the nouns and the symbols of that word, with `source` equal to `"dictionary"`: `kg.` is `kg`. A unit with its period and a number in one token is two words with these analyses: `km.24` is `km` with the words `km.` and `24`, and only its nouns and symbols count (`m.36` is no adjective). A word of the dictionary with a period of another part of speech gets no analyses: `come.` is a verb with the period of a sentence.

A word with a capital letter that the dictionary knows may be a name too, with its share. An initial (`D.`) and a word without accents that the dictionary has only with them (`Angel`, `Perez`) are mostly names, so their name goes first, while another word with a capital letter is a name less often (`Estados`, `Comisión`), so its name usually goes after the analyses of the dictionary. Of the words in capitals, a word of up to four letters is mostly an acronym (`ETA`, `PP`), so its name usually goes first, while a longer word and a function word - a preposition, a determiner, a pronoun, a conjunction, an auxiliary - are mostly words of a headline (`CAMIONES`, `EL`), so their name usually goes after the analyses of the dictionary. A Roman numeral that the dictionary knows as a word, such as `VI`, `XI` or `X`, is a numeral first. A token with a capital letter that splits as a verb with clitics may be a name too: an imperative without an accent mostly is one (`Mandela` is first of all `PROPN`), and so is a finite form without an accent (`Vila`) but one with `se`, `me` or `le` (`Vase`, `Quedose`), while an infinitive, a gerund, an imperative with an accent and a finite form with an accent mostly are verbs (`Hacerlo`, `Déjame`, `Levantóse`). A lowercase word is a name rarely: `miró` is first of all `mirar`, `Miró` a name. An analysis of a rule has no other forms: `forms()` gives the analysis itself. `is_known` tells only the words of the dictionary.

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
