import importlib
import importlib.metadata
import re
import shutil
import subprocess
import sys
import sysconfig
import tomllib
import zipfile
from pathlib import Path

import pytest

import esmorph

ROOT = Path(__file__).parents[1]


def test_version():
    assert re.fullmatch(r"\d+\.\d+\.\d+(\.dev\d+)?", esmorph.__version__)
    assert esmorph.__version__ != "0.0.0"


def test_metadata():
    assert "morphological analyzer" in esmorph.__description__
    assert "__version__" in esmorph.__all__
    assert esmorph.__all__ == sorted(esmorph.__all__)


def test_dictionary_licenses_match():
    root, crate = (
        ROOT / "LICENSE-DICTIONARY.txt",
        ROOT / "crates/esmorph-dict-es/LICENSE-DICTIONARY.txt",
    )
    assert root.read_bytes() == crate.read_bytes()


def test_version_matches_metadata():
    # The version comes from the extension module, without slow importlib.metadata on import
    assert esmorph.__version__ == importlib.metadata.version("esmorph")


def test_crate_version():
    with (ROOT / "Cargo.toml").open("rb") as file:
        crate = tomllib.load(file)["workspace"]["package"]["version"]
    with (ROOT / "pyproject.toml").open("rb") as file:
        package = tomllib.load(file)["project"]["version"]
    assert crate == re.sub(r"\.dev(\d+)$", r"-dev.\1", package)
    # The dictionary format changes with the core: esmorph requires the dictionary of its version
    with (ROOT / "crates" / "esmorph" / "Cargo.toml").open("rb") as file:
        dictionary = tomllib.load(file)["dependencies"]["esmorph-dict-es"]["version"]
    assert dictionary == f"={crate}"


def test_extension():
    extension = importlib.import_module("esmorph._esmorph")
    assert Path(extension.__file__).parent == Path(esmorph.__file__).parent


def test_package_data():
    package = Path(esmorph.__file__).parent
    assert (package / "py.typed").is_file()
    assert (package / "data" / "es.dict").is_file()


@pytest.mark.skipif(shutil.which("uv") is None, reason="uv builds the wheel")
def test_wheel_contents(tmp_path):
    subprocess.run(
        [
            "uv",
            "build",
            "--wheel",
            "--python",
            sys.executable,
            "--out-dir",
            str(tmp_path),
            str(ROOT),
        ],
        capture_output=True,
        check=True,
    )
    (wheel,) = tmp_path.glob("*.whl")
    assert ("-abi3-" in wheel.name) != bool(sysconfig.get_config_var("Py_GIL_DISABLED"))
    with zipfile.ZipFile(wheel) as archive:
        names = set(archive.namelist())
    assert {"esmorph/py.typed", "esmorph/data/es.dict"} <= names
    modules = [
        name for name in names if re.fullmatch(r"esmorph/_esmorph(\.[\w-]+)?\.(so|pyd)", name)
    ]
    assert len(modules) == 1
    assert all(name.startswith(("esmorph/", "esmorph-")) for name in names)
    licenses = {name.rsplit("/", 1)[1] for name in names if "/licenses/" in name}
    assert licenses == {"LICENSE.txt", "LICENSE-DICTIONARY.txt", "THIRD-PARTY-NOTICES.txt"}


@pytest.mark.skipif(shutil.which("cargo") is None, reason="cargo reads the crates")
def test_third_party_notices(tmp_path):
    # The licenses of the crates of the module follow Cargo.lock: after a change run python scripts/notices.py
    path = tmp_path / "notices.txt"
    subprocess.run(
        [sys.executable, str(ROOT / "scripts" / "notices.py"), str(path)],
        capture_output=True,
        check=True,
    )
    expected = (ROOT / "THIRD-PARTY-NOTICES.txt").read_text(encoding="utf-8")
    assert path.read_text(encoding="utf-8") == expected, "python scripts/notices.py"
