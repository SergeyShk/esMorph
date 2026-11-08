# esmorph

Fast Spanish morphological analyzer written in Rust, the core of the
[esMorph](https://github.com/SergeyShk/esMorph) library for Python.

- The analyses of a word form by a dictionary built from Wiktionary and treebanks, the most probable first: the lemma,
  the part of speech and the [Universal Dependencies](https://universaldependencies.org/) features.
- Case does not matter. A word absent as written is looked for without its accents and with a lost diaeresis.
- A verb with enclitic pronouns is analyzed as its verb, with the words of the token: `dámelo` is `da` + `me` + `lo`.
- The dictionary is mapped into memory or read from the data of the program without copies.

Work in progress: unknown words and inflection come in the next versions.

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
# #[cfg(feature = "bundled")] {
use esmorph::Dictionary;

let dictionary = Dictionary::bundled()?;
for analysis in dictionary.analyze("camiones") {
    let tag = dictionary.ud_tag(analysis.ud).unwrap();
    println!("{} {} {}", analysis.lemma, tag.pos(), tag.feats_string());
}
# }
# Ok::<(), esmorph::Error>(())
```

## License

The code is distributed under the [MIT](https://github.com/SergeyShk/esMorph/blob/master/LICENSE.txt) license. The dictionary is under [CC BY-SA 4.0](https://creativecommons.org/licenses/by-sa/4.0/).
