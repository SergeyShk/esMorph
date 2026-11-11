# Analizador

``` {.python .signature}
Analyzer(path=None, *, verify=False)
```

Analizador morfológico: para una forma de palabra busca todos sus [análisis](analysis.md) en un diccionario creado a partir de Wiktionary - el lema, la categoría gramatical y los rasgos de [Universal Dependencies](https://universaldependencies.org/) - y los ordena por probabilidad. Un token que el diccionario no tiene recibe los análisis de unas [reglas](analysis.md#rules), y una palabra desconocida en minúsculas se [estima](analysis.md#guess). El diccionario se carga al crear el analizador.

Excepciones:

- `FileNotFoundError` - el archivo del diccionario no existe;
- `OSError` - el archivo del diccionario no se puede leer: sin acceso, la ruta es un directorio, una tubería o un dispositivo;
- `ValueError` - el archivo no es un diccionario de esMorph o está dañado (con `verify=True`, también un archivo creado a propósito), la ruta tiene un carácter NUL;
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
| `verify` | `bool` | `False` | Comprobar el diccionario entero, para un archivo de una fuente no fiable; tarda decenas de milisegundos |

El objeto guarda la ruta absoluta del diccionario en `path`, de tipo `Path` y solo de lectura. El archivo del diccionario se proyecta en memoria, así que los procesos con el mismo archivo comparten su memoria. Mientras exista un analizador, el archivo no debe sobrescribirse en su lugar, pero se puede sustituir por un archivo nuevo; en Windows un archivo abierto no se puede ni sustituir ni borrar. Un diccionario es un dato de confianza, como el código: la carga rechaza un archivo dañado, pero no todo archivo creado a propósito, que puede hacer fallar el análisis. Un diccionario de una fuente no fiable se carga con `verify=True`: entonces un archivo creado a propósito se rechaza con `ValueError`, y pickle conserva la comprobación.

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

- `TypeError` - una cadena o bytes en lugar de una colección de palabras, una palabra que no es una cadena.

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

- `TypeError` - una cadena o bytes en lugar de una colección de palabras, una palabra que no es una cadena.

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

### Análisis de un documento { #annotate }

`annotate` · `None`
{ .metric-meta }

``` {.python .signature}
annotate(document)
```

Rellena las columnas `lemma`, `upos` y `feats` de las palabras de un documento del segmentador [anySeg](https://sergeyshk.github.io/anySeg/) con el análisis más probable de cada palabra, en el orden de `analyze`; los valores anteriores de estas columnas se sustituyen. Las palabras de un token multipalabra se analizan por el token: `dime` dividido en `di` + `me` es el imperativo de `decir` con el pronombre `me`, y los clíticos tras un verbo son pronombres. Un token que el segmentador deja como una sola palabra recibe el análisis del token entero: `devuélvenos` es el imperativo de `devolver`. Una palabra sin análisis recibe como lema su forma en minúsculas y la categoría `X`. El método libera el GIL. esMorph no depende de anySeg: el documento es un `anyseg.Document` o un objeto con los mismos `column("form")`, `multiword_tokens` (un token tiene `text` y `words`, una palabra tiene `index`) y `set_columns`.

| Parámetro | Tipo | Por defecto | Descripción |
| :-------- | :--- | :---------- | :---------- |
| `document` | `anyseg.Document` | - | Documento del segmentador (`pip install anyseg`) |

Excepciones:

- `ValueError` - las palabras de los tokens multipalabra no son palabras consecutivas del documento en orden;
- `TypeError` - una forma o el texto de un token que no es una cadena, el índice de una palabra que no es un entero.

=== "Código"

    ``` python
    from anyseg import Segmenter

    document = Segmenter("es").segment("Dime la verdad.")
    morph.annotate(document)
    [(word.form, word.lemma, word.upos) for word in document.words]
    ```

=== "Resultado"

    ```
    [('Di', 'decir', 'VERB'), ('me', 'yo', 'PRON'), ('la', 'el', 'DET'), ('verdad', 'verdad', 'NOUN'), ('.', '.', 'PUNCT')]
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

La propiedad `dictionary_info` describe el diccionario del analizador: la fuente y su instantánea con su SHA-256, la licencia, la versión de Universal Dependencies y los treebanks de las probabilidades con sus licencias, el número de formas de palabra y la suma de verificación del archivo. El diccionario es nuevo en cada llamada.

=== "Código"

    ``` python
    info = morph.dictionary_info
    info["source"], info["license"], sorted(info["treebanks"])
    ```

=== "Resultado"

    ```
    ('Wiktionary', 'CC BY-SA 4.0', ['UD_Spanish-AnCora', 'UD_Spanish-GSD'])
    ```

## Hilos y procesos { #parallel }

Un mismo analizador se puede usar desde varios hilos. `lemmatize` y `annotate` liberan el GIL, así que los hilos los aceleran. `analyze_many` crea los análisis como objetos de Python, así que los hilos lo aceleran poco en cualquier Python, también en 3.14t sin GIL: para un texto grande, `lemmatize` o los procesos son más rápidos. El analizador y los análisis se serializan con pickle, así que funcionan con `multiprocessing` y joblib: los procesos comparten el diccionario proyectado en memoria. Serializar un análisis cuesta más que encontrarlo, así que los procesos que devuelven lemas o tuplas de campos son más rápidos que los que devuelven análisis. Pickle guarda la ruta absoluta del archivo del diccionario (ninguna para el diccionario del paquete) y el lugar del análisis en las tablas de ese diccionario, que solo se busca de nuevo con otro diccionario: al deserializar, cada diccionario se carga una vez por proceso, así que los análisis deserializados uno a uno comparten un mismo analizador, un análisis cuyo archivo de diccionario ya no existe toma el diccionario del paquete, y un archivo dañado lanza `ValueError`. Un análisis que el diccionario no tiene, como uno guardado con otra versión del diccionario, conserva sus campos, pero `forms()` da una lista vacía e `inflect` lanza `FormNotFoundError`.

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
