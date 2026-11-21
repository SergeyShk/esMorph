# spaCy

The spaCy component was removed in esMorph 0.5.0. A pipeline that adds `esmorph` needs an earlier version:

``` bash
pip install "esmorph[spacy]<0.5"
```

Without spaCy, [`annotate`](analyzer.md#annotate) analyzes a document of the [anySeg](https://sergeyshk.github.io/anySeg/) segmenter in context.
