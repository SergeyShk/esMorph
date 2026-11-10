# Analizador

``` {.python .signature}
Analyzer(path=None)
```

Analizador morfológico: para una forma de palabra busca todos sus [análisis](analysis.md) en un diccionario creado a partir de Wiktionary - el lema, la categoría gramatical y los rasgos de [Universal Dependencies](https://universaldependencies.org/) - y los ordena por probabilidad. Un token que el diccionario no tiene recibe los análisis de unas [reglas](analysis.md#rules), y una palabra desconocida en minúsculas se [estima](analysis.md#guess). El diccionario se carga al crear el analizador.

Excepciones:

- `FileNotFoundError` - el archivo del diccionario no existe;
- `OSError` - el archivo del diccionario no se puede leer: sin acceso, la ruta es un directorio;
- `ValueError` - el archivo no es un diccionario de esMorph o está dañado;
- `TypeError` - una ruta que no es una cadena ni `PathLike`.

=== "Código"

    ``` python
    from esmorph import Analyzer

    morph = Analyzer()
    morph.analyze("hablamos")[0]
    ```

=== "Resultado"

    ```
    Analysis(word='hablamos', lemma='hablar', pos='VERB', feats={'Mood': 'Ind', 'Number': 'Plur', 'Person': '1', 'Tense': 'Pres', 'VerbForm': 'Fin'}, prob=0.978261, words=('hablamos',), source='dictionary')
    ```

## Parámetros

| Parámetro | Tipo | Por defecto | Descripción |
| :-------- | :--- | :---------- | :---------- |
| `path` | `str` / `PathLike` | `None` | Ruta al archivo del diccionario; por defecto, el diccionario del paquete |

El objeto guarda la ruta del diccionario en `path`, de tipo `Path` y solo de lectura. El archivo del diccionario se proyecta en memoria, así que los procesos con el mismo archivo comparten su memoria. Mientras exista un analizador, el archivo no debe sobrescribirse en su lugar, pero se puede sustituir por un archivo nuevo; en Windows un archivo abierto no se puede ni sustituir ni borrar. Un diccionario es un dato de confianza, como el código: la carga rechaza un archivo dañado, pero no todo archivo creado a propósito, así que no se deben cargar diccionarios de fuentes no fiables.

## Métodos

<div class="metric-card" markdown>

### Análisis de una forma de palabra { #analyze }

`analyze` · `list[Analysis]`
{ .metric-meta }

``` {.python .signature}
analyze(word)
```

Los análisis de la forma de palabra, primero el más probable; las probabilidades suman 1. Las mayúsculas no importan para buscar la palabra en el diccionario, aunque un token con mayúscula también puede ser un nombre, y una palabra que no está tal como se escribe se busca sin sus tildes y con una diéresis perdida ([ortografía](analysis.md#spelling)). Un verbo con pronombres enclíticos se analiza como su verbo ([clíticos](analysis.md#clitics)), un token que el diccionario no tiene - un signo, un número, un nombre - [por reglas](analysis.md#rules), y una palabra desconocida en minúsculas se [estima](analysis.md#guess). Un token que nada analiza - con un espacio o un carácter de control, una palabra en minúsculas sin modelo como `qzxw` - no tiene análisis.

| Parámetro | Tipo | Por defecto | Descripción |
| :-------- | :--- | :---------- | :---------- |
| `word` | `str` | - | Forma de palabra |

Excepciones:

- `TypeError` - una palabra que no es una cadena.

=== "Código"

    ``` python
    [(a.lemma, a.pos, round(a.prob, 3)) for a in morph.analyze("casa")]
    [(a.lemma, a.source) for a in morph.analyze("zumbeábamos")][:1]
    ```

=== "Resultado"

    ```
    [('casa', 'NOUN', 0.986), ('casar', 'VERB', 0.013), ('casar', 'VERB', 0.0)]
    [('zumbear', 'guess')]
    ```

</div>

<div class="metric-card" markdown>

### Análisis de una lista de formas { #analyze_many }

`analyze_many` · `list[list[Analysis]]`
{ .metric-meta }

``` {.python .signature}
analyze_many(words)
```

Los análisis de cada forma de palabra, como con [`analyze`](#analyze).

| Parámetro | Tipo | Por defecto | Descripción |
| :-------- | :--- | :---------- | :---------- |
| `words` | `Iterable[str]` | - | Formas de palabra: una lista, una tupla, un generador |

Excepciones:

- `TypeError` - una cadena en lugar de una colección de palabras, una palabra que no es una cadena.

=== "Código"

    ``` python
    [len(analyses) for analyses in morph.analyze_many(["el", "gato", "come"])]
    ```

=== "Resultado"

    ```
    [2, 1, 2]
    ```

</div>

<div class="metric-card" markdown>

### Lematización { #lemmatize }

`lemmatize` · `list[str]`
{ .metric-meta }

``` {.python .signature}
lemmatize(words)
```

Los lemas de los análisis más probables de las formas de palabra; un token sin análisis vuelve en minúsculas. Los lemas iguales son un mismo objeto de cadena, así que los lemas de un texto grande ocupan poca memoria; el método libera el GIL.

| Parámetro | Tipo | Por defecto | Descripción |
| :-------- | :--- | :---------- | :---------- |
| `words` | `Iterable[str]` | - | Formas de palabra: una lista, una tupla, un generador |

Excepciones:

- `TypeError` - una cadena en lugar de una colección de palabras, una palabra que no es una cadena.

=== "Código"

    ``` python
    morph.lemmatize(["Los", "niños", "cantaban", "canciones"])
    ```

=== "Resultado"

    ```
    ['el', 'niño', 'cantar', 'canción']
    ```

</div>

<div class="metric-card" markdown>

### Forma de palabra del diccionario { #is_known }

`is_known` · `bool`
{ .metric-meta }

``` {.python .signature}
is_known(word)
```

Si el diccionario tiene la forma de palabra, tal como está escrita o como un verbo del diccionario con clíticos; como en `analyze`, las mayúsculas y las tildes perdidas no importan. Los tokens analizados por reglas y las palabras estimadas no están en el diccionario.

| Parámetro | Tipo | Por defecto | Descripción |
| :-------- | :--- | :---------- | :---------- |
| `word` | `str` | - | Forma de palabra |

Excepciones:

- `TypeError` - una palabra que no es una cadena.

=== "Código"

    ``` python
    morph.is_known("Gatos"), morph.is_known("zumbeábamos")
    ```

=== "Resultado"

    ```
    (True, False)
    ```

</div>

## Diccionario { #dictionary }

El diccionario del paquete se crea a partir del [Wiktionary](https://en.wiktionary.org) en inglés de octubre de 2026: 676 mil formas de palabra de 117 mil lexemas. Los símbolos de unidades (`km`, `kg`) vienen de una tabla propia, y los determinantes, los pronombres, las preposiciones, las conjunciones y los numerales en letras, de una tabla según el treebank [UD Spanish AnCora](https://github.com/UniversalDependencies/UD_Spanish-AnCora), cuyas convenciones siguen los lemas y los rasgos ([Análisis de palabras](analysis.md#ud)). Las probabilidades de los análisis vienen de las partes de entrenamiento de UD Spanish AnCora y [UD Spanish GSD](https://github.com/UniversalDependencies/UD_Spanish-GSD); las formas que estas no tienen se ordenan por su terminación y los rasgos más comunes. El diccionario se distribuye bajo la licencia [CC BY-SA 4.0](https://creativecommons.org/licenses/by-sa/4.0/).

## Hilos y procesos { #parallel }

Un mismo analizador se puede usar desde varios hilos. `lemmatize` libera el GIL, así que los hilos lo aceleran; `analyze_many` crea los análisis con el GIL, así que con él los hilos lo aceleran poco, y en Python 3.14t sin GIL, que tiene su propio paquete precompilado, sí lo aceleran. El analizador y los análisis se serializan con pickle, así que funcionan con `multiprocessing` y joblib: los procesos comparten el diccionario proyectado en memoria. Pickle guarda la ruta del archivo del diccionario (ninguna para el diccionario del paquete): al deserializar, cada diccionario se carga una vez por proceso, así que los análisis deserializados uno a uno comparten un mismo analizador, y un análisis cuyo archivo de diccionario ya no existe toma el diccionario del paquete. Un análisis que el diccionario no tiene, como uno guardado con otra versión del diccionario, conserva sus campos, pero `forms()` da una lista vacía e `inflect` lanza `FormNotFoundError`.

=== "Código"

    ``` python
    from concurrent.futures import ThreadPoolExecutor

    texts = [["los", "niños", "cantaban"], ["el", "gato", "come"]]
    with ThreadPoolExecutor(2) as pool:
        lemmas = list(pool.map(morph.lemmatize, texts))
    lemmas
    ```

=== "Resultado"

    ```
    [['el', 'niño', 'cantar'], ['el', 'gato', 'comer']]
    ```
