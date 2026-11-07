"""
Licenses of the Rust crates built into the Python module: THIRD-PARTY-NOTICES.txt

Description:
    The crates are the normal dependencies of `esmorph-py` on any platform, without procedural
    macros and the crates of the workspace. A crate with a choice of licenses gives its MIT
    license (a crate with one license gives that one) and the license of the Unicode data.
    After a change of Cargo.lock the file is written again;
    `tests/test_package.py::test_third_party_notices` checks that they match.

Example:
    uv run python scripts/notices.py
"""

import json
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).parents[1]
HEADER = [
    "Licenses of the Rust crates linked into the Python module of esMorph. Where a crate offers a",
    "choice of licenses, the MIT license is the one used here, with the Unicode license of the Unicode",
    "data.",
    "",
]
# The files of the MIT license and of the Unicode data; a crate without them has one license
TEXTS = ("LICENSE-MIT", "LICENSE-UNICODE", "COPYRIGHT")
SINGLE = ("LICENSE", "LICENSE.txt", "LICENSE.md")


def linked(metadata: dict) -> list[dict]:
    """
    Crates of crates.io that the module builds on some platform, by name

    Arguments:
        metadata (dict): Output of `cargo metadata`

    Returns:
        list[dict]: Packages of the crates
    """
    packages = {package["id"]: package for package in metadata["packages"]}
    nodes = {node["id"]: node for node in metadata["resolve"]["nodes"]}
    root = next(
        package["id"] for package in metadata["packages"] if package["name"] == "esmorph-py"
    )
    seen, todo = set(), [root]
    while todo:
        for dependency in nodes[todo.pop()]["deps"]:
            package = packages[dependency["pkg"]]
            normal = any(kind["kind"] is None for kind in dependency["dep_kinds"])
            macro = any("proc-macro" in target["kind"] for target in package["targets"])
            if normal and not macro and package["id"] not in seen:
                seen.add(package["id"])
                todo.append(package["id"])
    crates = [packages[package] for package in seen if packages[package]["source"] is not None]
    return sorted(crates, key=lambda package: package["name"])


def notices() -> str:
    """
    Text of the file

    Returns:
        str: Header and licenses of the crates

    Raises:
        ValueError: If a crate has no license text
    """
    metadata = json.loads(
        subprocess.run(
            ["cargo", "metadata", "--format-version", "1", "--locked"],
            cwd=ROOT,
            capture_output=True,
            check=True,
        ).stdout
    )
    lines = list(HEADER)
    for package in linked(metadata):
        folder = Path(package["manifest_path"]).parent
        files = sorted(folder.iterdir())
        texts = [path for path in files if path.name.upper().startswith(TEXTS)]
        texts = texts or [path for path in files if path.name in SINGLE]
        if not texts:
            raise ValueError(f"{package['name']}: no license text in {folder}")
        rule = "=" * 100
        title = f"{package['name']} {package['version']} ({package['license']})"
        lines += [rule, f"{title}: {package.get('repository') or ''}", rule, ""]
        for path in texts:
            lines += [path.read_text(encoding="utf-8").strip(), ""]
    text = "\n".join(line.rstrip() for line in "\n".join(lines).splitlines())
    return text.rstrip() + "\n"


if __name__ == "__main__":
    path = Path(sys.argv[1]) if len(sys.argv) > 1 else ROOT / "THIRD-PARTY-NOTICES.txt"
    # LF on any system, as in the file of the repository
    path.write_text(notices(), encoding="utf-8", newline="\n")
