# The esmorph command

``` bash
esmorph [options] [file ...]
```

The command analyzes text from files or the standard input and writes lemmas, parts of speech and [Universal Dependencies](https://universaldependencies.org/) features in one of four formats. It is installed with the package (`pip install esmorph`) and also runs as `python -m esmorph`; without Python, `cargo install esmorph --features cli` installs it. Every word gets its most probable analysis, as [`annotate`](analyzer.md#annotate) gives it: the words of a multiword token are analyzed by the token. The Rust core analyzes and writes in one thread, about a million tokens per second.

``` bash
echo "Dime la verdad del caso." | esmorph
```

```
decir yo el verdad de el caso .
```

## Options { #options }

| Option | Default | Description |
| :----- | :------ | :---------- |
| `-f`, `--format` | `lemmas` | [Output format](#formats): `lemmas`, `conllu`, `tsv`, `jsonl` |
| `-i`, `--input` | `text` | [Input](#input): `text`, `tokens`, `conllu` |
| `-d`, `--dictionary` | the package dictionary | An esMorph dictionary file |
| `-o`, `--output` | the standard output | The result file; `-` is the standard output |
| `-h`, `--help` | | Show the help |
| `-V`, `--version` | | Show the version |

Files are read one after another; without files and in place of `-` the standard input is read. The input is in UTF-8: a file in another encoding (Latin-1, cp1252, UTF-16) is not analyzed, and a single line with invalid bytes is analyzed with U+FFFD in their place. An option value is written separately or attached: `-f conllu`, `-fconllu`, `--format=conllu`. Text is analyzed by paragraphs: a paragraph is written as soon as a blank line or the end of the input closes it - on a terminal at once, and to a pipe or a file whenever the command waits for the next input line, so it can be kept open and fed text paragraph by paragraph. The inputs `tokens` and `conllu` are written by sentences.

In Windows PowerShell 5.1 the `>` redirection and a pipeline (`Get-Content text.txt | esmorph`) corrupt letters with accents: pass the file as an argument and write the result with the `-o` option (in PowerShell 7 they work too). Text passed with `echo` in cmd comes in the console encoding: run `chcp 65001` first.

## Input { #input }

- `text` - text: a blank line ends a paragraph. Tokens, their words and sentences are found by the segmenter [anySeg](https://sergeyshk.github.io/anySeg/) following the conventions of Universal Dependencies: `del` and `al` are two words, and so are an infinitive or a gerund with clitics (`dárselo`) and frequent imperatives (`dime`). A token that the segmenter leaves as one word gets the analysis of the whole token: `devuélvenos` is the imperative of `devolver`. A text without blank lines is analyzed in pieces that end at a sentence boundary, so memory does not grow with the length of the input.
- `tokens` - one token per line, a blank line separates sentences; separators around the token and everything after a tab are ignored, so a table with the token in the first column works too. A token is one word.
- `conllu` - [CoNLL-U](https://universaldependencies.org/format.html): the LEMMA, UPOS and FEATS columns are filled, while the other columns, comments, multiword tokens and empty nodes stay as they are.

``` bash
printf '1\tCasas\t_\t_\t_\t_\t0\troot\t_\t_\n2\tblancas\t_\t_\t_\t_\t1\tamod\t_\t_\n' | esmorph -i conllu -f conllu
```

```
1	Casas	casa	NOUN	_	Gender=Fem|Number=Plur	0	root	_	_
2	blancas	blanco	ADJ	_	Gender=Fem|Number=Plur	1	amod	_	_
```

## Output formats { #formats }

All formats except `jsonl` give every word its most probable analysis; `jsonl` writes all analyses of every token, the most probable first. A word without analyses gets its lower case as the lemma and the part of speech `X`.

### lemmas

The lemmas of the words of a sentence separated by spaces, a line per sentence; for the `text` input paragraphs are separated by a blank line.

### conllu

CoNLL-U as anySeg writes it: multiword tokens with their words, the `sent_id` and `text` comments and, for the `text` input, `newdoc` at the start of a file, `newpar` at the start of a paragraph, `SpaceAfter=No` and `SpacesAfter` in MISC. The result is the same as [`annotate`](analyzer.md#annotate) of the whole text.

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
3	lo	él	PRON	_	Case=Acc|Definite=Def|Gender=Masc|Number=Sing|Person=3|PrepCase=Npr|PronType=Prs	_	_	_	_
4-5	del	_	_	_	_	_	_	_	_
4	de	de	ADP	_	_	_	_	_	_
5	el	el	DET	_	Definite=Def|Gender=Masc|Number=Sing|PronType=Art	_	_	_	_
6	mar	mar	NOUN	_	Gender=Masc|Number=Sing	_	_	_	SpaceAfter=No
7	.	.	PUNCT	_	PunctType=Peri	_	_	_	_
```

### tsv

The word, lemma, part of speech, features and probability of the analysis separated by tabs, without a header; a blank line separates sentences.

``` bash
echo "Dime la verdad." | esmorph -f tsv
```

```
Di	decir	VERB	Mood=Imp|Number=Sing|Person=2|VerbForm=Fin	0.0625
me	yo	PRON	Case=Dat|Number=Sing|Person=1|PrepCase=Npr|PronType=Prs	0.45766
la	el	DET	Definite=Def|Gender=Fem|Number=Sing|PronType=Art	0.9774450451099099
verdad	verdad	NOUN	Gender=Fem|Number=Sing	0.999997
.	.	PUNCT	PunctType=Peri	1
```

### jsonl

[JSON Lines](https://jsonlines.org/): a sentence per line - the text (except for the `tokens` input) and the tokens with all their analyses, as [`analyze`](analyzer.md#analyze) gives them: the word, lemma, part of speech, features, labels, probability, the words of the token and the [source of the analysis](analysis.md). Below is the line of a sentence formatted for reading.

``` bash
echo "Dime." | esmorph -f jsonl
```

``` json
{
  "text": "Dime.",
  "tokens": [
    {"word": "Dime", "analyses": [
      {"word": "Dime", "lemma": "Dime", "pos": "PROPN", "feats": {}, "labels": [],
       "prob": 0.9375, "words": ["Dime"], "source": "rule"},
      {"word": "dime", "lemma": "decir", "pos": "VERB", "feats": {"Mood": "Imp", "Number": "Sing", "Person": "2", "VerbForm": "Fin"},
       "labels": [], "prob": 0.0625, "words": ["di", "me"], "source": "clitics"}
    ]},
    {"word": ".", "analyses": [
      {"word": ".", "lemma": ".", "pos": "PUNCT", "feats": {"PunctType": "Peri"}, "labels": [],
       "prob": 1, "words": ["."], "source": "rule"}
    ]}
  ]
}
```

## Exit codes { #exit-codes }

| Code | When |
| :--- | :--- |
| 0 | Success, including when the output is closed before the end (`esmorph text.txt \| head`) |
| 1 | An input file is missing, cannot be read or is not in UTF-8, a line is not in UTF-8 (it is analyzed with U+FFFD), an invalid CoNLL-U line; the other files are still analyzed |
| 1 | The dictionary cannot be read, the output file cannot be created or is an input file or the dictionary; nothing is analyzed |
| 1 | A write error, a closed standard output; the analysis stops |
| 2 | Invalid options |

Error messages go to the standard error.
