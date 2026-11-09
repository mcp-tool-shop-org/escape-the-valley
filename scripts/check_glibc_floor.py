#!/usr/bin/env python3
"""Check that a PyInstaller Linux binary and all bundled .so files meet the glibc floor.

Extracts every `.so` from the PyInstaller CArchive via
`PyInstaller.archive.readers.CArchiveReader` (plus the outer bootloader ELF),
inspects dynamic symbol versions with `objdump -T`, prints the floor summary
table, and fails (exit 1) if any entry requires a `GLIBC_` symbol version above
the configured ceiling (default `GLIBC_2.35`, Ubuntu 22.04).

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

MAX_GLIBC_VERSION: tuple[int, int] = (2, 35)

_GLIBC_RE = re.compile(r"\bGLIBC_(\d+)\.(\d+)(?:\.\d+)?\b")


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


def is_so_entry_name(name: str) -> bool:
    """Return True if an archive entry name is a shared library (`.so` or `.so.*`)."""
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
    proc = subprocess.run(
        ["objdump", "-T", str(elf_path)],
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=60,
        check=False,
    )
    if proc.returncode != 0:
        raise RuntimeError(
            f"objdump -T failed (exit {proc.returncode}) on {elf_path}: "
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
) -> dict[str, tuple[int, int] | None]:
    """Inspect a PyInstaller binary's bundled `.so` files (and bootloader) for `GLIBC_` versions.

    Returns a mapping `{entry_name: highest_version_or_None}` for all inspected
    files. Raises `RuntimeError` if no `.so` files are found or if any entry
    requires a `GLIBC_` version above `max_allowed`.
    """
    bin_path = Path(binary_path)
    run_objdump = objdump_runner or _default_objdump_runner

    if archive_so_payloads is None:
        if not bin_path.is_file():
            raise FileNotFoundError(f"Binary not found: {bin_path}")
        readers_mod = importlib.import_module("PyInstaller.archive.readers")
        reader = readers_mod.CArchiveReader(str(bin_path))
        total_entries = len(reader.toc)
        so_names = sorted(
            str(k) for k in reader.toc.keys() if is_so_entry_name(str(k))
        )
        if not so_names:
            raise RuntimeError(
                f"No bundled .so entries found in CArchive of {bin_path} "
                f"({total_entries} total entries)"
            )
    else:
        reader = None
        total_entries = len(archive_so_payloads)
        so_names = sorted(
            str(k) for k in archive_so_payloads.keys() if is_so_entry_name(str(k))
        )
        if not so_names:
            raise RuntimeError("No .so entries provided in archive_so_payloads")

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
        for idx, name in enumerate(so_names):
            raw = (
                reader.extract(name)
                if reader is not None
                else archive_so_payloads[name]  # type: ignore[index]
            )
            leaf = Path(name.replace("\\", "/")).name
            tmp_so = tmp_root / f"{idx:03d}-{leaf}"
            tmp_so.write_bytes(raw)
            out = run_objdump(tmp_so)
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

    return results


def format_floor_report(
    results: dict[str, tuple[int, int] | None],
    *,
    max_allowed: tuple[int, int] = MAX_GLIBC_VERSION,
) -> str:
    """Format a human-readable summary table of highest `GLIBC_` versions per file."""
    by_version: dict[tuple[int, int], list[str]] = defaultdict(list)
    no_glibc: list[str] = []
    for name, ver in sorted(results.items()):
        if ver is None:
            no_glibc.append(name)
        else:
            by_version[ver].append(name)

    so_count = sum(1 for k in results if k != "<bootloader>")
    overall_max = max(by_version.keys()) if by_version else None
    overall_str = format_glibc(overall_max) if overall_max else "none"

    lines = [
        f"[glibc-floor] Inspected {so_count} bundled .so file(s)"
        + (" + bootloader" if "<bootloader>" in results else "")
        + f"; max needed: {overall_str} (ceiling: {format_glibc(max_allowed)})",
        "| Highest glibc needed | Files |",
        "|---|---|",
    ]
    for ver in sorted(by_version.keys()):
        files = by_version[ver]
        so_files = [f for f in files if f != "<bootloader>"]
        has_boot = "<bootloader>" in files
        desc = f"{len(so_files)}"
        if has_boot:
            desc += " (plus the bootloader)"
        lines.append(f"| {format_glibc(ver)} | {desc} |")
    if no_glibc:
        lines.append(f"| none | {len(no_glibc)} |")
    return "\n".join(lines)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description=(
            "Verify bundled .so files in a PyInstaller binary "
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

    try:
        results = inspect_binary_glibc_floor(
            args.binary, max_allowed=max_allowed
        )
    except Exception as exc:
        print(f"GLIBC FLOOR CHECK FAILED: {exc}", file=sys.stderr)
        return 1

    print(format_floor_report(results, max_allowed=max_allowed))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
