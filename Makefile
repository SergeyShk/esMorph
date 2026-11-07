.PHONY: help uv deps dictionary lock lint ruff format mypy rust-lint test test-cov clean clean-build clean-pyc clean-test clean-rust build docs-build docs-serve docs-deploy
.DEFAULT_GOAL := help
SRC_PATH := python
TESTS_PATH := tests
SCRIPTS_PATH := scripts
PACKAGE := esmorph
DICTIONARY := $(SRC_PATH)/$(PACKAGE)/data/es.dict
EXPORT_PATH := target/wiktionary
EXPORT_STAMP := $(EXPORT_PATH)/.exported

help: ## Show the list of commands
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "%-20s %s\n", $$1, $$2}'

uv: ## Check that uv and Rust are installed
	@which uv >/dev/null 2>&1 || { \
		echo "uv is not installed. Run 'curl -LsSf https://astral.sh/uv/install.sh | sh' or 'brew install uv'"; \
		exit 1; \
	}
	@which cargo >/dev/null 2>&1 || { \
		echo "Rust is not installed. Run 'curl --proto =https --tlsv1.2 -sSf https://sh.rustup.rs | sh'"; \
		exit 1; \
	}

deps: uv ## Install the dependencies and build the extension module
ifeq ($(MODE), ci)
	uv sync --locked --all-groups
else
	uv sync --all-groups
endif

dictionary: $(DICTIONARY) ## Build the dictionary from the snapshot of Wiktionary

$(EXPORT_STAMP): $(SCRIPTS_PATH)/export_wiktionary.py | deps
	uv run python $(SCRIPTS_PATH)/export_wiktionary.py $(EXPORT_PATH)
	touch $@

$(DICTIONARY): $(EXPORT_STAMP) $(wildcard crates/esmorph/src/*.rs crates/esmorph-build/src/*.rs)
	cargo run --release --locked -p esmorph-build -- $(EXPORT_PATH) $(DICTIONARY)

lock: uv ## Update the lock files to the latest versions of the dependencies
	uv lock --upgrade
	cargo update

lint: ruff mypy rust-lint ## Run all code checks

ruff: deps ## Check and format the Python code with ruff
ifeq ($(MODE), ci)
	uv run ruff check $(SRC_PATH) $(SCRIPTS_PATH) $(TESTS_PATH)
	uv run ruff format $(SRC_PATH) $(SCRIPTS_PATH) $(TESTS_PATH) --check
	git ls-files -z '*.md' | xargs -0 uv run ruff format --check
else
	uv run ruff check $(SRC_PATH) $(SCRIPTS_PATH) $(TESTS_PATH) --fix
	uv run ruff format $(SRC_PATH) $(SCRIPTS_PATH) $(TESTS_PATH)
	git ls-files -z '*.md' | xargs -0 uv run ruff format
endif

format: deps ## Format the code
	uv run ruff format $(SRC_PATH) $(SCRIPTS_PATH) $(TESTS_PATH)
	git ls-files -z '*.md' | xargs -0 uv run ruff format
	cargo fmt --all

mypy: deps ## Check the types with mypy
	uv run mypy

rust-lint: uv ## Check the Rust code with rustfmt and clippy
ifeq ($(MODE), ci)
	cargo fmt --all --check
else
	cargo fmt --all
endif
	cargo clippy --workspace --all-targets -- -D warnings

test: deps dictionary ## Run the tests
	uv run pytest
	cargo test -p $(PACKAGE) -p esmorph-dict-es --all-features

test-cov: deps dictionary ## Run the tests with the coverage check
	uv run pytest --cov $(PACKAGE)
	cargo test -p $(PACKAGE) -p esmorph-dict-es --all-features

clean: clean-build clean-pyc clean-test ## Remove all artifacts except the Rust build
	rm -f .coverage coverage.xml

clean-build: ## Remove the artifacts of the package build
	rm -fr build/ dist/ .eggs/
	find . -name '*.egg-info' -not -path './target/*' -exec rm -fr {} +

clean-pyc: ## Remove the artifacts of Python compilation
	find . -name '*.pyc' -not -path './target/*' -delete
	find . -name '*.pyo' -not -path './target/*' -delete
	find . -name '__pycache__' -type d -not -path './target/*' -exec rm -rf {} + 2>/dev/null || true

clean-test: ## Remove the artifacts of tests and linters
	rm -fr .pytest_cache .mypy_cache .ruff_cache

clean-rust: ## Remove the Rust build
	cargo clean

build: clean uv dictionary ## Build the distribution for the current platform
	uv build
	ls -l dist

docs-build: deps ## Build the documentation
	rm -fr site/
	uv run mkdocs build --strict

docs-serve: deps ## Start the documentation server
	uv run mkdocs serve

docs-deploy: deps ## Deploy the documentation
	uv run mkdocs gh-deploy
