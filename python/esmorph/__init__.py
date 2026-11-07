# Spanish morphological analyzer (esMorph)
#
# Copyright (C) 2026
# Author: Sergey Shkarin <kouki.sergey@gmail.com>
# URL: <https://github.com/SergeyShk/esMorph>

from ._esmorph import Analysis, Analyzer, __version__

__description__ = """Fast Spanish morphological analyzer written in Rust with a Python API.
Requires Python 3.11 or newer"""
__author__ = "Sergey Shkarin"
__author_email__ = "kouki.sergey@gmail.com"

__all__ = ["Analysis", "Analyzer", "__version__"]
