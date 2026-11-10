# Exceptions

The library raises built-in Python exceptions and one of its own, `FormNotFoundError`, a subclass of `LookupError`. It is available from `esmorph`.

| Exception | Built-in class | When it is raised |
| :-------- | :------------- | :---------------- |
| `FormNotFoundError` | `LookupError` | The lexeme has no form with the requested features, or the dictionary has no lexeme of the analysis ([`inflect`](inflection.md#inflect)) |
| `ValueError` | - | The feature or the value is not in the dictionary (`inflect`); the file is not an esMorph dictionary or is damaged, the path has a NUL character ([`Analyzer`](analyzer.md)) |
| `TypeError` | - | An argument of the wrong type: a word or a feature value that is not a string, a string or bytes instead of a collection of words ([`analyze_many`](analyzer.md#analyze_many), [`lemmatize`](analyzer.md#lemmatize)), a dictionary path that is neither a string nor `PathLike` (`Analyzer`) |
| `FileNotFoundError` | `OSError` | The dictionary file does not exist (`Analyzer`) |
| `PermissionError`, `IsADirectoryError` and others | `OSError` | The dictionary file cannot be read or is not a file (`Analyzer`) |

Exception messages are in English.

=== "Code"

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

=== "Result"

    ```
    FormNotFoundError True
    Unknown value of the feature Number: Plural; the dictionary has Plur, Sing
    ```
