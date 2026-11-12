# Installation

Python 3.11 or newer is required.

``` bash
pip install esmorph
```

The package also installs the [`esmorph` command](cli.md).

## Prebuilt wheels

PyPI hosts wheels for Linux (x86_64 and aarch64, manylinux and musllinux), macOS (Intel and Apple Silicon) and Windows (x64 and arm64). One wheel serves every Python version starting with 3.11, and the free-threaded Python 3.14t has a wheel of its own; Rust is not needed to install them. The dictionary is part of the package and needs no separate installation. The free-threaded Python 3.13 is not supported: free-threaded builds start with 3.14.

Pre-release versions `X.Y.Z.devN` are published only on TestPyPI:

``` bash
pip install --index-url https://test.pypi.org/simple/ --extra-index-url https://pypi.org/simple/ --pre esmorph
```

uv takes a package from the first index that has it, so it needs another order of indexes:

``` bash
uv pip install --extra-index-url https://test.pypi.org/simple/ --index-strategy unsafe-best-match --pre esmorph
```

## Building from source

On other platforms and on free-threaded builds newer than 3.14 pip builds the package from source, which requires [Rust](https://www.rust-lang.org/tools/install) 1.88 or newer:

``` bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
pip install esmorph
```

The development environment is described in [CONTRIBUTING.md](https://github.com/SergeyShk/esMorph/blob/master/CONTRIBUTING.md).

## Rust { #rust }

The analyzer core is published on crates.io as the [`esmorph`](https://crates.io/crates/esmorph) crate and the dictionary as the [`esmorph-dict-es`](https://crates.io/crates/esmorph-dict-es) crate: it is enabled by default through the feature `bundled` and loaded with `Dictionary::bundled()`. The API documentation is on [docs.rs](https://docs.rs/esmorph).

``` bash
cargo add esmorph
```

The feature `anyseg` adds `Dictionary::annotate` for documents of the segmenter [anySeg](https://sergeyshk.github.io/anySeg/), and the feature `cli` adds the [`esmorph` command](cli.md), which installs without Python:

``` bash
cargo install esmorph --features cli
```
