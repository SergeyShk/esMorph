# spaCy

El componente de spaCy se eliminó en esMorph 0.5.0. Una cadena que añade `esmorph` necesita una versión anterior:

``` bash
pip install "esmorph[spacy]<0.5"
```

Sin spaCy, [`annotate`](analyzer.md#annotate) analiza en contexto un documento del segmentador [anySeg](https://sergeyshk.github.io/anySeg/).
