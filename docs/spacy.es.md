# spaCy

El componente `esmorph` incorpora esMorph a una cadena de [spaCy](https://spacy.io/): anota los tokens de un documento con la categoría gramatical (`pos_`), los rasgos de [Universal Dependencies](https://universaldependencies.org/) (`morph`) y el lema (`lemma_`) del [análisis en contexto](analyzer.md#analyze_sentence). En la cadena sustituye al etiquetador, al morfologizador y al lematizador.

``` bash
pip install "esmorph[spacy]"
```

El componente se registra en spaCy al instalar el paquete y se añade por su nombre, sin importar esmorph. spaCy no tiene ruedas precompiladas para Python 3.14t sin GIL.

=== "Código"

    ``` python
    import spacy

    nlp = spacy.blank("es")
    _ = nlp.add_pipe("esmorph")
    doc = nlp("Ella la vio del balcón.")
    [(token.lemma_, token.pos_) for token in doc]
    str(doc[1].morph)
    ```

=== "Resultado"

    ```
    [('él', 'PRON'), ('él', 'PRON'), ('ver', 'VERB'), ('de', 'ADP'), ('balcón', 'NOUN'), ('.', 'PUNCT')]
    'Case=Acc|Gender=Fem|Number=Sing|Person=3|PrepCase=Npr|PronType=Prs'
    ```

Con las palabras de referencia de las partes de prueba de los treebanks ([Comparación](comparison.md)), la categoría, los rasgos y el lema del componente aciertan a la vez para el 95.62, el 87.50 y el 89.50 % de las palabras de AnCora, PUD y COSER, frente al 95.92, el 83.59 y el 85.79 % de `es_core_news_sm`, y una cadena con solo el componente analiza 90 mil tokens por segundo frente a 45 mil.

## Modelos es_core_news { #models }

En una cadena de los modelos `es_core_news_*` el componente sustituye al morfologizador y al lematizador, y se quedan el analizador sintáctico y el reconocimiento de entidades; el componente toma las oraciones que marca el analizador sintáctico y va antes de `attribute_ruler`, que rellena `tag_` según la categoría. El morfologizador y el lematizador son capas pequeñas sobre los vectores que el analizador sintáctico calcula de todos modos, así que el componente, más preciso que ellos, hace una cadena así un 15 % más lenta.

``` python
nlp = spacy.load("es_core_news_sm", exclude=["morphologizer", "lemmatizer"])
nlp.add_pipe("esmorph", before="attribute_ruler")
```

## Tokens de varias palabras { #multiword }

Un token de spaCy es un token del texto: una contracción (`del`, `al`) o un verbo con clíticos (`dámelo`) es un solo token. Recibe el análisis de su primera palabra: `del` es ADP `de`, `dámelo` es VERB `dar` en imperativo. Con el tokenizador de anySeg, que da las palabras de un token, es el análisis más probable con esas palabras, tal como lo elige [`annotate`](analyzer.md#annotate), y con otro tokenizador, el análisis más probable del token en la oración. Las palabras del token están en su [análisis de esMorph](#extension).

## Oraciones { #sentences }

El modelo de contexto analiza una oración entera. El componente toma las oraciones de la anotación del documento - el analizador sintáctico, `senter` o `sentencizer` - y, sin ella, divide el documento después de los tokens de signos de fin de oración («.», «!», «?», «…») con las comillas y los paréntesis de cierre que los siguen, y en una línea en blanco. Los tokens de espacios y los que solo tienen separadores invisibles (BOM, U+200B, U+2060, el guion suave) reciben la categoría `SPACE`, como en los modelos de spaCy, y el propio token como lema: el modelo no los ve. Un token sin análisis de esMorph recibe `X` y el token en minúsculas como lema, como en el [comando esmorph](cli.md).

Los tokens, sus palabras y los inicios de oración según las convenciones de Universal Dependencies, con las que se entrena el modelo de contexto, los da el segmentador [anySeg](https://sergeyshk.github.io/anySeg/) (`pip install "anyseg[spacy]"`): el tokenizador `anyseg.Tokenizer.v1` y el componente `anyseg_sentencizer` van antes de `esmorph`. Las palabras de un token de varias también están entonces en `token._.anyseg_words`.

=== "Código"

    ``` python
    tokenizer = {"@tokenizers": "anyseg.Tokenizer.v1"}
    nlp = spacy.blank("es", config={"nlp": {"tokenizer": tokenizer}})
    _ = nlp.add_pipe("anyseg_sentencizer")
    _ = nlp.add_pipe("esmorph")
    [(token.text, token.lemma_) for token in nlp("Dámelo ya, Sr. García.")]
    ```

=== "Resultado"

    ```
    [('Dámelo', 'dar'), ('ya', 'ya'), (',', ','), ('Sr.', 'Sr.'), ('García', 'García'), ('.', '.')]
    ```

## Parámetros { #config }

| Parámetro | Tipo | Por defecto | Descripción |
| :-------- | :--- | :---------- | :---------- |
| `dictionary` | `str` | `None` | Ruta al archivo del diccionario; por defecto, el diccionario del paquete |
| `context` | `bool` | `True` | Elegir el análisis por los tokens de alrededor; `False` - el análisis más probable de la forma, como con [`analyze`](analyzer.md#analyze) |

`nlp.evaluate` y `spacy evaluate` evalúan el componente como al morfologizador y al lematizador: `pos_acc`, `morph_acc`, `morph_per_feat` y `lemma_acc`. `spacy package` no encuentra esmorph entre los requisitos de una cadena: añada `"requirements": ["esmorph>=0.3,<0.4"]` a su `meta.json` antes de empaquetarla. El parámetro `dictionary` guarda una ruta, así que un diccionario propio debe estar en esa ruta donde se instale el paquete.

=== "Código"

    ``` python
    nlp = spacy.blank("es")
    _ = nlp.add_pipe("esmorph", config={"context": False})
    [(token.lemma_, token.pos_) for token in nlp("Ella la vio")]
    ```

=== "Resultado"

    ```
    [('él', 'PRON'), ('el', 'DET'), ('ver', 'VERB')]
    ```

## Análisis de esMorph { #extension }

`token._.esmorph` es el [análisis](analysis.md) de esMorph con el lema, la categoría y los rasgos del token, según el diccionario del componente que anotó el documento: da las palabras del token y las [formas de la palabra](inflection.md). Su probabilidad no tiene en cuenta el contexto. Si la forma no tiene ese análisis - por ejemplo, otro componente cambió la anotación del token - el valor es `None`. El documento guarda en `user_data` la ruta de un diccionario distinto del del paquete, así que `DocBin(store_user_data=True)` y `span.as_doc(copy_user_data=True)` la conservan, mientras que `Doc.from_docs` no la traslada.

=== "Código"

    ``` python
    nlp = spacy.blank("es")
    _ = nlp.add_pipe("esmorph")
    analysis = nlp("Dámelo ya")[0]._.esmorph
    analysis.lemma, analysis.words
    analysis.inflect(Mood="Ind", Tense="Pres", Person="1")
    ```

=== "Resultado"

    ```
    ('dar', ('da', 'me', 'lo'))
    'doy'
    ```
