"""Unit tests for scripts/check_glibc_floor.py."""

from __future__ import annotations

import importlib.util
import sys
from pathlib import Path

import pytest

SCRIPT_PATH = Path(__file__).resolve().parents[1] / "scripts" / "check_glibc_floor.py"


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
                "libpython3.12.so.1.0": b"so1",
                "Crypto/Cipher/_raw_aes.abi3.so": b"so2",
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
            return f"payload:{name}".encode()

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
