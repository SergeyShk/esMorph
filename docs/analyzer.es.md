# Analizador

``` {.python .signature}
Analyzer(path=None, *, verify=False, words=None)
```

Analizador morfológico: para una forma de palabra busca todos sus [análisis](analysis.md) en un diccionario creado a partir de Wiktionary - el lema, la categoría gramatical y los rasgos de [Universal Dependencies](https://universaldependencies.org/) - y los ordena por probabilidad. Un token que el diccionario no tiene recibe los análisis de unas [reglas](analysis.md#rules), y una palabra desconocida en minúsculas se [estima](analysis.md#guess). El diccionario se carga al crear el analizador.

Excepciones:

- `FileNotFoundError` - el archivo del diccionario no existe;
- `OSError` - el archivo del diccionario no se puede leer: sin acceso, la ruta es un directorio, una tubería o un dispositivo;
- `ValueError` - el archivo no es un diccionario de esMorph, es de otra versión del formato (un diccionario construido por una versión anterior se construye de nuevo) o está dañado (con `verify=True`, también un archivo creado a propósito), la ruta tiene un carácter NUL, una [palabra propia](#words) no sigue a su modelo;
- `TypeError` - una ruta que no es una cadena ni `PathLike`, `verify` que no es `bool`, `words` que no es un mapeo de cadenas a cadenas o a pares de cadenas.

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

## Parámetros { #parameters }

| Parámetro | Tipo | Por defecto | Descripción |
| :-------- | :--- | :---------- | :---------- |
| `path` | `str` / `PathLike` | `None` | Ruta al archivo del diccionario; por defecto, el diccionario del paquete |
| `verify` | `bool` | `False` | Comprobar el diccionario entero, para un archivo de una fuente no fiable; tarda decenas de milisegundos |
| `words` | `Mapping[str, str \| tuple[str, str]]` | `None` | [Palabras propias](#words) y sus modelos |

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

### Análisis de una oración en contexto { #analyze_sentence }

`analyze_sentence` · `list[list[Analysis]]`
{ .metric-meta }

``` {.python .signature}
analyze_sentence(words)
```

Los análisis de los tokens de una oración, tal como los da [`analyze`](#analyze), ordenados por el modelo de contexto del diccionario: el primero es el que el modelo eligió por los tokens de alrededor, y `prob` es su puntuación en esta oración. Unas pocas reglas del español ponen después primero otro análisis donde solo él cabe, y las puntuaciones del modelo se conservan:

- un artículo ante un verbo en forma personal es el pronombre clítico (`no la descubras`), aunque no tras una preposición o un determinante (`de la luthiere`);
- un sustantivo justo tras un verbo que rige un infinitivo sin preposición es ese infinitivo (`puede ser`, `parecía pesar`), pero no tras otro verbo (`siento pesar`);
- una forma de `ser` e `ir` es `ir`:
    - tras un clítico reflexivo, salvo que siga un predicado (`se fue`, pero `me fue muy difícil`);
    - ante `a` y su destino, salvo que siga una locución prepositiva o una hora (`fue a casa`, pero `fue a partir de 1990`, `fue a las cinco`);
    - ante un gerundio (`fue cayendo`);
- una palabra tras un clítico, la negación o `tú` es un verbo (`que te vayas`, `no vayas`), aunque no tras una preposición (`punto de no retorno`);
- un imperativo con mayúscula que abre una oración no es un nombre (`Indique si desea`), salvo que el modelo ponga por encima un sustantivo del diccionario (`Sangre de Abel`);
- una palabra del diccionario que suele ser un verbo con clíticos lo es (`¡Cállate!`);
- un verbo raro cede ante uno frecuente con la misma forma (`aunque vengan` es `venir`, no `vengar`), salvo que el modelo haya elegido un participio (`había entrevisto`).

Los tokens son los de una oración en orden, tal como los toma `analyze`: una contracción (`del`) o un verbo con clíticos (`dámelo`) es un solo token. Una oración de un token se analiza como con `analyze`, y una de más de 256 tokens, por partes de unos 256. El modelo - una pequeña red neuronal entrenada con los treebanks UD Spanish AnCora y GSD - se lee en la primera llamada, en un par de milisegundos; el método libera el GIL.

| Parámetro | Tipo | Por defecto | Descripción |
| :-------- | :--- | :---------- | :---------- |
| `words` | `Iterable[str]` | - | Tokens de la oración: una lista, una tupla, un generador |

Excepciones:

- `TypeError` - una cadena o bytes en lugar de una colección de tokens, un token que no es una cadena;
- `ValueError` - el diccionario no tiene modelo de contexto.

=== "Código"

    ``` python
    sentence = morph.analyze_sentence(["Ella", "la", "vio"])
    [(analyses[0].lemma, analyses[0].pos) for analyses in sentence]
    [analysis.pos for analysis in morph.analyze("la")]
    ```

=== "Resultado"

    ```
    [('él', 'PRON'), ('él', 'PRON'), ('ver', 'VERB')]
    ['DET', 'PRON', 'DET', 'NOUN']
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
annotate(document, *, context=True)
```

Rellena las columnas `lemma`, `upos` y `feats` de las palabras de un documento del segmentador [anySeg](https://sergeyshk.github.io/anySeg/) con el análisis más probable de cada palabra en el contexto de su oración, en el orden en que [`analyze_sentence`](#analyze_sentence) ordena los análisis de sus tokens; con `context=False`, en el orden de `analyze`, unas nueve veces más rápido. Los valores anteriores de estas columnas se sustituyen. Las palabras de un token multipalabra se analizan por el token: `dime` dividido en `di` + `me` es el imperativo de `decir` con el pronombre `me`, y los clíticos tras un verbo son pronombres. Un token que el segmentador deja como una sola palabra recibe el análisis del token entero: `devuélvenos` es el imperativo de `devolver`. Una palabra sin análisis recibe como lema su forma en minúsculas y la categoría `X`. El método libera el GIL. esMorph no depende de anySeg: el documento es un `anyseg.Document` o un objeto con los mismos `column("form")`, `multiword_tokens` (un token tiene `text` y `words`, una palabra tiene `index`), `sentences` (una oración tiene `words`) y `set_columns`; un objeto sin `sentences` es una sola oración, y con `context=False` no se leen.

| Parámetro | Tipo | Por defecto | Descripción |
| :-------- | :--- | :---------- | :---------- |
| `document` | `anyseg.Document` | - | Documento del segmentador (`pip install anyseg`) |
| `context` | `bool` | `True` | Ordenar los análisis con el modelo de contexto |

Excepciones:

- `ValueError` - las palabras de los tokens multipalabra no son palabras consecutivas del documento en orden, las primeras palabras de las oraciones no son palabras del documento en orden, el análisis es en contexto y el diccionario no tiene modelo de contexto;
- `TypeError` - un objeto que no es un documento, una forma o el texto de un token que no es una cadena, un token sin `text` o `words`, una palabra sin `index` o con un índice que no es un entero.

=== "Código"

    ``` python
    from anyseg import Segmenter

    document = Segmenter("es").segment("Dime la verdad.")
    morph.annotate(document)
    [(word.form, word.lemma, word.upos) for word in document.words]
    document = Segmenter("es").segment("Ella la vio.")
    morph.annotate(document, context=False)
    document.words[1].upos
    morph.annotate(document)
    document.words[1].upos
    ```

=== "Resultado"

    ```
    [('Di', 'decir', 'VERB'), ('me', 'yo', 'PRON'), ('la', 'el', 'DET'), ('verdad', 'verdad', 'NOUN'), ('.', '.', 'PUNCT')]
    'DET'
    'PRON'
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

El diccionario del paquete se crea a partir del [Wiktionary](https://en.wiktionary.org) en inglés de octubre de 2026 con las palabras y las formas que solo tiene el [Wikcionario](https://es.wiktionary.org) en español de noviembre de 2026: 718 mil formas de palabra de 136 mil lexemas. La edición en español aporta sustantivos, adjetivos, adverbios e interjecciones y formas nuevas de las palabras de la inglesa, pero no nombres propios ni verbos nuevos; su palabra cede ante una grafía coloquial de una palabra de la edición inglesa (`juera` es `fuera`) y ante un verbo con clíticos (`abrázala` es `abraza` + `la`), y con mayúscula es sobre todo un nombre propio (`Navas`). Los símbolos de unidades (`km`, `kg`) vienen de una tabla propia, y los determinantes, los pronombres, las preposiciones, las conjunciones y los numerales en letras, de una tabla según el treebank [UD Spanish AnCora](https://github.com/UniversalDependencies/UD_Spanish-AnCora), cuyas convenciones siguen los lemas y los rasgos ([Análisis de palabras](analysis.md#ud)). Las probabilidades de los análisis vienen de las partes de entrenamiento de UD Spanish AnCora y [UD Spanish GSD](https://github.com/UniversalDependencies/UD_Spanish-GSD); las formas que estas no tienen se ordenan por su terminación y los rasgos más comunes. El diccionario se distribuye bajo la licencia [CC BY-SA 4.0](https://creativecommons.org/licenses/by-sa/4.0/).

La propiedad `dictionary_info` describe el diccionario del analizador: la fuente y su instantánea principal con su SHA-256, las instantáneas de todas sus fuentes con las suyas (`snapshots`), la licencia, la versión de Universal Dependencies y los treebanks de las probabilidades con sus licencias, para un diccionario con modelo de contexto la versión y los treebanks de su entrenamiento (`context`), el número de formas de palabra y la suma de verificación del archivo. Cada acceso devuelve un dict nuevo; su tipo para los verificadores de tipos es `esmorph.DictionaryInfo`, y el de `context`, `esmorph.ContextInfo`.

=== "Código"

    ``` python
    info = morph.dictionary_info
    info["source"], info["license"], sorted(info["treebanks"])
    ```

=== "Resultado"

    ```
    ('Wiktionary', 'CC BY-SA 4.0', ['UD_Spanish-AnCora', 'UD_Spanish-GSD'])
    ```

## Palabras propias { #words }

El parámetro `words` añade palabras que el diccionario no tiene. Cada palabra viene con su modelo: una palabra del diccionario en la misma forma, normalmente el lema.

- La palabra toma las formas de los lexemas de su modelo con sus rasgos, su orden y sus proporciones: `{"tiktokear": "bloguear"}` da `tiktokeo`, `tiktokeó` y `tiktokeando`.
- Un modelo con lexemas de varias categorías necesita la categoría, una etiqueta de UD en mayúsculas, en un par: `{"zajo": ("bajo", "NOUN")}`; el par puede ser una lista, y varias categorías se separan con comas: `{"tiktokero": ("bloguero", "ADJ,NOUN")}`.
- Una palabra propia puede tener letras que el diccionario no tiene.
- Una palabra del diccionario dada como palabra propia conserva sus análisis, y los nuevos se suman a ellos.
- Los análisis de las palabras propias son los de las palabras del diccionario, con `source` igual a `"dictionary"`, y se [flexionan](inflection.md) como sus modelos.

=== "Código"

    ``` python
    own = Analyzer(words={"tiktokear": "bloguear", "zajo": ("bajo", "NOUN")})
    [
        (analysis.lemma, analysis.pos)
        for analysis in (own.analyze(word)[0] for word in ["tiktokeó", "zajos"])
    ]
    ```

=== "Resultado"

    ```
    [('tiktokear', 'VERB'), ('zajo', 'NOUN')]
    ```

Un modelo que no está en el diccionario, un modelo con lexemas de varias categorías sin la categoría, una categoría que el modelo no tiene (`("bajo", "noun")`), una palabra sin la terminación de su modelo (`"tiuteo"` como `"bloguear"`) y una palabra con un espacio lanzan `ValueError`. El analizador comparte el diccionario proyectado en memoria con los analizadores del mismo archivo. Veinte mil palabras propias se añaden en menos de medio segundo, y el análisis es unos pocos por ciento más lento. Cada analizador guarda en memoria las formas de sus palabras propias: veinte mil palabras de los treebanks ocupan unos 50 MB, veinte mil verbos unos 220 MB. Pickle conserva las palabras propias, y un proceso que restaura muchas veces un analizador con palabras propias o sus análisis, como lo hace un proceso de un pool, crea el analizador una sola vez y conserva los dos últimos analizadores así creados hasta que termina el proceso. El [comando](cli.md#options) toma las palabras propias de un archivo con `-w`, y el [componente de spaCy](spacy.md#config) las toma con el parámetro `words`.

## Hilos y procesos { #parallel }

Un mismo analizador se puede usar desde varios hilos. `lemmatize`, `analyze_sentence` y `annotate` liberan el GIL, así que los hilos los aceleran. `analyze_many` crea los análisis como objetos de Python, así que los hilos lo aceleran poco en cualquier Python, también sin GIL (3.14t, 3.15t): para un texto grande, `lemmatize` o los procesos son más rápidos. El analizador y los análisis se serializan con pickle, así que funcionan con `multiprocessing` y joblib: los procesos comparten el diccionario proyectado en memoria. Serializar un análisis cuesta más que encontrarlo, así que los procesos que devuelven lemas o tuplas de campos son más rápidos que los que devuelven análisis. Pickle guarda la ruta absoluta del archivo del diccionario (ninguna para el diccionario del paquete) y el lugar del análisis en las tablas de ese diccionario, que solo se busca de nuevo con otro diccionario: al deserializar, cada diccionario se carga una vez por proceso, así que los análisis deserializados uno a uno comparten un mismo analizador, un análisis cuyo archivo de diccionario ya no existe o es de otra versión del formato toma el diccionario del paquete, y un archivo dañado lanza `ValueError`. Un análisis que el diccionario no tiene, como uno guardado con otra versión del diccionario, conserva sus campos, pero `forms()` da una lista vacía e `inflect` lanza `FormNotFoundError`.

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
