"""Every English documentation page has a Spanish twin with the same headings, anchors and links"""

import re
from pathlib import Path

import pytest

DOCS = Path(__file__).parents[1] / "docs"
PAGES = sorted(path for path in DOCS.rglob("*.md") if not path.name.endswith(".es.md"))


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
