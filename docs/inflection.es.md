# Flexión

Cada análisis conoce su lexema: las formas de la palabra con el mismo lema y la misma categoría gramatical. `forms()` las da todas, e `inflect()` da la forma con los rasgos de [Universal Dependencies](https://universaldependencies.org/) indicados. El lexema sigue las [convenciones](analysis.md#ud) de los análisis: el auxiliar `haber` y el verbo `haber` son dos lexemas, igual que un participio y el adjetivo que forma.

## Formas del lexema { #forms }

`forms()` devuelve las formas como análisis iguales a los de `analyze`, con `prob` igual a 1. Primero va el lema y después el paradigma: para un verbo, el infinitivo, el gerundio, los participios y las formas personales por modo (indicativo, condicional, subjuntivo, imperativo), tiempo, número y persona; para un sustantivo o un adjetivo, por número y género. Las variantes - las formas de `vos` y las grafías antiguas y regionales - siguen al paradigma. Un análisis es igual a su forma de `forms()`: la probabilidad no participa en la comparación.

=== "Código"

    ``` python
    from esmorph import Analyzer

    morph = Analyzer()
    [form.word for form in morph.analyze("bonitas")[0].forms()]
    [form.word for form in morph.analyze("hablamos")[0].forms()][:12]
    ```

=== "Resultado"

    ```
    ['bonito', 'bonita', 'bonitos', 'bonitas']
    ['hablar', 'hablando', 'hablado', 'hablada', 'hablados', 'habladas', 'hablo', 'hablas', 'habla', 'hablamos', 'habláis', 'hablan']
    ```

## Forma según los rasgos { #inflect }

`inflect(**feats)` devuelve como cadena la forma con los rasgos indicados. Los rasgos no indicados se conservan en lo posible: `gatos` en singular es `gato`, en femenino `gatas`. Un infinitivo o un gerundio al que se le pide un tiempo, un modo, una persona o un número da una forma personal.

=== "Código"

    ``` python
    gatos = morph.analyze("gatos")[0]
    gatos.inflect(Number="Sing"), gatos.inflect(Gender="Fem")
    morph.analyze("hablar")[0].inflect(Tense="Past", Person="3", Number="Plur")
    ```

=== "Resultado"

    ```
    ('gato', 'gatas')
    'hablaron'
    ```

Varias formas pueden tener los mismos rasgos. Una variante solo sale de una variante: `hablás` en imperativo es `hablá`, mientras que `hablas` es `habla`. De las dos formas del pretérito imperfecto de subjuntivo el análisis conserva la suya: `hablase` en primera persona del plural es `hablásemos`. En los demás casos va primero la forma más frecuente: el verbo `haber` da `hay`, el auxiliar da `ha`.

=== "Código"

    ``` python
    [(a.pos, a.inflect(Person="3", Number="Sing")) for a in morph.analyze("haber")[:2]]
    ```

=== "Resultado"

    ```
    [('AUX', 'ha'), ('VERB', 'hay')]
    ```

Una forma de un sustantivo, un adjetivo, un determinante o un pronombre sin género o sin número concuerda con cualquiera: `grande` en femenino plural es `grandes`. Un verbo con [clíticos](analysis.md#clitics) se flexiona como su verbo, y sus formas no llevan clíticos: `dámelo` en plural es `dad`.

=== "Código"

    ``` python
    grande, token = morph.analyze("grande")[0], morph.analyze("dámelo")[0]
    grande.inflect(Gender="Fem", Number="Plur"), token.inflect(Number="Plur")
    ```

=== "Resultado"

    ```
    ('grandes', 'dad')
    ```

## Errores { #errors }

- `FormNotFoundError`, una subclase de `LookupError` - el lexema no tiene una forma con esos rasgos: `casa` no tiene masculino;
- `ValueError` - el diccionario no tiene ese rasgo o ese valor, como en `Number="Plural"`; el mensaje enumera los válidos;
- `TypeError` - un valor no es una cadena.

=== "Código"

    ``` python
    from esmorph import FormNotFoundError

    try:
        morph.analyze("casa")[0].inflect(Gender="Masc")
    except FormNotFoundError as error:
        print(error)
    ```

=== "Resultado"

    ```
    The lexeme of 'casa' has no form Gender=Masc
    ```
