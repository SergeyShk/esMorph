# Analizador

`Analyzer` busca los análisis de una forma de palabra en un diccionario creado a partir de Wiktionary: el lema, la categoría gramatical y los rasgos de [Universal Dependencies](https://universaldependencies.org/).

=== "Código"

    ``` python
    from esmorph import Analyzer

    morph = Analyzer()
    analysis = morph.analyze("camiones")[0]
    analysis.word, analysis.lemma, analysis.pos, analysis.feats
    ```

=== "Resultado"

    ```
    ('camiones', 'camión', 'NOUN', {'Gender': 'Masc', 'Number': 'Plur'})
    ```

## Orden de los análisis { #order }

Los análisis van del más probable al menos probable. `prob` es la parte de un análisis entre los análisis de la palabra, según la frecuencia con que lo tienen los treebanks UD Spanish AnCora y GSD; para una palabra que no han visto, las partes son iguales y el orden se estima por la terminación de la palabra y los rasgos más comunes.

=== "Código"

    ``` python
    [(a.lemma, a.pos, round(a.prob, 2)) for a in morph.analyze("casa")][:2]
    ```

=== "Resultado"

    ```
    [('casa', 'NOUN', 0.99), ('casar', 'VERB', 0.01)]
    ```

Los lemas y los rasgos siguen las convenciones de estos treebanks: `la` es una forma de `el`, `se` de `él`, `muy` de `mucho`, y los verbos no llevan `-se` en el lema. Un verbo auxiliar tiene análisis `AUX` y `VERB`, y un participio también es un adjetivo.

## Palabras con clíticos { #clitics }

Un verbo con pronombres enclíticos se analiza como su verbo: el análisis describe el infinitivo, el gerundio o el imperativo, y `words` da las palabras del token, como las tiene Universal Dependencies. `del` y `al` también tienen sus dos palabras. Una palabra del diccionario no se divide: `comete` es una forma de `cometer`.

=== "Código"

    ``` python
    [(a.lemma, a.pos, a.words, a.source) for a in morph.analyze("dámelo")]
    ```

=== "Resultado"

    ```
    [('dar', 'VERB', ('da', 'me', 'lo'), 'clitics')]
    ```

## Ortografía { #spelling }

Las mayúsculas no importan. Una palabra escrita como en el diccionario da los análisis de esa grafía: `publico` no es `público`. Una palabra que no está tal como se escribe se busca sin sus tildes y con la diéresis perdida, y `word` muestra la grafía del diccionario; para un verbo con clíticos `word` es el token tal como se escribe, en minúsculas.

=== "Código"

    ``` python
    [morph.analyze(word)[0].word for word in ["CAMIÓN", "camion", "pinguino"]]
    ```

=== "Resultado"

    ```
    ['camión', 'camión', 'pingüino']
    ```

## Métodos { #methods }

| Método | Resultado |
| :-- | :-- |
| `Analyzer(path=None)` | Un analizador con el diccionario del paquete o de un archivo |
| `analyze(word)` | La lista de `Analysis`; vacía para una palabra desconocida |
| `analyze_many(words)` | Lo mismo para una colección de palabras, sin el GIL |
| `lemmatize(words)` | El lema del primer análisis de cada palabra; una palabra desconocida en minúsculas, sin el GIL |
| `is_known(word)` | Si `analyze` encuentra análisis de la palabra |

`Analysis` tiene los campos `word`, `lemma`, `pos`, `feats`, `prob`, `words` y `source`. Es inmutable y se puede usar como clave, y tanto el analizador como el análisis se pueden serializar con pickle.

!!! note "En desarrollo"
    Las palabras desconocidas aún no tienen análisis: llegarán en próximas versiones.
