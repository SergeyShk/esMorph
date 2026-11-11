# Spanish morphological analyzer (esMorph)
#
# Copyright (C) 2026
# Author: Sergey Shkarin <kouki.sergey@gmail.com>
# URL: <https://github.com/SergeyShk/esMorph>

"""
The esmorph command: morphological analysis of text from files and the standard input

Example:
    esmorph --format conllu text.txt
    python -m esmorph --help
"""

import signal
import sys

from ._esmorph import _main


def main() -> None:
    """
    Entry point of the esmorph command

    Description:
        The core in Rust parses the arguments, reads the files and writes the result; Ctrl+C ends
        the command at once, as any program, and an ignored SIGINT (a command in the background of
        a script) stays ignored

    Raises:
        SystemExit: Always, with the exit code of the command
    """
    if signal.getsignal(signal.SIGINT) is signal.default_int_handler:
        signal.signal(signal.SIGINT, signal.SIG_DFL)
    sys.exit(_main(sys.argv[1:]))


if __name__ == "__main__":
    main()
