# spaCy

El componente `esmorph` incorpora esMorph a una cadena de [spaCy](https://spacy.io/): anota los tokens de un documento con la categoría gramatical (`pos_`), los rasgos de [Universal Dependencies](https://universaldependencies.org/) (`morph`) y el lema (`lemma_`) del [análisis en contexto](analyzer.md). En la cadena sustituye al etiquetador, al morfologizador y al lematizador.

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

## Tokens de varias palabras { #multiword }

Un token de spaCy es un token del texto: una contracción (`del`, `al`) o un verbo con clíticos (`dámelo`) es un solo token. Recibe el análisis de su primera palabra, tal como [`annotate`](analyzer.md#annotate) se lo da a la primera palabra: `del` es ADP `de`, `dámelo` es VERB `dar` en imperativo. Las palabras del token están en su [análisis de esMorph](#extension).

## Oraciones { #sentences }

El modelo de contexto analiza una oración entera. El componente toma las oraciones de la anotación del documento - el analizador sintáctico, `senter` o `sentencizer` - y, sin ella, divide el documento después de los tokens de signos de fin de oración («.», «!», «?», «…»). Los tokens de espacios y los que solo tienen separadores invisibles (BOM, U+200B, U+2060, el guion suave) reciben la categoría `SPACE`, como en los modelos de spaCy, y el propio token como lema: el modelo no los ve. Un token sin análisis de esMorph recibe `X` y el token en minúsculas como lema, como en el [comando esmorph](cli.md).

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
