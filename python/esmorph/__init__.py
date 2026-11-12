# Spanish morphological analyzer (esMorph)
#
# Copyright (C) 2026
# Author: Sergey Shkarin <kouki.sergey@gmail.com>
# URL: <https://github.com/SergeyShk/esMorph>

from ._esmorph import Analysis, Analyzer, FormNotFoundError, __version__

# typing takes longer to import than the package: the types are imported on first use
TYPE_CHECKING = False
if TYPE_CHECKING:
    from ._types import ContextInfo as ContextInfo, DictionaryInfo as DictionaryInfo
else:

    def __getattr__(name: str) -> object:
        if name in ("ContextInfo", "DictionaryInfo"):
            from . import _types

            return getattr(_types, name)
        raise AttributeError(f"module {__name__!r} has no attribute {name!r}")


__description__ = """Fast Spanish morphological analyzer written in Rust with a Python API.
Requires Python 3.11 or newer"""
__author__ = "Sergey Shkarin"
__author_email__ = "kouki.sergey@gmail.com"

__all__ = [
    "Analysis",
    "Analyzer",
    "ContextInfo",
    "DictionaryInfo",
    "FormNotFoundError",
    "__version__",
]
