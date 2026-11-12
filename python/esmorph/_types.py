# Spanish morphological analyzer (esMorph)
#
# Copyright (C) 2026
# Author: Sergey Shkarin <kouki.sergey@gmail.com>
# URL: <https://github.com/SergeyShk/esMorph>

"""Types of the values of the analyzer"""

from typing import Required, TypedDict


class DictionaryInfo(TypedDict, total=False):
    """Description of a dictionary, as Analyzer.dictionary_info gives it"""

    source: str
    snapshot: str
    sha256: str
    license: str
    ud_release: str
    treebanks: dict[str, str]
    word_forms: Required[int]
    checksum: Required[int]
