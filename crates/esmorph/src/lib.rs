//! Spanish morphological analyzer: the analyses of a word form (lemma, part of speech and
//! Universal Dependencies features). Word forms are kept in a finite state automaton, and
//! analyses are restored through paradigms
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
//! `esmorph-dict-es`, `build` - building dictionaries (`Builder`)
#![cfg_attr(docsrs, feature(doc_cfg))]

mod alphabet;
#[cfg(any(test, feature = "build"))]
mod builder;
mod dictionary;
mod error;
mod format;
mod ud;

#[cfg(feature = "build")]
#[cfg_attr(docsrs, doc(cfg(feature = "build")))]
pub use builder::Builder;
pub use dictionary::{Analysis, Dictionary, Entry, Source};
pub use error::Error;
pub use ud::UdTag;

/// Examples of the README of the crate are doc tests
#[cfg(all(doctest, feature = "bundled"))]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;
