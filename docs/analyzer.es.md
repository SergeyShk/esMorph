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

## Ortografía { #spelling }

Las mayúsculas no importan. Una palabra escrita como en el diccionario da los análisis de esa grafía: `publico` no es `público`. Una palabra que no está tal como se escribe se busca sin sus tildes y con la diéresis perdida, y `word` muestra la grafía del diccionario.

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
| `is_known(word)` | Si `analyze` encuentra la palabra en el diccionario |

`Analysis` tiene los campos `word`, `lemma`, `pos`, `feats` y `source`. Es inmutable y se puede usar como clave, y tanto el analizador como el análisis se pueden serializar con pickle.

!!! note "En desarrollo"
    Los análisis aún no tienen un orden significativo, las palabras con clíticos y las desconocidas no tienen análisis, y los lemas de las clases cerradas no siguen las convenciones de los treebanks: esto llegará en las próximas versiones.
