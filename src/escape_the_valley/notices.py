"""Third-party license notices generator and bundled-file resolver (issue #28)."""

from __future__ import annotations

import importlib.metadata as md
import re
import sys
from pathlib import Path

from . import __version__

# Build and development tools excluded from third-party runtime notices.
# PyInstaller itself is handled separately via the Bootloader Exception below.
EXCLUDED_BUILD_TOOLS: frozenset[str] = frozenset({
    "pip",
    "setuptools",
    "wheel",
    "pyinstaller",
    "pyinstaller-hooks-contrib",
    "altgraph",
    "pefile",
    "macholib",
    "pywin32-ctypes",
    "packaging",
    "pytest",
    "pytest-cov",
    "coverage",
    "iniconfig",
    "pluggy",
    "ruff",
    "pip-audit",
    "build",
    "hatchling",
    "twine",
})

# Direct runtime roots of escape-the-valley (base + [xrpl] extra bundled in
# standalone binaries). Used to seed dependency graph traversal so dev/global
# packages in a test environment do not pollute generated notices.
_RUNTIME_SEED_ROOTS: tuple[str, ...] = (
    "escape-the-valley",
    "httpx",
    "rich",
    "textual",
    "typer",
    "xrpl-py",
)

_GAME_MIT_LICENSE_FALLBACK = """\
MIT License

Copyright (c) 2026 mcp-tool-shop

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
"""

_LICENSE_FILENAME_RE = re.compile(
    r"^(license|licence|copying|notice|authors)([._-].*)?$",
    re.IGNORECASE,
)
_DOC_NOISE_RE = re.compile(
    r"^(readme|changelog|changes|history)([._-].*)?$",
    re.IGNORECASE,
)
_SKIP_EXTENSIONS = (
    ".py",
    ".pyc",
    ".pyo",
    ".pyd",
    ".so",
    ".dll",
    ".exe",
    ".whl",
    ".json",
    ".toml",
    ".cfg",
)


def _norm(name: str) -> str:
    """PEP 503 normalize a distribution name."""
    return re.sub(r"[-_.]+", "-", name).lower()


def _resolve_license_label(dist: md.Distribution) -> str:
    """Extract the license label from License-Expression, License, or Classifiers."""
    meta = dist.metadata
    lic_expr = (meta.get("License-Expression") or "").strip()
    if lic_expr and lic_expr.upper() != "UNKNOWN":
        return lic_expr

    lic_field = (meta.get("License") or "").strip()
    if lic_field and lic_field.upper() != "UNKNOWN" and "\n" not in lic_field:
        return lic_field

    classifiers = [
        c.split("::")[-1].strip()
        for c in (meta.get_all("Classifier") or [])
        if c.startswith("License ::") and c.split("::")[-1].strip() != "OSI Approved"
    ]
    if classifiers:
        return ", ".join(classifiers)

    if lic_field and lic_field.upper() != "UNKNOWN":
        return lic_field.splitlines()[0].strip()

    return "Unknown"


def _extract_dist_license_files(dist: md.Distribution) -> list[tuple[str, str]]:
    """Return [(display_name, text), ...] for all license/copying/notice files in dist."""
    files = list(dist.files or [])
    # Prefer files inside *.dist-info/ so deduplication keeps canonical metadata names.
    files.sort(
        key=lambda p: (
            ".dist-info/" not in str(p).replace("\\", "/"),
            str(p).replace("\\", "/"),
        )
    )

    results: list[tuple[str, str]] = []
    seen_texts: set[str] = set()

    for f in files:
        rel = str(f).replace("\\", "/")
        base = rel.rsplit("/", 1)[-1]
        if base.lower().endswith(_SKIP_EXTENSIONS):
            continue

        rel_lower = rel.lower()
        in_licenses_dir = (
            ".dist-info/licenses/" in rel_lower
            or ".dist-info/license/" in rel_lower
        )
        is_named_license = bool(_LICENSE_FILENAME_RE.match(base))
        is_doc_noise = bool(_DOC_NOISE_RE.match(base))

        if not ((in_licenses_dir and not is_doc_noise) or is_named_license):
            continue

        try:
            text = f.locate().read_text(encoding="utf-8", errors="replace").strip()
        except Exception:
            continue

        if not text or text in seen_texts:
            continue
        seen_texts.add(text)

        # Strip the leading `<pkg>-<ver>.dist-info/(licenses/)?` prefix so no
        # installation layout noise leaks into the output.
        display_rel = re.sub(
            r"^[^/]+\.dist-info/(?:licenses?/)?",
            "",
            rel,
            flags=re.IGNORECASE,
        )
        results.append((display_rel or base, text))

    return results


def _collect_runtime_distributions(
    dists_by_norm: dict[str, md.Distribution],
) -> list[md.Distribution]:
    """Return the sorted third-party runtime distributions for Escape the Valley."""
    active_extras: dict[str, set[str]] = {"escape-the-valley": {"xrpl", "voice"}}
    visited: set[str] = set()
    queue: list[str] = [root for root in _RUNTIME_SEED_ROOTS if root in dists_by_norm]

    while queue:
        curr = queue.pop(0)
        if curr not in dists_by_norm or curr in EXCLUDED_BUILD_TOOLS:
            continue
        visited.add(curr)
        dist = dists_by_norm[curr]
        curr_extras = active_extras.get(curr, set())

        for req in dist.metadata.get_all("Requires-Dist") or []:
            parts = req.split(";", 1)
            pkg_part = parts[0].strip()
            marker = parts[1].strip() if len(parts) > 1 else ""
            if "extra" in marker and re.search(r"\bextra\s*==", marker):
                matched = any(
                    re.search(
                        rf"""extra\s*==\s*['"]{re.escape(ex)}['"]""",
                        marker,
                    )
                    for ex in curr_extras
                )
                if not matched:
                    continue

            m = re.match(
                r"^([A-Za-z0-9_.-]+)(?:\[([A-Za-z0-9_,.\s-]+)\])?",
                pkg_part,
            )
            if not m:
                continue
            pkg = _norm(m.group(1))
            if pkg in EXCLUDED_BUILD_TOOLS or pkg not in dists_by_norm:
                continue

            req_extras = {
                _norm(x.strip())
                for x in (m.group(2) or "").split(",")
                if x.strip()
            }
            before = set(active_extras.get(pkg, set()))
            after = before | req_extras
            active_extras[pkg] = after
            if pkg not in visited or after != before:
                queue.append(pkg)

    visited.discard("escape-the-valley")
    if not visited:
        # Fallback if custom/mocked distributions were supplied without standard roots.
        visited = {
            k
            for k in dists_by_norm
            if k != "escape-the-valley" and k not in EXCLUDED_BUILD_TOOLS
        }

    return [
        dists_by_norm[k]
        for k in sorted(
            visited,
            key=lambda k: (dists_by_norm[k].metadata.get("Name") or k).lower(),
        )
    ]


def _read_game_license_text(dists_by_norm: dict[str, md.Distribution]) -> str:
    """Read Escape the Valley's own MIT LICENSE text."""
    repo_license = Path(__file__).resolve().parent.parent.parent / "LICENSE"
    if repo_license.is_file():
        try:
            return repo_license.read_text(encoding="utf-8", errors="replace").strip()
        except Exception:
            pass

    etv_dist = dists_by_norm.get("escape-the-valley")
    if etv_dist is not None:
        files = _extract_dist_license_files(etv_dist)
        if files:
            return files[0][1]

    return _GAME_MIT_LICENSE_FALLBACK.strip()


def generate_third_party_notices(
    distributions: list[md.Distribution] | None = None,
) -> str:
    """Generate the full THIRD_PARTY_NOTICES.txt content from installed metadata.

    Contains no local filesystem paths.
    """
    raw_dists = (
        list(distributions)
        if distributions is not None
        else list(md.distributions())
    )
    dists_by_norm: dict[str, md.Distribution] = {}
    for d in raw_dists:
        name = d.metadata.get("Name")
        if name:
            norm_name = _norm(name)
            if norm_name not in dists_by_norm:
                dists_by_norm[norm_name] = d

    runtime_dists = _collect_runtime_distributions(dists_by_norm)
    pyinstaller_dist = dists_by_norm.get("pyinstaller")

    sep = "=" * 80
    lines: list[str] = [
        sep,
        f"Escape the Valley (escape-the-valley {__version__}) — License & Third-Party Notices",
        sep,
        "",
        _read_game_license_text(dists_by_norm),
        "",
        sep,
        "Summary of Included Third-Party Distributions",
        sep,
        "",
    ]

    for d in runtime_dists:
        name = d.metadata.get("Name") or "Unknown"
        ver = d.metadata.get("Version") or "Unknown"
        lic = _resolve_license_label(d)
        lines.append(f"- {name} {ver} ({lic})")

    if pyinstaller_dist is not None:
        pyi_ver = pyinstaller_dist.metadata.get("Version") or "Unknown"
        lines.append(
            f"- PyInstaller {pyi_ver} — Bootloader and Runtime Hooks "
            "(GPLv2-or-later with Bootloader Exception / Apache-2.0)"
        )

    lines.append("")

    if pyinstaller_dist is not None:
        pyi_ver = pyinstaller_dist.metadata.get("Version") or "Unknown"
        pyi_lic = _resolve_license_label(pyinstaller_dist)
        pyi_files = _extract_dist_license_files(pyinstaller_dist)
        lines.extend([
            sep,
            "Name: PyInstaller (Bootloader and Runtime Hooks)",
            f"Version: {pyi_ver}",
            f"License: {pyi_lic}",
            sep,
            "",
        ])
        for label, text in pyi_files:
            lines.append(f"--- {label} ---")
            lines.append(text)
            lines.append("")

    for d in runtime_dists:
        name = d.metadata.get("Name") or "Unknown"
        ver = d.metadata.get("Version") or "Unknown"
        lic = _resolve_license_label(d)
        lic_files = _extract_dist_license_files(d)

        lines.extend([
            sep,
            f"Name: {name}",
            f"Version: {ver}",
            f"License: {lic}",
            sep,
            "",
        ])
        if lic_files:
            for label, text in lic_files:
                lines.append(f"--- {label} ---")
                lines.append(text)
                lines.append("")
        else:
            lines.append(f"(License metadata: {lic})")
            lines.append("")

    return "\n".join(lines).rstrip() + "\n"


def write_third_party_notices(output_path: str | Path = "THIRD_PARTY_NOTICES.txt") -> Path:
    """Generate and write THIRD_PARTY_NOTICES.txt to output_path."""
    path = Path(output_path)
    content = generate_third_party_notices()
    path.write_text(content, encoding="utf-8")
    return path


def resolve_bundled_notices_path() -> Path | None:
    """Resolve a bundled THIRD_PARTY_NOTICES.txt if present (PyInstaller or package dir)."""
    meipass = getattr(sys, "_MEIPASS", None)
    candidates: list[Path] = []
    if getattr(sys, "frozen", False) and meipass:
        meipass_dir = Path(meipass)
        candidates.extend([
            meipass_dir / "escape_the_valley" / "THIRD_PARTY_NOTICES.txt",
            meipass_dir / "THIRD_PARTY_NOTICES.txt",
        ])
    candidates.append(Path(__file__).resolve().parent / "THIRD_PARTY_NOTICES.txt")

    for candidate in candidates:
        if candidate.is_file():
            return candidate
    return None


def get_third_party_notices_text() -> str:
    """Return the bundled THIRD_PARTY_NOTICES.txt, or generate it at runtime for pip installs."""
    bundled = resolve_bundled_notices_path()
    if bundled is not None:
        return bundled.read_text(encoding="utf-8", errors="replace")
    if getattr(sys, "frozen", False):
        raise FileNotFoundError(
            "Bundled THIRD_PARTY_NOTICES.txt not found in frozen binary."
        )
    return generate_third_party_notices()
