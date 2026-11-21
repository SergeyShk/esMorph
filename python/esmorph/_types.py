# Spanish morphological analyzer (esMorph)
#
# Copyright (C) 2026
# Author: Sergey Shkarin <kouki.sergey@gmail.com>
# URL: <https://github.com/SergeyShk/esMorph>

"""Types of the values of the analyzer"""

from typing import Required, TypedDict


class ContextInfo(TypedDict, total=False):
    """Data of the training of the context model of a dictionary"""

    ud_release: str
    treebanks: dict[str, str]


class ParadigmsInfo(TypedDict, total=False):
    """Data of the training of the paradigm model of a dictionary"""

    snapshots: dict[str, str]
    words: int


class DictionaryInfo(TypedDict, total=False):
    """Description of a dictionary, as Analyzer.dictionary_info gives it"""

    source: str
    snapshot: str
    sha256: str
    snapshots: dict[str, str]
    license: str
    ud_release: str
    treebanks: dict[str, str]
    context: ContextInfo
    paradigms: ParadigmsInfo
    word_forms: Required[int]
    checksum: Required[int]
