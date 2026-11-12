"""Every English documentation page has a Spanish twin with the same headings, anchors and links,
and the examples of every page give the shown results"""

import ast
import contextlib
import io
import json
import math
import re
import shlex
import subprocess
import sys
import textwrap
from pathlib import Path

import pytest

DOCS = Path(__file__).parents[1] / "docs"
PAGES = sorted(path for path in DOCS.rglob("*.md") if not path.name.endswith(".es.md"))
ALL_PAGES = sorted(DOCS.rglob("*.md"))


def anchors(text: str) -> set[str]:
    return set(re.findall(r"\{ #([\w-]+) \}", text))


def links(text: str) -> set[str]:
    return set(re.findall(r"\]\(([^)#\s]+\.md)", text))


@pytest.mark.parametrize("page", PAGES, ids=[str(page.relative_to(DOCS)) for page in PAGES])
def test_spanish_page(page):
    spanish = page.with_name(page.name[:-3] + ".es.md")
    assert spanish.is_file(), f"missing Spanish version {spanish.relative_to(DOCS)}"
    english_text = page.read_text(encoding="utf-8")
    spanish_text = spanish.read_text(encoding="utf-8")
    assert anchors(spanish_text) == anchors(english_text)
    assert links(spanish_text) == links(english_text)
    assert english_text.count("\n## ") == spanish_text.count("\n## ")


def test_no_orphan_spanish_pages():
    spanish = {path.relative_to(DOCS) for path in DOCS.rglob("*.es.md")}
    assert spanish == {page.relative_to(DOCS).with_suffix(".es.md") for page in PAGES}


def example_pattern(code: str, result: str) -> re.Pattern[str]:
    return re.compile(
        rf'=== "{code}"\n\n    ``` python\n(.*?)    ```\n\n=== "{result}"\n\n    ```\n(.*?)    ```',
        re.S,
    )


EXAMPLES = example_pattern("Code", "Result")
SPANISH_EXAMPLES = example_pattern("Código", "Resultado")


def run(code: str, namespace: dict) -> str:
    """What an example prints: the value of an expression as the interactive interpreter shows it"""
    output = io.StringIO()
    with contextlib.redirect_stdout(output):
        for node in ast.parse(code).body:
            if isinstance(node, ast.Expr):
                value = eval(compile(ast.Expression(node.value), "<example>", "eval"), namespace)
                if value is not None:
                    print(repr(value))
            else:
                exec(compile(ast.Module([node], []), "<example>", "exec"), namespace)
    return output.getvalue()


@pytest.mark.parametrize(
    "page", ALL_PAGES, ids=[str(page.relative_to(DOCS)) for page in ALL_PAGES]
)
def test_examples(page):
    pattern = SPANISH_EXAMPLES if page.name.endswith(".es.md") else EXAMPLES
    text = page.read_text(encoding="utf-8")
    # spaCy has no wheels for the free-threaded Python
    if "import spacy" in text:
        pytest.importorskip("spacy")
    namespace: dict = {}
    for code, result in pattern.findall(text):
        code, result = textwrap.dedent(code), textwrap.dedent(result)
        # Long results are wrapped on the page
        assert re.sub(r"\s+", "", run(code, namespace)) == re.sub(r"\s+", "", result), code


COMMANDS = re.compile(
    r"``` bash\n((?:echo|printf) [^\n]*\| esmorph[^\n]*)\n```\n\n``` ?(\w*)\n(.*?)```", re.S
)


def same(found: object, shown: object) -> bool:
    """Equal values, numbers up to the last digits, which differ between processors"""
    if isinstance(found, float | int) and isinstance(shown, float | int):
        return math.isclose(found, shown, rel_tol=0, abs_tol=1e-5)
    if isinstance(found, dict) and isinstance(shown, dict):
        return found.keys() == shown.keys() and all(same(found[k], shown[k]) for k in found)
    if isinstance(found, list) and isinstance(shown, list):
        return len(found) == len(shown) and all(map(same, found, shown))
    return found == shown


def number(field: str) -> float | str:
    try:
        return float(field)
    except ValueError:
        return field


@pytest.mark.parametrize(
    "page", ALL_PAGES, ids=[str(page.relative_to(DOCS)) for page in ALL_PAGES]
)
def test_commands(page):
    for command, kind, shown in COMMANDS.findall(page.read_text(encoding="utf-8")):
        source, text, pipe, program, *args = shlex.split(command)
        assert (pipe, program) == ("|", "esmorph"), command
        text = (
            text.replace("\\t", "\t").replace("\\n", "\n") if source == "printf" else text + "\n"
        )
        found = subprocess.run(
            [sys.executable, "-X", "utf8", "-m", "esmorph", *args],
            input=text.encode(),
            capture_output=True,
            check=True,
        ).stdout.decode()
        if kind == "json":
            assert same(json.loads(found), json.loads(shown)), (command, found)
            continue
        # A page leaves out the blank line after the last sentence
        rows, expected = (
            [[number(field) for field in line.split("\t")] for line in text.rstrip().splitlines()]
            for text in (found, shown)
        )
        assert same(rows, expected), (command, found)
