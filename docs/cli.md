# The esmorph command

``` bash
esmorph [options] [file ...]
```

The command analyzes text from files or the standard input and writes lemmas, parts of speech and [Universal Dependencies](https://universaldependencies.org/) features in one of four formats. It is installed with the package (`pip install esmorph`) and also runs as `python -m esmorph`; without Python, `cargo install esmorph --features cli` installs it. Every word gets its most probable analysis in the context of its sentence, as [`annotate`](analyzer.md#annotate) gives it: the words of a multiword token are analyzed by the token. The Rust core analyzes and writes in one thread, about 100 thousand tokens per second, and over a million with `--no-context`.

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
| `-d`, `--dictionary` | the package dictionary | An esMorph dictionary file, checked whole as with [`verify=True`](analyzer.md#parameters) |
| `--no-context` | | Order the analyses of a word as [`analyze`](analyzer.md#analyze) does, without the context model: faster; for a list of words and a dictionary without a model |
| `-o`, `--output` | the standard output | The result file; `-` is the standard output |
| `-h`, `--help` | | Show the help |
| `-V`, `--version` | | Show the version |

Files are read one after another; without files and in place of `-` the standard input is read. The input is in UTF-8: a file in another encoding (Latin-1, cp1252, UTF-16) is not analyzed, unless its first letter with an accent comes after the first 64 KiB: then the text before that line is analyzed up to its last blank line. A single line with invalid bytes is analyzed with U+FFFD in their place. An option value is written separately or attached: `-f conllu`, `-fconllu`, `--format=conllu`. Text is analyzed by paragraphs: a paragraph is written as soon as a blank line or the end of the input closes it - on a terminal at once, and to a pipe or a file whenever the command waits for the next input line, so it can be kept open and fed text paragraph by paragraph. The inputs `tokens` and `conllu` are written by sentences.

In Windows PowerShell 5.1 the `>` redirection and a pipeline (`Get-Content text.txt | esmorph`) corrupt letters with accents: pass the file as an argument and write the result with the `-o` option (in PowerShell 7 they work too). Text passed with `echo` in cmd comes in the console encoding: run `chcp 65001` first.

## Input { #input }

- `text` - text: a blank line ends a paragraph. Tokens, their words and sentences are found by the segmenter [anySeg](https://sergeyshk.github.io/anySeg/) following the conventions of Universal Dependencies: `del` and `al` are two words, and so are an infinitive or a gerund with clitics (`dárselo`) and frequent imperatives (`dime`). A token that the segmenter leaves as one word gets the analysis of the whole token: `devuélvenos` is the imperative of `devolver`. A paragraph longer than 256 KiB is analyzed in pieces that end at a sentence boundary, so memory does not grow with the length of the input; its sentences may then differ slightly from those of the whole text.
- `tokens` - one token per line, a blank line separates sentences; separators around the token and everything after a tab are ignored, so a table with the token in the first column works too. A token is one word. A sentence without a blank line is closed after 8,192 tokens. The words around a token change its analysis, so a list of words that do not make a sentence is analyzed with `--no-context` or with a blank line after every word; the `tsv` format gives a line per word.
- `conllu` - [CoNLL-U](https://universaldependencies.org/format.html): the LEMMA, UPOS and FEATS columns are filled, while the other columns, comments, multiword tokens and empty nodes stay as they are. An invalid sentence is written as it is in the `conllu` format and left out of the others, its line goes to the error message, and the rest of the file is analyzed.

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

CoNLL-U as anySeg writes it: multiword tokens with their words, the `sent_id` and `text` comments and, for the `text` input, `newdoc` at the start of a file, `newpar` at the start of a paragraph, `SpaceAfter=No` and `SpacesAfter` in MISC. The result is the same as [`annotate`](analyzer.md#annotate) of the whole text, except for paragraphs longer than 256 KiB. Analysis gives no syntax tree: from the `text` and `tokens` inputs the HEAD and DEPREL columns are `_`, so a tool that needs a tree, such as the evaluation script of the CoNLL 2018 shared task, reads the result of `-i conllu` on a file that has one.

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

The word, lemma, part of speech, features and probability of the analysis separated by tabs, without a header (the probability is the score of the context model in the sentence, as [`analyze_sentence`](analyzer.md#analyze_sentence) gives it, and with `--no-context` as `analyze` gives it); a blank line separates sentences. The probability of a word of a multiword token is its share among the analyses that fit the token: the verb `di` of `dime` among the analyses with these words, the clitic `la` of `verla` among those of a pronoun.

``` bash
echo "Dime la verdad." | esmorph -f tsv
```

```
Di	decir	VERB	Mood=Imp|Number=Sing|Person=2|VerbForm=Fin	1
me	yo	PRON	Case=Dat|Number=Sing|Person=1|PrepCase=Npr|PronType=Prs	0.45766
la	el	DET	Definite=Def|Gender=Fem|Number=Sing|PronType=Art	0.9989243733311355
verdad	verdad	NOUN	Gender=Fem|Number=Sing	0.999981820943117
.	.	PUNCT	PunctType=Peri	1
```

### jsonl

[JSON Lines](https://jsonlines.org/): a sentence per line - the text (except for the `tokens` input) and the tokens with all their analyses, as [`analyze_sentence`](analyzer.md#analyze_sentence) orders them in the sentence, and with `--no-context` as [`analyze`](analyzer.md#analyze) gives them: the word, lemma, part of speech, features, probability, the words of the token and the [source of the analysis](analysis.md). Below is the line of a sentence formatted for reading.

``` bash
echo "Vino ayer." | esmorph -f jsonl
```

``` json
{
  "text": "Vino ayer.",
  "tokens": [
    {"word": "Vino", "analyses": [
      {"word": "vino", "lemma": "venir", "pos": "VERB", "feats": {"Mood": "Ind", "Number": "Sing", "Person": "3", "Tense": "Past", "VerbForm": "Fin"},
       "prob": 0.8093463865008943, "words": ["vino"], "source": "dictionary"},
      {"word": "vino", "lemma": "vino", "pos": "NOUN", "feats": {"Gender": "Masc", "Number": "Sing"},
       "prob": 0.16867937111012504, "words": ["vino"], "source": "dictionary"},
      {"word": "Vino", "lemma": "Vino", "pos": "PROPN", "feats": {},
       "prob": 0.021974242388980627, "words": ["Vino"], "source": "rule"}
    ]},
    {"word": "ayer", "analyses": [
      {"word": "ayer", "lemma": "ayer", "pos": "ADV", "feats": {},
       "prob": 0.9995206355999463, "words": ["ayer"], "source": "dictionary"},
      {"word": "ayer", "lemma": "ayer", "pos": "NOUN", "feats": {"Gender": "Masc", "Number": "Sing"},
       "prob": 0.0004793644000537769, "words": ["ayer"], "source": "dictionary"}
    ]},
    {"word": ".", "analyses": [
      {"word": ".", "lemma": ".", "pos": "PUNCT", "feats": {"PunctType": "Peri"},
       "prob": 1, "words": ["."], "source": "rule"}
    ]}
  ]
}
```

## Exit codes { #exit-codes }

| Code | When |
| :--- | :--- |
| 0 | Success, including when the output is closed before the end (`esmorph text.txt \| head`) |
| 1 | An input file is missing, cannot be read or is not in UTF-8, a line is not in UTF-8 (it is analyzed with U+FFFD), an invalid CoNLL-U sentence; the rest is still analyzed |
| 1 | The dictionary cannot be read or is damaged, it has no context model and `--no-context` is not given, the output file cannot be created, the output file or the standard output is an input file or the dictionary; nothing is analyzed |
| 1 | A write error, a closed standard output (the command installed with cargo writes it to /dev/null and ends with 0); the analysis stops |
| 2 | Invalid options |

Error messages go to the standard error.
