# Inflection

Every analysis knows its lexeme - the forms of the word with the same lemma and part of speech. `forms()` gives all of them, and `inflect()` gives the form with the given [Universal Dependencies](https://universaldependencies.org/) features. The lexeme follows the [conventions](analyzer.md#order) of the analyses: the auxiliary `haber` and the verb `haber` are two lexemes, and so are a participle and the adjective it makes.

## Forms of the lexeme { #forms }

`forms()` returns the forms as analyses like those of `analyze`, with `prob` equal to 1. The lemma goes first, then the paradigm: for a verb the infinitive, the gerund, the participles and the finite forms by mood (indicative, conditional, subjunctive, imperative), tense, number and person; for a noun or an adjective by number and gender. Variants - the forms of `vos`, old and regional spellings - follow the paradigm. An analysis equals its form from `forms()`: the probability takes no part in the comparison.

=== "Code"

    ``` python
    from esmorph import Analyzer

    morph = Analyzer()
    [form.word for form in morph.analyze("bonitas")[0].forms()]
    [form.word for form in morph.analyze("hablamos")[0].forms()][:12]
    ```

=== "Result"

    ```
    ['bonito', 'bonita', 'bonitos', 'bonitas']
    ['hablar', 'hablando', 'hablado', 'hablada', 'hablados', 'habladas', 'hablo', 'hablas', 'habla', 'hablamos', 'habláis', 'hablan']
    ```

## Form by features { #inflect }

`inflect(**feats)` returns the form with the given features as a string. The features that are not given are kept where possible: `gatos` in the singular is `gato`, in the feminine `gatas`. An infinitive or a gerund asked for a tense, a mood, a person or a number gives a finite form.

=== "Code"

    ``` python
    gatos = morph.analyze("gatos")[0]
    gatos.inflect(Number="Sing"), gatos.inflect(Gender="Fem")
    morph.analyze("hablar")[0].inflect(Tense="Past", Person="3", Number="Plur")
    ```

=== "Result"

    ```
    ('gato', 'gatas')
    'hablaron'
    ```

Several forms may have the same features. A variant comes only from a variant: `hablás` in the imperative is `hablá`, while `hablas` is `habla`. Of the two forms of the imperfect subjunctive the analysis keeps its own: `hablase` in the first person plural is `hablásemos`. Otherwise the more frequent form goes first: the verb `haber` gives `hay`, the auxiliary gives `ha`.

=== "Code"

    ``` python
    [(a.pos, a.inflect(Person="3", Number="Sing")) for a in morph.analyze("haber")[:2]]
    ```

=== "Result"

    ```
    [('AUX', 'ha'), ('VERB', 'hay')]
    ```

A form of a noun, an adjective, a determiner or a pronoun without a gender or a number agrees with any: `grande` in the feminine plural is `grandes`. A verb with [clitics](analyzer.md#clitics) inflects as its verb, and its forms have no clitics: `dámelo` in the plural is `dad`.

=== "Code"

    ``` python
    grande, token = morph.analyze("grande")[0], morph.analyze("dámelo")[0]
    grande.inflect(Gender="Fem", Number="Plur"), token.inflect(Number="Plur")
    ```

=== "Result"

    ```
    ('grandes', 'dad')
    ```

## Errors { #errors }

- `FormNotFoundError`, a subclass of `LookupError` - the lexeme has no form with these features: `casa` has no masculine;
- `ValueError` - the dictionary has no such feature or value, as in `Number="Plural"`; the message lists the valid ones;
- `TypeError` - a value is not a string.

=== "Code"

    ``` python
    from esmorph import FormNotFoundError

    try:
        morph.analyze("casa")[0].inflect(Gender="Masc")
    except FormNotFoundError as error:
        print(error)
    ```

=== "Result"

    ```
    The lexeme of 'casa' has no form Gender=Masc
    ```
