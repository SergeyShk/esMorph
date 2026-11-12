# Excepciones

La biblioteca lanza excepciones integradas de Python y una propia, `FormNotFoundError`, subclase de `LookupError`. Está disponible en `esmorph`.

| Excepción | Clase integrada | Cuándo se lanza |
| :-------- | :-------------- | :-------------- |
| `FormNotFoundError` | `LookupError` | El lexema no tiene una forma con los rasgos pedidos, o el diccionario no tiene el lexema del análisis ([`inflect`](inflection.md#inflect)) |
| `ValueError` | - | El rasgo o el valor no está en el diccionario (`inflect`); el archivo no es un diccionario de esMorph o está dañado, con `verify=True` también un archivo creado a propósito, la ruta tiene un carácter NUL ([`Analyzer`](analyzer.md)); las palabras de los tokens multipalabra no son palabras consecutivas del documento en orden ([`annotate`](analyzer.md#annotate)) |
| `TypeError` | - | Un argumento de tipo incorrecto: una palabra o un valor de rasgo que no es una cadena, una cadena o bytes en lugar de una colección de palabras ([`analyze_many`](analyzer.md#analyze_many), [`lemmatize`](analyzer.md#lemmatize)), una ruta del diccionario que no es una cadena ni `PathLike`, `verify` que no es `bool` (`Analyzer`); un objeto que no es un documento, una forma o el texto de un token que no es una cadena, el índice de una palabra que no es un entero (`annotate`) |
| `FileNotFoundError` | `OSError` | El archivo del diccionario no existe (`Analyzer`) |
| `PermissionError`, `IsADirectoryError` y otras | `OSError` | El archivo del diccionario no se puede leer o no es un archivo (`Analyzer`) |

Los mensajes de las excepciones están en inglés.

=== "Código"

    ``` python
    from esmorph import Analyzer, FormNotFoundError

    morph = Analyzer()
    try:
        morph.analyze("casa")[0].inflect(Gender="Masc")
    except FormNotFoundError as e:
        print(type(e).__name__, isinstance(e, LookupError))
    try:
        morph.analyze("gato")[0].inflect(Number="Plural")
    except ValueError as e:
        print(e)
    ```

=== "Resultado"

    ```
    FormNotFoundError True
    Unknown value of the feature Number: Plural; the dictionary has Plur, Sing
    ```
