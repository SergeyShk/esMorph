# Installation

Python 3.11 or newer is required. Pre-release versions `0.1.0.devN` are published only on TestPyPI:

``` bash
pip install --index-url https://test.pypi.org/simple/ esmorph
```

## Prebuilt wheels

Wheels are built for Linux (x86_64 and aarch64, manylinux and musllinux), macOS (Intel and Apple Silicon) and Windows (x64 and arm64). One wheel serves every Python version starting with 3.11, and the free-threaded Python 3.14t has a wheel of its own; Rust is not needed to install them. The free-threaded Python 3.13 is not supported: free-threaded builds start with 3.14.

## Building from source

On other platforms and on free-threaded builds newer than 3.14 pip builds the package from source, which requires [Rust](https://www.rust-lang.org/tools/install) 1.88 or newer:

``` bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
pip install 'maturin>=1.9.4,<2'
pip install --no-build-isolation --index-url https://test.pypi.org/simple/ esmorph
```

The build requirement is installed first and from PyPI, because TestPyPI has no suitable version of maturin; the package itself must come only from TestPyPI. Run both commands in an activated virtual environment: with `--no-build-isolation` the build looks for the `maturin` command on `PATH`.

The development environment is described in [CONTRIBUTING.md](https://github.com/SergeyShk/esMorph/blob/master/CONTRIBUTING.md).
