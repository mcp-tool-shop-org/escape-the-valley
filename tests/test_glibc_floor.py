"""Unit tests for scripts/check_glibc_floor.py and clean-container CLI smoke."""

from __future__ import annotations

import importlib.util
import os
import subprocess
import sys
from pathlib import Path

import pytest
from typer.testing import CliRunner

from escape_the_valley.cli import app

REPO_ROOT = Path(__file__).resolve().parents[1]
SCRIPT_PATH = REPO_ROOT / "scripts" / "check_glibc_floor.py"


def _load_script_module():
    spec = importlib.util.spec_from_file_location("check_glibc_floor", SCRIPT_PATH)
    assert spec is not None and spec.loader is not None
    mod = importlib.util.module_from_spec(spec)
    sys.modules["check_glibc_floor"] = mod
    spec.loader.exec_module(mod)
    return mod


FAKE_OBJDUMP_OK = """\
libpython3.12.so.1.0:     file format elf64-x86-64

DYNAMIC SYMBOL TABLE:
0000000000000000      DF *UND*\t0000000000000000  GLIBC_2.2.5 free
0000000000000000      DF *UND*\t0000000000000000  GLIBC_2.14  memcpy
0000000000000000      DF *UND*\t0000000000000000  GLIBC_2.17  secure_getenv
"""

FAKE_OBJDUMP_AT_CEILING = """\
libfoo.so:     file format elf64-x86-64

DYNAMIC SYMBOL TABLE:
0000000000000000      DF *UND*\t0000000000000000  GLIBC_2.35  __libc_start_main
"""

FAKE_OBJDUMP_ABOVE_CEILING = """\
_bad_ext.cpython-312-x86_64-linux-gnu.so:     file format elf64-x86-64

DYNAMIC SYMBOL TABLE:
0000000000000000      DF *UND*\t0000000000000000  GLIBC_2.14  memcpy
0000000000000000      DF *UND*\t0000000000000000  GLIBC_2.38  __isoc23_strtol
0000000000000000      DF *UND*\t0000000000000000  GLIBC_2.43  __ newest_sym
"""

FAKE_OBJDUMP_NO_GLIBC = """\
static_or_unversioned:     file format elf64-x86-64

DYNAMIC SYMBOL TABLE:
no symbols
"""


def test_parse_and_check_objdump_floor_accepts_up_to_2_35():
    mod = _load_script_module()
    assert mod.parse_glibc_versions(FAKE_OBJDUMP_OK) == {(2, 2), (2, 14), (2, 17)}
    assert mod.check_objdump_floor("libpython3.12.so.1.0", FAKE_OBJDUMP_OK) == (2, 17)
    assert mod.check_objdump_floor("libfoo.so", FAKE_OBJDUMP_AT_CEILING) == (2, 35)


def test_check_objdump_floor_refuses_version_above_2_35():
    mod = _load_script_module()
    with pytest.raises(RuntimeError, match=r"GLIBC_2\.38, GLIBC_2\.43.*ceiling GLIBC_2\.35"):
        mod.check_objdump_floor(
            "_bad_ext.cpython-312-x86_64-linux-gnu.so",
            FAKE_OBJDUMP_ABOVE_CEILING,
        )


def test_inspect_binary_glibc_floor_refuses_fake_objdump_above_ceiling(tmp_path):
    mod = _load_script_module()
    fake_bin = tmp_path / "escape-the-valley"
    fake_bin.write_bytes(b"\x7fELFfake-bootloader")

    outputs = {
        "libpython3.12.so.1.0": FAKE_OBJDUMP_OK,
        "Crypto/Cipher/_raw_aes.abi3.so": FAKE_OBJDUMP_ABOVE_CEILING,
    }

    def fake_runner(path: Path) -> str:
        if path == fake_bin:
            return FAKE_OBJDUMP_OK
        for key, text in outputs.items():
            if path.name.endswith(Path(key).name):
                return text
        raise AssertionError(f"Unexpected path: {path}")

    with pytest.raises(
        RuntimeError,
        match=r"Crypto/Cipher/_raw_aes\.abi3\.so requires GLIBC_2\.38, GLIBC_2\.43",
    ):
        mod.inspect_binary_glibc_floor(
            fake_bin,
            objdump_runner=fake_runner,
            archive_so_payloads={
                "libpython3.12.so.1.0": b"\x7fELFso1",
                "Crypto/Cipher/_raw_aes.abi3.so": b"\x7fELFso2",
            },
        )


def test_cli_main_exits_nonzero_on_violation_and_zero_when_clean(tmp_path, monkeypatch, capsys):
    mod = _load_script_module()
    fake_bin = tmp_path / "escape-the-valley"
    fake_bin.write_bytes(b"\x7fELFfake-bootloader")

    class FakeReader:
        def __init__(self, path: str):
            self.toc = {
                "libpython3.12.so.1.0": (0, 0, 0, 0, "b"),
                "Crypto/Cipher/_raw_aes.abi3.so": (0, 0, 0, 0, "b"),
                "escape_the_valley/tui.tcss": (0, 0, 0, 0, "x"),
            }

        def extract(self, name: str) -> bytes:
            if name.endswith(".tcss"):
                return b"Screen { background: black; }"
            return b"\x7fELF" + f"payload:{name}".encode()

    class FakeReadersMod:
        CArchiveReader = FakeReader

    monkeypatch.setattr(mod.importlib, "import_module", lambda name: FakeReadersMod)

    # 1. Above ceiling -> refuses with exit code 1
    monkeypatch.setattr(
        mod,
        "_default_objdump_runner",
        lambda p: FAKE_OBJDUMP_ABOVE_CEILING if "_raw_aes" in p.name else FAKE_OBJDUMP_OK,
    )
    rc_fail = mod.main([str(fake_bin)])
    captured_fail = capsys.readouterr()
    assert rc_fail == 1
    assert "GLIBC FLOOR CHECK FAILED" in captured_fail.err
    assert "GLIBC_2.38" in captured_fail.err

    # 2. Within ceiling -> passes with exit code 0 and prints floor table
    monkeypatch.setattr(mod, "_default_objdump_runner", lambda p: FAKE_OBJDUMP_OK)
    rc_ok = mod.main([str(fake_bin)])
    captured_ok = capsys.readouterr()
    assert rc_ok == 0
    assert "| GLIBC_2.17 | 2 (plus the bootloader) |" in captured_ok.out


def test_every_entry_zero_glibc_versions_exits_one_with_reason(tmp_path, monkeypatch, capsys):
    """NB-2: zero GLIBC_ versions across all inspected entries fails closed (exit 1)."""
    mod = _load_script_module()
    fake_bin = tmp_path / "escape-the-valley"
    fake_bin.write_bytes(b"\x7fELFfake-bootloader")

    class FakeReader:
        def __init__(self, path: str):
            self.toc = {
                "libpython3.12.so.1.0": (0, 0, 0, 0, "b"),
                "vendor/libstatic.so": (0, 0, 0, 0, "b"),
            }

        def extract(self, name: str) -> bytes:
            return b"\x7fELF" + name.encode()

    class FakeReadersMod:
        CArchiveReader = FakeReader

    monkeypatch.setattr(mod.importlib, "import_module", lambda name: FakeReadersMod)

    orig_default_runner = mod._default_objdump_runner

    # Single versionless ELF alongside versioned ELFs is reported in table, not failed.
    monkeypatch.setattr(
        mod,
        "_default_objdump_runner",
        lambda p: FAKE_OBJDUMP_NO_GLIBC if "libstatic" in p.name else FAKE_OBJDUMP_OK,
    )
    rc_mixed = mod.main([str(fake_bin)])
    captured_mixed = capsys.readouterr()
    assert rc_mixed == 0
    assert "| none | 1 |" in captured_mixed.out

    # Every inspected entry yields zero GLIBC_ versions -> exit 1 with named reason.
    monkeypatch.setattr(mod, "_default_objdump_runner", lambda p: FAKE_OBJDUMP_NO_GLIBC)
    rc_zero = mod.main([str(fake_bin)])
    captured_zero = capsys.readouterr()
    assert rc_zero == 1
    assert "GLIBC FLOOR CHECK FAILED" in captured_zero.err
    assert "no_glibc_versions" in captured_zero.err

    with pytest.raises(RuntimeError, match="no_glibc_versions"):
        mod.inspect_binary_glibc_floor(
            fake_bin,
            objdump_runner=lambda p: FAKE_OBJDUMP_NO_GLIBC,
            archive_so_payloads={"libpython3.12.so.1.0": b"\x7fELFso"},
        )

    # Missing objdump executable -> exit 1 with named reason no_objdump.
    monkeypatch.setattr(mod, "_default_objdump_runner", orig_default_runner)

    def raise_missing_objdump(*args, **kwargs):
        raise FileNotFoundError(2, "No such file or directory", "objdump")

    monkeypatch.setattr(mod.subprocess, "run", raise_missing_objdump)
    rc_no_bin = mod.main([str(fake_bin)])
    captured_no_bin = capsys.readouterr()
    assert rc_no_bin == 1
    assert "no_objdump" in captured_no_bin.err


def test_elf_member_with_non_so_name_is_inspected(tmp_path, monkeypatch, capsys):
    """NB-1: an ELF archive member without `.so` in its name is selected by ELF magic."""
    mod = _load_script_module()
    fake_bin = tmp_path / "escape-the-valley"
    fake_bin.write_bytes(b"\x7fELFfake-bootloader")

    class FakeReader:
        def __init__(self, path: str):
            self.toc = {
                "libpython3.12.so.1.0": (0, 0, 0, 0, "b"),
                "libexec/etv-helper": (0, 0, 0, 0, "x"),
                "PYZ-00.pyz": (0, 0, 0, 0, "z"),
            }

        def extract(self, name: str) -> bytes:
            if name == "PYZ-00.pyz":
                raise AssertionError("PYZ archive should not be extracted as flat file")
            return b"\x7fELF" + name.encode()

    class FakeReadersMod:
        CArchiveReader = FakeReader

    monkeypatch.setattr(mod.importlib, "import_module", lambda name: FakeReadersMod)

    # 1. Non-.so ELF above ceiling is caught and fails the check.
    monkeypatch.setattr(
        mod,
        "_default_objdump_runner",
        lambda p: FAKE_OBJDUMP_ABOVE_CEILING if "etv-helper" in p.name else FAKE_OBJDUMP_OK,
    )
    rc_bad = mod.main([str(fake_bin)])
    captured_bad = capsys.readouterr()
    assert rc_bad == 1
    assert "libexec/etv-helper" in captured_bad.err
    assert "GLIBC_2.38" in captured_bad.err

    # 2. Clean non-.so ELF as the sole archive member is inspected and reported.
    results = mod.inspect_binary_glibc_floor(
        fake_bin,
        objdump_runner=lambda p: FAKE_OBJDUMP_OK,
        archive_so_payloads={"libexec/etv-helper": b"\x7fELFhelper"},
    )
    assert "libexec/etv-helper" in results
    assert results["libexec/etv-helper"] == (2, 17)
    report = mod.format_floor_report(results)
    assert "inspected non-.so ELF member(s): libexec/etv-helper" in report


def test_non_elf_member_named_so_is_not_passed_off_as_inspected(tmp_path, monkeypatch, capsys):
    """NB-1: a non-ELF archive member named `*.so` is not treated as an inspected ELF."""
    mod = _load_script_module()
    fake_bin = tmp_path / "escape-the-valley"
    fake_bin.write_bytes(b"\x7fELFfake-bootloader")

    inspected_paths: list[str] = []

    def tracking_runner(path: Path) -> str:
        inspected_paths.append(path.name)
        return FAKE_OBJDUMP_OK

    skipped: list[str] = []
    results = mod.inspect_binary_glibc_floor(
        fake_bin,
        objdump_runner=tracking_runner,
        archive_so_payloads={
            "libpython3.12.so.1.0": b"\x7fELFreal-so",
            "vendor/linker_script.so": b"/* GNU ld script */\nGROUP ( libfoo.so.1 )\n",
        },
        skipped_so_named_out=skipped,
    )
    assert "libpython3.12.so.1.0" in results
    assert "vendor/linker_script.so" not in results
    assert not any("linker_script.so" in name for name in inspected_paths)
    assert skipped == ["vendor/linker_script.so"]

    report = mod.format_floor_report(results, skipped_so_named=skipped)
    assert "Inspected 1 bundled ELF file(s)" in report
    assert "skipped non-ELF .so-named member(s): vendor/linker_script.so" in report

    # If the archive has ONLY a non-ELF `*.so`, check fails closed with no_elf_entries.
    class FakeOnlyNonElfSoReader:
        def __init__(self, path: str):
            self.toc = {"vendor/linker_script.so": (0, 0, 0, 0, "b")}

        def extract(self, name: str) -> bytes:
            return b"/* GNU ld script */\n"

    class FakeReadersMod:
        CArchiveReader = FakeOnlyNonElfSoReader

    monkeypatch.setattr(mod.importlib, "import_module", lambda name: FakeReadersMod)
    monkeypatch.setattr(mod, "_default_objdump_runner", lambda p: FAKE_OBJDUMP_OK)
    rc = mod.main([str(fake_bin)])
    captured = capsys.readouterr()
    assert rc == 1
    assert "no_elf_entries" in captured.err
    assert "vendor/linker_script.so" in captured.err


def test_cli_selfcheck_imports_textual_and_xrpl_with_stdin_closed(tmp_path, monkeypatch):
    """NB-4: `trail --selfcheck` loads textual and xrpl offline with stdin closed and exits 0."""
    env = dict(os.environ)
    src_dir = str(REPO_ROOT / "src")
    existing_py_path = env.get("PYTHONPATH", "")
    env["PYTHONPATH"] = (
        f"{src_dir}{os.pathsep}{existing_py_path}" if existing_py_path else src_dir
    )

    # 1. Clean interpreter with stdin=/dev/null AND closed sys.stdin: proves lazy
    #    extensions are absent before --selfcheck, imported by --selfcheck, and exit 0.
    probe_code = (
        "import sys\n"
        "assert 'textual' not in sys.modules\n"
        "assert 'xrpl' not in sys.modules\n"
        "sys.stdin.close()\n"
        "assert sys.stdin.closed\n"
        "from escape_the_valley.cli import app\n"
        "assert 'textual' not in sys.modules\n"
        "assert 'xrpl' not in sys.modules\n"
        "try:\n"
        "    app(['--selfcheck'])\n"
        "except SystemExit as exc:\n"
        "    assert exc.code in (0, None), f'unexpected exit code: {exc.code}'\n"
        "else:\n"
        "    raise AssertionError('expected SystemExit(0)')\n"
        "assert 'textual' in sys.modules\n"
        "assert 'textual.app' in sys.modules\n"
        "assert 'xrpl' in sys.modules\n"
        "assert 'xrpl.wallet' in sys.modules\n"
    )
    proc = subprocess.run(
        [sys.executable, "-c", probe_code],
        stdin=subprocess.DEVNULL,
        capture_output=True,
        text=True,
        encoding="utf-8",
        timeout=60,
        env=env,
        check=False,
    )
    assert proc.returncode == 0, f"stdout={proc.stdout!r} stderr={proc.stderr!r}"
    assert "selfcheck ok:" in proc.stdout
    assert "textual=" in proc.stdout
    assert "xrpl=" in proc.stdout

    # 2. Entrypoint `python -m escape_the_valley --selfcheck` with stdin closed (/dev/null).
    proc_entry = subprocess.run(
        [sys.executable, "-m", "escape_the_valley", "--selfcheck"],
        stdin=subprocess.DEVNULL,
        capture_output=True,
        text=True,
        encoding="utf-8",
        timeout=60,
        env=env,
        check=False,
    )
    assert proc_entry.returncode == 0, (
        f"stdout={proc_entry.stdout!r} stderr={proc_entry.stderr!r}"
    )
    assert "selfcheck ok:" in proc_entry.stdout

    # 3. In-process CliRunner exercise (both clean pass and failure branches).
    cli_runner = CliRunner()
    res_ok = cli_runner.invoke(app, ["--selfcheck"])
    assert res_ok.exit_code == 0
    assert "selfcheck ok:" in res_ok.output
    assert "textual" in sys.modules
    assert "xrpl" in sys.modules

    monkeypatch.setattr("escape_the_valley.backpack._HAS_XRPL", False)
    res_no_xrpl = cli_runner.invoke(app, ["--selfcheck"])
    assert res_no_xrpl.exit_code == 1

    monkeypatch.setattr("escape_the_valley.backpack._HAS_XRPL", True)
    from escape_the_valley.tui_app import LedgerTrailApp

    orig_css_path = LedgerTrailApp.CSS_PATH
    missing_css = tmp_path / "missing.tcss"
    monkeypatch.setattr(LedgerTrailApp, "CSS_PATH", str(missing_css))
    res_missing_css = cli_runner.invoke(app, ["--selfcheck"])
    assert res_missing_css.exit_code == 1

    empty_css = tmp_path / "empty.tcss"
    empty_css.write_text("", encoding="utf-8")
    monkeypatch.setattr(LedgerTrailApp, "CSS_PATH", str(empty_css))
    res_empty_css = cli_runner.invoke(app, ["--selfcheck"])
    assert res_empty_css.exit_code == 1

    monkeypatch.setattr(LedgerTrailApp, "CSS_PATH", orig_css_path)
    monkeypatch.setattr("escape_the_valley.events.build_event_library", list)
    res_low_events = cli_runner.invoke(app, ["--selfcheck"])
    assert res_low_events.exit_code == 1
