//! Spanish morphological analyzer: the analyses of a word form (lemma, part of speech and
//! Universal Dependencies features) and the forms of its lexeme. Word forms are kept in a finite
//! state automaton, and analyses and forms are restored through paradigms
//!
//! ```
//! # #[cfg(feature = "bundled")] {
//! use esmorph::Dictionary;
//!
//! let dictionary = Dictionary::bundled()?;
//! let analyses = dictionary.analyze("casas");
//! assert!(analyses.iter().any(|analysis| analysis.lemma == "casa"));
//! # }
//! # Ok::<(), esmorph::Error>(())
//! ```
//!
//! Features: `bundled` (on by default) - the dictionary of esMorph from the crate
//! `esmorph-dict-es`, `anyseg` - the analysis of a document of the anySeg segmenter
//! (`Dictionary::annotate`, the crate is `esmorph::anyseg`), `cli` - the command `esmorph`
//! (`cargo install esmorph --features cli`), `build` - building dictionaries (`Builder`)
#![cfg_attr(docsrs, feature(doc_cfg))]

mod alphabet;
#[cfg(any(test, feature = "build"))]
mod builder;
#[cfg(feature = "cli")]
#[cfg_attr(docsrs, doc(cfg(feature = "cli")))]
pub mod cli;
mod clitics;
mod dictionary;
mod error;
mod format;
mod rules;
mod ud;

#[cfg(feature = "anyseg")]
#[cfg_attr(docsrs, doc(cfg(feature = "anyseg")))]
pub use anyseg;
#[cfg(feature = "build")]
#[cfg_attr(docsrs, doc(cfg(feature = "build")))]
pub use builder::Builder;
#[doc(hidden)]
pub use dictionary::Fields;
pub use dictionary::{Analysis, Dictionary, Entry, Source};
pub use error::Error;
pub use ud::UdTag;

/// Examples of the README of the crate are doc tests
#[cfg(all(doctest, feature = "bundled", feature = "anyseg"))]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;
