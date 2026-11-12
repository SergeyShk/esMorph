import json
import shutil
import signal
import subprocess
import sys
import threading
from pathlib import Path

import pytest
from anyseg import Segmenter

import esmorph
from esmorph import Analyzer, __main__ as command

TEXT = "Dime la verdad del caso.\n"


def run(*args: str, text: str = "") -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [sys.executable, "-m", "esmorph", *args],
        input=text,
        capture_output=True,
        encoding="utf-8",
        check=False,
    )


def test_lemmas():
    result = run(text=TEXT + "\nVoy a verlas.\n")
    assert (result.returncode, result.stderr) == (0, "")
    assert result.stdout == "decir yo el verdad de el caso .\n\nir a ver él .\n"


def test_formats(tmp_path):
    path = tmp_path / "text.txt"
    path.write_text(TEXT, encoding="utf-8")
    tsv = run("--format", "tsv", str(path)).stdout.splitlines()
    assert tsv[0].split("\t")[:3] == ["Di", "decir", "VERB"]
    # CoNLL-U is the document of anySeg with the columns of annotate
    document = Segmenter("es").segment(TEXT)
    Analyzer().annotate(document)
    assert run("-f", "conllu", str(path)).stdout == document.to_conllu()
    sentence = json.loads(run("-f", "jsonl", str(path)).stdout)
    assert sentence["text"] == TEXT.strip()
    analyses = sentence["tokens"][0]["analyses"]
    # The context model puts the verb first; the dictionary orders by the counts of treebanks
    assert [(analysis["lemma"], analysis["pos"]) for analysis in analyses] == [
        ("decir", "VERB"),
        ("Dime", "PROPN"),
    ]
    assert analyses[0]["words"] == ["di", "me"]
    assert analyses[0]["feats"]["Mood"] == "Imp"
    assert analyses[0]["source"] == "clitics" and 0.5 < analyses[0]["prob"] <= 1
    sentence = json.loads(run("-f", "jsonl", "--no-context", str(path)).stdout)
    analyses = sentence["tokens"][0]["analyses"]
    assert [(analysis["lemma"], analysis["pos"]) for analysis in analyses] == [
        ("Dime", "PROPN"),
        ("decir", "VERB"),
    ]


def test_tokens_and_conllu_input():
    assert run("-i", "tokens", text="dime\nla\n\nverdad\n").stdout == "decir el\nverdad\n"
    conllu = "# text = Dime\n1-2\tDime\t_\t_\t_\t_\t_\t_\t_\t_\n1\tDi\t_\t_\t_\t_\t0\troot\t_\t_\n2\tme\t_\t_\t_\t_\t1\tiobj\t_\t_\n"
    lines = run("-i", "conllu", "-f", "conllu", text=conllu).stdout.splitlines()
    assert lines[2].split("\t")[:4] == ["1", "Di", "decir", "VERB"]
    columns = lines[3].split("\t")
    assert (columns[:4], columns[6:8]) == (["2", "me", "yo", "PRON"], ["1", "iobj"])


def test_version_and_help():
    assert run("--version").stdout == f"esmorph {esmorph.__version__}\n"
    assert run("--help").stdout.startswith("esmorph - morphological analysis of Spanish text")


def test_errors(tmp_path):
    result = run("--format", "xml")
    assert result.returncode == 2
    assert result.stderr == (
        "esmorph: unknown format xml, expected one of: lemmas, conllu, tsv, jsonl\n"
        "Help: esmorph --help\n"
    )
    missing = tmp_path / "missing.txt"
    result = run(str(missing))
    assert result.returncode == 1
    assert result.stderr == f"esmorph: {missing}: no such file\n"
    broken = tmp_path / "broken.dict"
    broken.write_bytes(b"not a dictionary")
    result = run("--dictionary", str(broken), text=TEXT)
    assert result.returncode == 1
    assert result.stderr.startswith(f"esmorph: {broken}: wrong dictionary format")


def test_dictionary_option(tmp_path):
    path = tmp_path / "es.dict"
    shutil.copy(Analyzer().path, path)
    assert run("-d", str(path), text=TEXT).stdout == "decir yo el verdad de el caso .\n"


def test_closed_pipe(tmp_path):
    path = tmp_path / "text.txt"
    path.write_text(TEXT * 20_000, encoding="utf-8")
    process = subprocess.Popen(
        [sys.executable, "-m", "esmorph", str(path)],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    assert process.stdout is not None and process.stderr is not None
    assert process.stdout.readline().decode() == "decir yo el verdad de el caso .\n"
    process.stdout.close()
    assert process.wait(timeout=60) == 0
    assert process.stderr.read() == b""


def test_console_script():
    script = shutil.which("esmorph", path=str(Path(sys.executable).parent))
    assert script is not None
    result = subprocess.run([script, "-V"], capture_output=True, encoding="utf-8", check=False)
    assert result.stdout == f"esmorph {esmorph.__version__}\n"


def test_main_in_process(tmp_path, monkeypatch, capfd):
    path = tmp_path / "text.txt"
    path.write_text(TEXT, encoding="utf-8")
    handlers = []
    monkeypatch.setattr(signal, "signal", lambda *args: handlers.append(args))
    monkeypatch.setattr(sys, "argv", ["esmorph", str(path)])
    with pytest.raises(SystemExit) as exit_info:
        command.main()
    assert exit_info.value.code == 0
    assert handlers == [(signal.SIGINT, signal.SIG_DFL)]
    assert capfd.readouterr().out == "decir yo el verdad de el caso .\n"
    # An ignored SIGINT stays ignored
    handlers.clear()
    monkeypatch.setattr(signal, "getsignal", lambda _: signal.SIG_IGN)
    with pytest.raises(SystemExit):
        command.main()
    assert handlers == []


def test_paragraph_by_paragraph():
    # The command answers every paragraph without waiting for the end of the input: it can be
    # kept open
    process = subprocess.Popen(
        [sys.executable, "-m", "esmorph"],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        encoding="utf-8",
    )
    assert process.stdin is not None and process.stdout is not None
    stdout = process.stdout
    try:
        # A blank line goes before the second paragraph
        for expected in (["decir yo\n"], ["\n", "decir yo\n"]):
            process.stdin.write("Dime\n\n")
            process.stdin.flush()
            lines: list[str] = []

            def read(out: list[str] = lines, count: int = len(expected)) -> None:
                out.extend(stdout.readline() for _ in range(count))

            reader = threading.Thread(target=read)
            reader.start()
            reader.join(timeout=60)
            assert lines == expected
    finally:
        process.stdin.close()
        process.wait(timeout=60)


def test_output_file_and_encoding(tmp_path):
    path, output = tmp_path / "text.txt", tmp_path / "text.conllu"
    path.write_text(TEXT, encoding="utf-8")
    result = run("-f", "conllu", "-o", str(output), str(path))
    assert (result.returncode, result.stdout, result.stderr) == (0, "", "")
    assert (
        output.read_text(encoding="utf-8").splitlines()[3] == "# text = Dime la verdad del caso."
    )
    # The standard input from the output file: the file is not overwritten
    if sys.platform != "win32":
        with path.open("rb") as stdin:
            result = subprocess.run(
                [sys.executable, "-m", "esmorph", "-o", str(path)],
                stdin=stdin,
                capture_output=True,
                check=False,
            )
        assert result.returncode == 1
        assert path.read_text(encoding="utf-8") == TEXT
    # The standard output appended to an input file or the dictionary: they are not grown
    dictionary = tmp_path / "es.dict"
    shutil.copy(Analyzer().path, dictionary)
    cases = [([str(path)], path), (["-d", str(dictionary), str(path)], dictionary)]
    if sys.platform != "win32":
        # /dev/stdout of a stream open only for writing
        cases.append((["-o", "/dev/stdout", str(path)], path))
    for args, appended in cases:
        with appended.open("ab") as stdout:
            result = subprocess.run(
                [sys.executable, "-m", "esmorph", *args],
                stdout=stdout,
                stderr=subprocess.PIPE,
                timeout=60,
                check=False,
            )
        assert result.returncode == 1
        assert result.stderr.endswith(b"is an input file or the dictionary\n")
    assert path.read_text(encoding="utf-8") == TEXT
    assert dictionary.read_bytes() == Path(Analyzer().path).read_bytes()
    # A file in cp1252 with ASCII at the start (subtitles) is not analyzed, not even in part
    subtitles = tmp_path / "cp1252.srt"
    subtitles.write_bytes("1\n00:00:01,000 --> 00:00:02,000\nDime la canción\n".encode("cp1252"))
    result = run(str(subtitles))
    assert (result.returncode, result.stdout) == (1, "")
    assert result.stderr == f"esmorph: {subtitles}: the file is not in UTF-8 (line 3)\n"
