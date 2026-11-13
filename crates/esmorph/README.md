# esmorph

Fast Spanish morphological analyzer written in Rust, the core of the
[esMorph](https://github.com/SergeyShk/esMorph) library for Python.

- The analyses of a word form by a dictionary built from Wiktionary and treebanks, the most probable first: the lemma,
  the part of speech and the [Universal Dependencies](https://universaldependencies.org/) features.
- The analyses of the tokens of a sentence ordered by a small context model trained on the treebanks: `la` is a
  pronoun in `Ella la vio` and an article in `la casa`.
- Case does not matter. A word absent as written is looked for without its accents and with a lost diaeresis.
- A verb with enclitic pronouns is analyzed as its verb, with the words of the token: `dámelo` is `da` + `me` + `lo`.
- The forms of a lexeme in the order of its paradigm and the form with given features: conjugation, gender and number.
- Tokens out of the dictionary by rules: punctuation, symbols, numbers, Roman numerals, names and acronyms.
- Unknown lowercase words guessed by their last part after a hyphen, a known prefix or their ending, with their forms.
- The dictionary is mapped into memory or read from the data of the program without copies.
- Documents of the [anySeg](https://crates.io/crates/anyseg) segmenter get the lemma, the part of speech and the features of every word in one call.

## Installation

```toml
[dependencies]
esmorph = "0.3"
```

The dictionary is in the crate [`esmorph-dict-es`](https://crates.io/crates/esmorph-dict-es), used through the feature `bundled` (on by default). Without it the dictionary is read from a file:

```rust,ignore
let dictionary = esmorph::Dictionary::open("es.dict")?;
```

## Example

```rust
use esmorph::Dictionary;

fn main() -> Result<(), esmorph::Error> {
    let dictionary = Dictionary::bundled()?;
    for analysis in dictionary.analyze("camiones") {
        let tag = dictionary.ud_tag(analysis.ud).unwrap();
        println!("{} {} {}", analysis.lemma, tag.pos(), tag.feats_string());
    }
    let hablar = &dictionary.analyze("hablar")[0];
    let past = [("Tense", "Past"), ("Person", "3"), ("Number", "Plur")];
    assert_eq!(dictionary.inflect(hablar, &past).unwrap().word, "hablaron");
    Ok(())
}
```

## Analysis in context

`analyze_sentence` gives the analyses of the tokens of a sentence as `analyze` gives them, ordered by the context model of the
dictionary, a BiLSTM trained on the treebanks UD Spanish AnCora and GSD; the probability is its score in the sentence. The
model is read on the first call, in a couple of milliseconds, and analyzes about 170 thousand tokens per second in one
thread, while `analyze` gives over a million.

```rust
fn main() -> Result<(), esmorph::Error> {
    let dictionary = esmorph::Dictionary::bundled()?;
    let sentence = dictionary.analyze_sentence(&["Ella", "la", "vio"])?;
    assert_eq!(dictionary.ud_tag(sentence[1][0].ud).unwrap().pos(), "PRON");
    Ok(())
}
```

## The esmorph command

The feature `cli` builds the command `esmorph`, which analyzes text from files and the standard input and writes lemmas, CoNLL-U, TSV or JSON Lines:

```bash
cargo install esmorph --features cli
echo "Dime la verdad del caso." | esmorph
```

## Documents of anySeg

With the feature `anyseg` the dictionary fills the columns of the words of a document of the segmenter [anySeg](https://crates.io/crates/anyseg), whose crate is `esmorph::anyseg`: `annotate` chooses the analysis of every word in the context of its sentence, and `annotate_without_context` as `analyze` orders them, more than ten times faster:

```toml
[dependencies]
esmorph = { version = "0.3", features = ["anyseg"] }
```

```rust
use esmorph::anyseg::{Column, Segmenter};

fn main() -> Result<(), esmorph::Error> {
    let dictionary = esmorph::Dictionary::bundled()?;
    let mut document = Segmenter::new("es").unwrap().segment("Dime la verdad.");
    dictionary.annotate(&mut document)?;
    assert_eq!(document.column(Column::Lemma, 0), Some("decir"));
    Ok(())
}
```

## License

The code is distributed under the [MIT](https://github.com/SergeyShk/esMorph/blob/master/LICENSE.txt) license. The dictionary is under [CC BY-SA 4.0](https://creativecommons.org/licenses/by-sa/4.0/).
