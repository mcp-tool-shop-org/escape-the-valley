#!/usr/bin/env python3
"""Check that a PyInstaller Linux binary and all bundled ELF files meet the glibc floor.

Extracts every ELF member (`\\x7fELF`) from the PyInstaller CArchive via
`PyInstaller.archive.readers.CArchiveReader` (plus the outer bootloader ELF),
inspects dynamic symbol versions with `objdump -T`, prints the floor summary
table, and fails closed (exit 1) if any entry requires a `GLIBC_` symbol version
above the configured ceiling (default `GLIBC_2.35`, Ubuntu 22.04) or if zero
`GLIBC_` symbol versions are found across all inspected entries.

Usage:
    python scripts/check_glibc_floor.py <path-to-binary> [--max-glibc 2.35]
"""

from __future__ import annotations

import argparse
import importlib
import re
import subprocess
import sys
import tempfile
from collections import defaultdict
from collections.abc import Callable
from pathlib import Path

ELF_MAGIC: bytes = b"\x7fELF"
MAX_GLIBC_VERSION: tuple[int, int] = (2, 35)

_GLIBC_RE = re.compile(r"\bGLIBC_(\d+)\.(\d+)(?:\.\d+)?\b")
_SKIP_CARCHIVE_TYPECODES = frozenset({"z", "Z", b"z", b"Z", "o", b"o"})


def format_glibc(ver: tuple[int, int]) -> str:
    """Format a `(major, minor)` tuple as `GLIBC_<major>.<minor>`."""
    return f"GLIBC_{ver[0]}.{ver[1]}"


def parse_version_arg(raw: str) -> tuple[int, int]:
    """Parse `'2.35'` or `'GLIBC_2.35'` into `(2, 35)`."""
    cleaned = raw.strip()
    if cleaned.upper().startswith("GLIBC_"):
        cleaned = cleaned[6:]
    parts = cleaned.split(".")
    if len(parts) < 2 or not all(p.isdigit() for p in parts[:2]):
        raise ValueError(f"Invalid glibc version {raw!r}; expected e.g. '2.35'")
    return int(parts[0]), int(parts[1])


def is_elf_bytes(data: bytes) -> bool:
    """Return True if `data` begins with the 4-byte ELF magic (`\\x7fELF`)."""
    return len(data) >= len(ELF_MAGIC) and data[: len(ELF_MAGIC)] == ELF_MAGIC


def is_so_entry_name(name: str) -> bool:
    """Return True if an archive entry name looks like a shared object (`.so` / `.so.*`).

    Used only as a reported cross-check; actual inspection selects by `ELF_MAGIC`.
    """
    leaf = name.replace("\\", "/").rsplit("/", 1)[-1].lower()
    return leaf.endswith(".so") or ".so." in leaf


def parse_glibc_versions(objdump_output: str) -> set[tuple[int, int]]:
    """Extract all `(major, minor)` `GLIBC_` symbol versions from `objdump -T` output."""
    return {
        (int(m.group(1)), int(m.group(2)))
        for m in _GLIBC_RE.finditer(objdump_output)
    }


def max_glibc_version(objdump_output: str) -> tuple[int, int] | None:
    """Return the highest `(major, minor)` `GLIBC_` version in `objdump_output`, or None."""
    versions = parse_glibc_versions(objdump_output)
    return max(versions) if versions else None


def check_objdump_floor(
    name: str,
    objdump_output: str,
    *,
    max_allowed: tuple[int, int] = MAX_GLIBC_VERSION,
) -> tuple[int, int] | None:
    """Verify `objdump_output` for `name` needs no `GLIBC_` version above `max_allowed`.

    Returns the highest `(major, minor)` version found (or `None` if no `GLIBC_`
    references exist). Raises `RuntimeError` if any version exceeds `max_allowed`.
    """
    versions = parse_glibc_versions(objdump_output)
    if not versions:
        return None
    highest = max(versions)
    exceeding = sorted(v for v in versions if v > max_allowed)
    if exceeding:
        bad_str = ", ".join(format_glibc(v) for v in exceeding)
        raise RuntimeError(
            f"{name} requires {bad_str} (highest {format_glibc(highest)} > "
            f"ceiling {format_glibc(max_allowed)})"
        )
    return highest


def _default_objdump_runner(elf_path: Path) -> str:
    """Run `objdump -T <elf_path>` and return stdout."""
    try:
        proc = subprocess.run(
            ["objdump", "-T", str(elf_path)],
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            timeout=60,
            check=False,
        )
    except FileNotFoundError as exc:
        raise RuntimeError(
            f"no_objdump: objdump executable not found while inspecting {elf_path}: {exc}"
        ) from exc
    if proc.returncode != 0:
        raise RuntimeError(
            f"objdump_failed: objdump -T failed (exit {proc.returncode}) on {elf_path}: "
            f"{proc.stderr.strip()}"
        )
    return proc.stdout


def inspect_binary_glibc_floor(
    binary_path: str | Path,
    *,
    max_allowed: tuple[int, int] = MAX_GLIBC_VERSION,
    objdump_runner: Callable[[Path], str] | None = None,
    archive_so_payloads: dict[str, bytes] | None = None,
    check_bootloader: bool = True,
    skipped_so_named_out: list[str] | None = None,
) -> dict[str, tuple[int, int] | None]:
    """Inspect a PyInstaller binary's bundled ELF files (and bootloader) for `GLIBC_` versions.

    Selects archive members by ELF content magic (`\\x7fELF`), not by filename.
    Returns a mapping `{entry_name: highest_version_or_None}` for all inspected
    ELF files. Raises `RuntimeError` with a named reason if the bootloader is
    not an ELF, no bundled ELF entries are found, any entry exceeds
    `max_allowed`, or zero `GLIBC_` versions are found across all inspected
    entries.
    """
    bin_path = Path(binary_path)
    run_objdump = objdump_runner or _default_objdump_runner

    elf_entries: list[tuple[str, bytes]] = []
    skipped_non_elf_so: list[str] = []

    if archive_so_payloads is None:
        if not bin_path.is_file():
            raise FileNotFoundError(f"Binary not found: {bin_path}")
        if check_bootloader:
            with bin_path.open("rb") as fh:
                boot_magic = fh.read(len(ELF_MAGIC))
            if boot_magic != ELF_MAGIC:
                raise RuntimeError(
                    f"non_elf_bootloader: {bin_path} does not start with ELF magic (\\x7fELF)"
                )
        readers_mod = importlib.import_module("PyInstaller.archive.readers")
        reader = readers_mod.CArchiveReader(str(bin_path))
        total_entries = len(reader.toc)
        for raw_name, toc_entry in sorted(
            reader.toc.items(), key=lambda kv: str(kv[0])
        ):
            name = str(raw_name)
            typecode = (
                toc_entry[-1]
                if isinstance(toc_entry, (tuple, list)) and toc_entry
                else None
            )
            if typecode in _SKIP_CARCHIVE_TYPECODES:
                continue
            payload = reader.extract(name)
            if isinstance(payload, bytes) and is_elf_bytes(payload):
                elf_entries.append((name, payload))
            elif is_so_entry_name(name):
                skipped_non_elf_so.append(name)
        if not elf_entries:
            skipped_note = (
                f"; skipped non-ELF .so-named entries: {', '.join(skipped_non_elf_so)}"
                if skipped_non_elf_so
                else ""
            )
            raise RuntimeError(
                f"no_elf_entries: no bundled ELF entries (\\x7fELF) found in CArchive of "
                f"{bin_path} ({total_entries} total entries{skipped_note})"
            )
    else:
        if check_bootloader and bin_path.is_file():
            with bin_path.open("rb") as fh:
                boot_magic = fh.read(len(ELF_MAGIC))
            if boot_magic != ELF_MAGIC:
                raise RuntimeError(
                    f"non_elf_bootloader: {bin_path} does not start with ELF magic (\\x7fELF)"
                )
        for raw_name, payload in sorted(
            archive_so_payloads.items(), key=lambda kv: str(kv[0])
        ):
            name = str(raw_name)
            if isinstance(payload, bytes) and is_elf_bytes(payload):
                elf_entries.append((name, payload))
            elif is_so_entry_name(name):
                skipped_non_elf_so.append(name)
        if not elf_entries:
            skipped_note = (
                f" (skipped non-ELF .so-named entries: {', '.join(skipped_non_elf_so)})"
                if skipped_non_elf_so
                else ""
            )
            raise RuntimeError(
                f"no_elf_entries: no ELF entries (\\x7fELF) found in "
                f"archive_so_payloads{skipped_note}"
            )

    if skipped_so_named_out is not None:
        skipped_so_named_out.extend(skipped_non_elf_so)

    results: dict[str, tuple[int, int] | None] = {}
    violations: list[str] = []

    if check_bootloader and bin_path.is_file():
        boot_out = run_objdump(bin_path)
        try:
            results["<bootloader>"] = check_objdump_floor(
                "<bootloader>", boot_out, max_allowed=max_allowed
            )
        except RuntimeError as exc:
            results["<bootloader>"] = max_glibc_version(boot_out)
            violations.append(str(exc))

    with tempfile.TemporaryDirectory(prefix="etv-glibc-floor-") as tmpdir:
        tmp_root = Path(tmpdir)
        for idx, (name, raw) in enumerate(elf_entries):
            leaf = Path(name.replace("\\", "/")).name
            tmp_elf = tmp_root / f"{idx:03d}-{leaf}"
            tmp_elf.write_bytes(raw)
            out = run_objdump(tmp_elf)
            try:
                highest = check_objdump_floor(
                    name, out, max_allowed=max_allowed
                )
                results[name] = highest
            except RuntimeError as exc:
                results[name] = max_glibc_version(out)
                violations.append(str(exc))

    if violations:
        raise RuntimeError(
            f"glibc floor check failed (ceiling {format_glibc(max_allowed)}): "
            + "; ".join(violations)
        )

    if not any(ver is not None for ver in results.values()):
        inspected = ", ".join(sorted(results.keys())) or "<none>"
        raise RuntimeError(
            f"no_glibc_versions: zero GLIBC_ symbol versions found across "
            f"{len(results)} inspected ELF entry/entries ({inspected})"
        )

    return results


def format_floor_report(
    results: dict[str, tuple[int, int] | None],
    *,
    max_allowed: tuple[int, int] = MAX_GLIBC_VERSION,
    skipped_so_named: list[str] | None = None,
) -> str:
    """Format a human-readable summary table of highest `GLIBC_` versions per file."""
    by_version: dict[tuple[int, int], list[str]] = defaultdict(list)
    no_glibc: list[str] = []
    for name, ver in sorted(results.items()):
        if ver is None:
            no_glibc.append(name)
        else:
            by_version[ver].append(name)

    elf_count = sum(1 for k in results if k != "<bootloader>")
    overall_max = max(by_version.keys()) if by_version else None
    overall_str = format_glibc(overall_max) if overall_max else "none"

    lines = [
        f"[glibc-floor] Inspected {elf_count} bundled ELF file(s)"
        + (" + bootloader" if "<bootloader>" in results else "")
        + f"; max needed: {overall_str} (ceiling: {format_glibc(max_allowed)})",
        "| Highest glibc needed | Files |",
        "|---|---|",
    ]
    for ver in sorted(by_version.keys()):
        files = by_version[ver]
        archive_files = [f for f in files if f != "<bootloader>"]
        has_boot = "<bootloader>" in files
        desc = f"{len(archive_files)}"
        if has_boot:
            desc += " (plus the bootloader)"
        lines.append(f"| {format_glibc(ver)} | {desc} |")
    if no_glibc:
        archive_none = [f for f in no_glibc if f != "<bootloader>"]
        none_desc = f"{len(archive_none)}"
        if "<bootloader>" in no_glibc:
            none_desc += " (plus the bootloader)"
        lines.append(f"| none | {none_desc} |")

    non_so_elfs = sorted(
        k for k in results if k != "<bootloader>" and not is_so_entry_name(k)
    )
    if non_so_elfs:
        lines.append(
            f"[glibc-floor] Note: inspected non-.so ELF member(s): "
            f"{', '.join(non_so_elfs)}"
        )
    if skipped_so_named:
        lines.append(
            f"[glibc-floor] Note: skipped non-ELF .so-named member(s): "
            f"{', '.join(sorted(skipped_so_named))}"
        )
    return "\n".join(lines)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description=(
            "Verify bundled ELF files in a PyInstaller binary "
            "do not exceed the glibc floor."
        )
    )
    parser.add_argument("binary", help="Path to the PyInstaller onefile binary")
    parser.add_argument(
        "--max-glibc",
        default="2.35",
        help="Maximum allowed GLIBC_ version (default: 2.35)",
    )
    args = parser.parse_args(argv)

    try:
        max_allowed = parse_version_arg(args.max_glibc)
    except ValueError as exc:
        print(f"GLIBC FLOOR USAGE ERROR: {exc}", file=sys.stderr)
        return 2

    skipped_so_named: list[str] = []
    try:
        results = inspect_binary_glibc_floor(
            args.binary,
            max_allowed=max_allowed,
            skipped_so_named_out=skipped_so_named,
        )
    except Exception as exc:
        print(f"GLIBC FLOOR CHECK FAILED: {exc}", file=sys.stderr)
        return 1

    print(
        format_floor_report(
            results,
            max_allowed=max_allowed,
            skipped_so_named=skipped_so_named,
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
