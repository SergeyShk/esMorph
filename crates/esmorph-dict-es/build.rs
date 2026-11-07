//! The dictionary file for `include_bytes!`: from the Python package of the workspace (`make
//! dictionary`) or from the crate (a published crate, and CI before publishing). An empty
//! dictionary only in the workspace and with `ESMORPH_EMPTY_DICTIONARY=1`, set by the
//! `.cargo/config.toml` of the repository: so code that needs no dictionary is checked, while a
//! crate without a dictionary is not packaged and a dependency from git does not build without one

use std::path::{Path, PathBuf};

fn main() {
    let crate_dir = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap_or_default());
    let output = PathBuf::from(std::env::var_os("OUT_DIR").unwrap_or_default()).join("es.dict");
    let package = crate_dir.join("../../python/esmorph");
    let candidates = [package.join("data/es.dict"), crate_dir.join("es.dict")];
    watch(&candidates[0], &package);
    watch(&candidates[1], &crate_dir);
    println!("cargo::rerun-if-env-changed=ESMORPH_EMPTY_DICTIONARY");
    let empty = std::env::var_os("ESMORPH_EMPTY_DICTIONARY").is_some_and(|value| value == "1");
    let result = match candidates.iter().find(|path| path.is_file()) {
        Some(found) => std::fs::copy(found, &output).map(drop),
        None if empty && package.is_dir() => {
            println!(
                "cargo::warning=the dictionary is not built (make dictionary): the crate has an empty one"
            );
            std::fs::write(&output, b"")
        }
        None => panic!(
            "no dictionary file es.dict: build it in the esMorph repository (make dictionary) \
             or use the crate esmorph from crates.io"
        ),
    };
    if let Err(err) = result {
        panic!("{}: {err}", output.display());
    }
}

/// Rebuild when the file changes; a missing file is watched through the nearest existing directory
/// not above `root`: otherwise cargo would rebuild the crate on every build, and above `root` of a
/// crate used by a path outside esMorph are the directories of the user's project
fn watch(path: &Path, root: &Path) {
    let existing = path
        .ancestors()
        .take_while(|ancestor| ancestor.starts_with(root))
        .find(|ancestor| ancestor.exists());
    if let Some(existing) = existing {
        println!("cargo::rerun-if-changed={}", existing.display());
    }
}
