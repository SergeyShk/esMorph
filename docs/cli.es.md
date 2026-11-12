# El comando esmorph

``` bash
esmorph [opciones] [archivo ...]
```

El comando analiza texto de archivos o de la entrada estándar y escribe los lemas, las categorías y los rasgos de [Universal Dependencies](https://universaldependencies.org/) en uno de cuatro formatos. Se instala con el paquete (`pip install esmorph`) y también se ejecuta como `python -m esmorph`; sin Python, lo instala `cargo install esmorph --features cli`. Cada palabra recibe su análisis más probable en el contexto de su oración, como lo da [`annotate`](analyzer.md#annotate): las palabras de un token multipalabra se analizan por el token. El núcleo en Rust analiza y escribe en un solo hilo, unos 100 mil tokens por segundo, y más de un millón con `--no-context`.

``` bash
echo "Dime la verdad del caso." | esmorph
```

```
decir yo el verdad de el caso .
```

## Opciones { #options }

| Opción | Por defecto | Descripción |
| :----- | :---------- | :---------- |
| `-f`, `--format` | `lemmas` | [Formato de salida](#formats): `lemmas`, `conllu`, `tsv`, `jsonl` |
| `-i`, `--input` | `text` | [Entrada](#input): `text`, `tokens`, `conllu` |
| `-d`, `--dictionary` | el diccionario del paquete | Un archivo de diccionario de esMorph, comprobado entero como con [`verify=True`](analyzer.md#parameters) |
| `--no-context` | | Ordenar los análisis de una palabra como [`analyze`](analyzer.md#analyze), sin el modelo de contexto: más rápido; para un diccionario sin modelo |
| `-o`, `--output` | la salida estándar | El archivo del resultado; `-` es la salida estándar |
| `-h`, `--help` | | Mostrar la ayuda |
| `-V`, `--version` | | Mostrar la versión |

Los archivos se leen uno tras otro; sin archivos y en lugar de `-` se lee la entrada estándar. La entrada está en UTF-8: un archivo en otra codificación (Latin-1, cp1252, UTF-16) no se analiza, salvo que su primera letra con tilde llegue después de los primeros 64 KiB: entonces se analiza el texto anterior a esa línea hasta su última línea en blanco. Una sola línea con bytes no válidos se analiza con U+FFFD en su lugar. El valor de una opción se escribe aparte o junto: `-f conllu`, `-fconllu`, `--format=conllu`. El texto se analiza por párrafos: un párrafo se escribe en cuanto una línea en blanco o el final de la entrada lo cierra - en una terminal de inmediato, y a una tubería o un archivo cada vez que el comando espera la siguiente línea de entrada, así que se puede mantener abierto y darle texto párrafo a párrafo. Las entradas `tokens` y `conllu` se escriben por oraciones.

En Windows PowerShell 5.1 la redirección `>` y una canalización (`Get-Content text.txt | esmorph`) estropean las letras con tilde: pase el archivo como argumento y escriba el resultado con la opción `-o` (en PowerShell 7 también funcionan). El texto pasado con `echo` en cmd llega en la codificación de la consola: ejecute antes `chcp 65001`.

## Entrada { #input }

- `text` - texto: una línea en blanco termina un párrafo. Los tokens, sus palabras y las oraciones los encuentra el segmentador [anySeg](https://sergeyshk.github.io/anySeg/) según las convenciones de Universal Dependencies: `del` y `al` son dos palabras, y también un infinitivo o un gerundio con clíticos (`dárselo`) y los imperativos frecuentes (`dime`). Un token que el segmentador deja como una sola palabra recibe el análisis del token entero: `devuélvenos` es el imperativo de `devolver`. Un párrafo de más de 256 KiB se analiza por partes que terminan en un límite de oración, así que la memoria no crece con la longitud de la entrada; sus oraciones pueden entonces diferir un poco de las del texto entero.
- `tokens` - un token por línea, una línea en blanco separa las oraciones; los separadores alrededor del token y todo lo que sigue a una tabulación se ignoran, así que también sirve una tabla con el token en la primera columna. Un token es una palabra. Una oración sin línea en blanco se cierra tras 8192 tokens: para una lista de palabras el formato `tsv` da una línea por palabra.
- `conllu` - [CoNLL-U](https://universaldependencies.org/format.html): se rellenan las columnas LEMMA, UPOS y FEATS, mientras que las demás columnas, los comentarios, los tokens multipalabra y los nodos vacíos quedan como están. Una oración no válida se escribe tal cual en el formato `conllu` y se omite en los demás, su línea va al mensaje de error, y el resto del archivo se analiza.

``` bash
printf '1\tCasas\t_\t_\t_\t_\t0\troot\t_\t_\n2\tblancas\t_\t_\t_\t_\t1\tamod\t_\t_\n' | esmorph -i conllu -f conllu
```

```
1	Casas	casa	NOUN	_	Gender=Fem|Number=Plur	0	root	_	_
2	blancas	blanco	ADJ	_	Gender=Fem|Number=Plur	1	amod	_	_
```

## Formatos de salida { #formats }

Todos los formatos salvo `jsonl` dan a cada palabra su análisis más probable; `jsonl` escribe todos los análisis de cada token, el más probable primero. Una palabra sin análisis recibe como lema su forma en minúsculas y la categoría `X`.

### lemmas

Los lemas de las palabras de una oración separados por espacios, una línea por oración; con la entrada `text` los párrafos se separan con una línea en blanco.

### conllu

CoNLL-U como lo escribe anySeg: los tokens multipalabra con sus palabras, los comentarios `sent_id` y `text` y, con la entrada `text`, `newdoc` al comienzo de un archivo, `newpar` al comienzo de un párrafo, `SpaceAfter=No` y `SpacesAfter` en MISC. El resultado es el mismo que el de [`annotate`](analyzer.md#annotate) del texto entero, salvo en los párrafos de más de 256 KiB. El análisis no da árbol sintáctico: con las entradas `text` y `tokens` las columnas HEAD y DEPREL son `_`, así que una herramienta que necesita un árbol, como el script de evaluación de la tarea compartida CoNLL 2018, lee el resultado de `-i conllu` sobre un archivo que lo tiene.

``` bash
echo "Dímelo del mar." | esmorph -f conllu
```

```
# newdoc
# newpar
# sent_id = 1
# text = Dímelo del mar.
1-3	Dímelo	_	_	_	_	_	_	_	_
1	Di	decir	VERB	_	Mood=Imp|Number=Sing|Person=2|VerbForm=Fin	_	_	_	_
2	me	yo	PRON	_	Case=Dat|Number=Sing|Person=1|PrepCase=Npr|PronType=Prs	_	_	_	_
3	lo	él	PRON	_	Case=Acc|Gender=Masc|Number=Sing|Person=3|PrepCase=Npr|PronType=Prs	_	_	_	_
4-5	del	_	_	_	_	_	_	_	_
4	de	de	ADP	_	_	_	_	_	_
5	el	el	DET	_	Definite=Def|Gender=Masc|Number=Sing|PronType=Art	_	_	_	_
6	mar	mar	NOUN	_	Gender=Masc|Number=Sing	_	_	_	SpaceAfter=No
7	.	.	PUNCT	_	PunctType=Peri	_	_	_	_
```

### tsv

La palabra, el lema, la categoría, los rasgos y la probabilidad del análisis separados por tabulaciones, sin encabezado; una línea en blanco separa las oraciones. La probabilidad de una palabra de un token multipalabra es su parte entre los análisis que encajan con el token: el verbo `di` de `dime` entre los análisis con estas palabras, el clítico `la` de `verla` entre los de un pronombre.

``` bash
echo "Dime la verdad." | esmorph -f tsv
```

```
Di	decir	VERB	Mood=Imp|Number=Sing|Person=2|VerbForm=Fin	1
me	yo	PRON	Case=Dat|Number=Sing|Person=1|PrepCase=Npr|PronType=Prs	0.45766
la	el	DET	Definite=Def|Gender=Fem|Number=Sing|PronType=Art	0.9774450451099099
verdad	verdad	NOUN	Gender=Fem|Number=Sing	0.999997
.	.	PUNCT	PunctType=Peri	1
```

### jsonl

[JSON Lines](https://jsonlines.org/): una oración por línea - el texto (salvo con la entrada `tokens`) y los tokens con todos sus análisis, como los da [`analyze`](analyzer.md#analyze): la palabra, el lema, la categoría, los rasgos, la probabilidad, las palabras del token y la [fuente del análisis](analysis.md). Abajo está la línea de una oración formateada para leerla.

``` bash
echo "Dime." | esmorph -f jsonl
```

``` json
{
  "text": "Dime.",
  "tokens": [
    {"word": "Dime", "analyses": [
      {"word": "Dime", "lemma": "Dime", "pos": "PROPN", "feats": {},
       "prob": 0.9375, "words": ["Dime"], "source": "rule"},
      {"word": "dime", "lemma": "decir", "pos": "VERB", "feats": {"Mood": "Imp", "Number": "Sing", "Person": "2", "VerbForm": "Fin"},
       "prob": 0.0625, "words": ["di", "me"], "source": "clitics"}
    ]},
    {"word": ".", "analyses": [
      {"word": ".", "lemma": ".", "pos": "PUNCT", "feats": {"PunctType": "Peri"},
       "prob": 1, "words": ["."], "source": "rule"}
    ]}
  ]
}
```

## Códigos de salida { #exit-codes }

| Código | Cuándo |
| :----- | :----- |
| 0 | Éxito, también cuando la salida se cierra antes del final (`esmorph text.txt \| head`) |
| 1 | Un archivo de entrada no existe, no se puede leer o no está en UTF-8, una línea no está en UTF-8 (se analiza con U+FFFD), una oración de CoNLL-U no válida; el resto se analiza igualmente |
| 1 | El diccionario no se puede leer o está dañado, el archivo de salida no se puede crear, el archivo de salida o la salida estándar es un archivo de entrada o el diccionario; no se analiza nada |
| 1 | Un error de escritura, la salida estándar cerrada (el comando instalado con cargo la escribe en /dev/null y termina con 0); el análisis se detiene |
| 2 | Opciones no válidas |

Los mensajes de error van a la salida de error estándar.
