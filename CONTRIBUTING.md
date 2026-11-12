# Contributing

Bug reports, ideas and pull requests are welcome, in English or Spanish.

## Reporting a bug or proposing an idea

Open an [issue](https://github.com/SergeyShk/esMorph/issues/new/choose) using a template. A bug report needs the word forms or the code that reproduce it, the expected and actual result, and the versions of esMorph, Python and the OS. An idea needs the task you are solving and why the current features are not enough.

## Development environment

The project uses [uv](https://docs.astral.sh/uv/) for dependencies, [maturin](https://www.maturin.rs/) to build the Rust module and [ruff](https://docs.astral.sh/ruff/) for linting and formatting. Python 3.11 or newer and [Rust](https://www.rust-lang.org/tools/install) 1.88 or newer are required.

```bash
git clone https://github.com/SergeyShk/esMorph.git
cd esMorph

make deps                   # create the environment, install all dependencies and build the Rust module
make dictionary             # build the dictionary from the snapshot of Wiktionary, the counts of treebanks and the context model
uv run pre-commit install   # hooks: linters on commit, tests on push
```

The Rust module is built when the package is installed into the environment; `uv run` rebuilds it when the files of `crates/` or `Cargo.toml` change. The dictionary `python/esmorph/data/es.dict` is not in git: `make dictionary` downloads the snapshot of the Spanish entries of Wiktionary (a dataset on Hugging Face, checked by SHA-256), turns it into a table with `scripts/export_wiktionary.py` and builds the file with `crates/esmorph-build`, which adds the weights of the context model `data/es-context.bin`. The full list of commands is in `make help`.

The context model is trained by `scripts/train_context.py` (`uv run --with-editable . scripts/train_context.py`, a few minutes on the CPU): it writes `data/es-context.bin` and `data/es-context-check.tsv`, the analyses of sentences of AnCora dev that a test compares with the analyzer. A change of the analyses of the dictionary (new parts of speech or features, other candidates) needs a new model: the builder refuses a dictionary with tags the model was not trained with, so the dictionary is built without the model (`esmorph-build` with three arguments), the model is trained with it (`--dictionary`), and the dictionary is built again.

## Checks

Before submitting a pull request, these must pass:

```bash
make lint        # ruff check --fix, ruff format (code and Python in Markdown), mypy, cargo fmt, cargo clippy
make test-cov    # pytest with docstring examples (doctest) and the coverage threshold, cargo test
make docs-build  # mkdocs build --strict, if the documentation changed
```

The licenses of the Rust crates built into the module are in `THIRD-PARTY-NOTICES.txt` and go into the wheels; after a change of `Cargo.lock` the file is written again with `uv run python scripts/notices.py`, otherwise `test_third_party_notices` fails.

CI runs the same on Python 3.11-3.14 and the free-threaded 3.14t (Linux) and on Windows and macOS for one Python version, and checks the build on the minimum Rust version 1.88. When raising the minimum Rust version, change it in `rust-version` in `Cargo.toml`, in the `msrv` job of `ci.yml` and on the installation pages of the documentation and in the README.

## What a pull request is expected to contain

- **One topic per pull request.** Unrelated changes go separately.
- **Tests.** New code is covered by tests: Rust code by the tests in `crates/`, the Python API by the tests in `tests/`, Python coverage stays at 90% or higher.
- **Documentation.** The documentation is bilingual - the English page `docs/*.md` and the Spanish page `docs/*.es.md` next to it, and `tests/test_docs.py` checks that their sections, anchors and links match. Examples in docstrings and the documentation show the actual output.
- **Compatibility.** Changes to the Python API, the analyses or the dictionary format come with an explanation in the pull request description: it goes into the release notes. A change of the dictionary format raises its version in `crates/esmorph/src/format.rs`.
- **Style.** Python code is formatted with ruff and its types are checked with mypy; Rust code is formatted with rustfmt and passes clippy without warnings (`make format`, `make lint`). Docstrings are in English in the project format (`Description`, `Arguments`, `Returns`, `Raises`, `Example`). Comments do not repeat what the code says.
- **Commits.** Messages in English, imperative mood: "Add ...", "Fix ...", "Remove ...".

The pull request description answers two questions: what was done and why. If it closes an issue, mention its number.

## Repository layout

- `crates/esmorph/` - the analyzer core in Rust: the dictionary format, the builder (feature `build`) and the analysis; published on crates.io.
- `crates/esmorph-dict-es/` - the crate with the dictionary for `esmorph` (feature `bundled`): the file is not in git, `build.rs` takes `python/esmorph/data/es.dict`, and without it builds an empty dictionary only with `ESMORPH_EMPTY_DICTIONARY=1` from `.cargo/config.toml`; published on crates.io.
- `crates/esmorph-build/` - the builder of the dictionary from the table of the export, not published.
- `crates/esmorph-py/` - the Python module `esmorph._esmorph` built with PyO3, not published separately.
- `python/esmorph/` - the Python package.
- `tests/` - tests of the Python package; `tests/test_docs.py` keeps the two documentation languages in sync.
- `docs/` - MkDocs (Material) documentation, `mkdocs.yml` - navigation and its Spanish translation.
- `scripts/` - `wiktionary_snapshot.py` makes the snapshot of Wiktionary, `export_wiktionary.py` turns it into the table of the builder, `conventions.py` brings it to the conventions of the treebanks with `data/closed-classes.tsv`, `export_ud.py` counts analyses in the treebanks, `train_context.py` trains the context model (`data/es-context.bin`), `smoke.py` checks the installed package before publishing, `notices.py` writes the licenses of the crates.
- `.github/workflows/` - CI (`ci.yml`), publishing (`publish.yml`), documentation (`docs.yml`).

The sdist is made of the files tracked by git (`sdist-generator = "git"`), so that `maturin build --locked` works on it with all the members of the workspace in place. It can only be made inside a git checkout: `uv build` and `python -m build` fail on an unpacked source archive, while `pip install` and `maturin build` of the sdist work anywhere.

The version lives in `pyproject.toml` and `Cargo.toml`, changed together; `tests/test_package.py` checks that they match. Releases are made through GitHub Releases, changes are described in the release notes, there is no separate CHANGELOG file.
