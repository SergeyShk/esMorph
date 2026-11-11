# esmorph

Fast Spanish morphological analyzer written in Rust, the core of the
[esMorph](https://github.com/SergeyShk/esMorph) library for Python.

- The analyses of a word form by a dictionary built from Wiktionary and treebanks, the most probable first: the lemma,
  the part of speech and the [Universal Dependencies](https://universaldependencies.org/) features.
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
esmorph = "0.1"
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

## Documents of anySeg

With the feature `anyseg` the dictionary fills the columns of the words of a document of the segmenter [anySeg](https://crates.io/crates/anyseg), whose crate is `esmorph::anyseg`:

```toml
[dependencies]
esmorph = { version = "0.2", features = ["anyseg"] }
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
