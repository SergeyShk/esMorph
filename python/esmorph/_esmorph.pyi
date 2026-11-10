from collections.abc import Iterable
from os import PathLike
from pathlib import Path
from typing import final

__all__ = ["Analysis", "Analyzer", "FormNotFoundError", "__version__"]

__version__: str

class FormNotFoundError(LookupError):
    """The lexeme has no form with the requested features"""

@final
class Analysis:
    """Analysis of a word form"""

    @property
    def word(self) -> str:
        """The word form as the dictionary spells it"""

    @property
    def lemma(self) -> str:
        """The lemma"""

    @property
    def pos(self) -> str:
        """The part of speech of Universal Dependencies"""

    @property
    def feats(self) -> dict[str, str]:
        """The Universal Dependencies features"""

    @property
    def prob(self) -> float:
        """Share of the analysis among the analyses of the word"""

    @property
    def words(self) -> tuple[str, ...]:
        """Words of the token: a verb and its clitics, the words of a contraction, or the word"""

    @property
    def source(self) -> str:
        """Where the analysis comes from: "dictionary", "clitics", "rule" or "guess\""""

    def forms(self) -> list[Analysis]:
        """Forms of the lexeme"""

    def inflect(self, **feats: str) -> str:
        """The form with the given features"""

    def __copy__(self) -> Analysis: ...
    def __deepcopy__(self, _memo: object) -> Analysis: ...

def _analysis(
    word: str,
    lemma: str,
    pos: str,
    feats: str,
    prob: float,
    words: list[str],
    source: str,
    path: str | PathLike[str] | None = None,
) -> Analysis:
    """Restores an analysis from pickle"""

def _analyzer(path: str | PathLike[str] | None = None) -> Analyzer:
    """Restores an analyzer from pickle"""

@final
class Analyzer:
    """Morphological analyzer of Spanish"""

    def __new__(cls, path: str | PathLike[str] | None = None) -> Analyzer: ...
    @property
    def path(self) -> Path:
        """Path to the dictionary file"""

    def analyze(self, word: str) -> list[Analysis]:
        """Analyses of a word form"""

    def analyze_many(self, words: Iterable[str]) -> list[list[Analysis]]:
        """Analyses of a list of word forms"""

    def lemmatize(self, words: Iterable[str]) -> list[str]:
        """Lemmas of a list of word forms"""

    def is_known(self, word: str) -> bool:
        """Whether the dictionary has the word form"""
