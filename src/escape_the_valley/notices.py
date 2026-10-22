"""Third-party license notices generator and bundled-file resolver (issue #28)."""

from __future__ import annotations

import importlib.metadata as md
import re
import sys
import sysconfig
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

# Licences in the MPL, LGPL, or GPL family require a source-distribution offer / URL
# when binary or object forms are distributed (e.g. MPL-2.0 §3.2(a) for certifi).
_COPYLEFT_SOURCE_RE = re.compile(
    r"\b(MPL|Mozilla Public License|LGPL|Lesser General Public License|"
    r"Library General Public License|GPL|General Public License)\b",
    re.IGNORECASE,
)

# CPython's Section C ("Licenses and Acknowledgements for Incorporated Software"
# from CPython Doc/license.rst) covering third-party C libraries linked into or
# shipped with the CPython interpreter binary across Windows, macOS, and Linux.
_CPYTHON_INCORPORATED_SOFTWARE_NOTICES = """\
C. LICENSES AND ACKNOWLEDGEMENTS FOR INCORPORATED SOFTWARE (CPython Runtime)
============================================================================

This section lists the licenses and acknowledgements for third-party software
incorporated into or bundled alongside the CPython runtime distribution:
OpenSSL, libffi, zlib, Expat, bzip2, XZ Utils / liblzma, SQLite, and mpdecimal.

1. OpenSSL (The OpenSSL Project — Apache License 2.0)
-----------------------------------------------------
Copyright (c) 1998-2025 The OpenSSL Project. All rights reserved.
Copyright (c) 1995-1998 Eric Young. All rights reserved.

Licensed under the Apache License, Version 2.0 (the "License"); you may not use
this file except in compliance with the License. You may obtain a copy of the
License at https://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software distributed
under the License is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR
CONDITIONS OF ANY KIND, either express or implied.

2. libffi (MIT License)
-----------------------
libffi - Copyright (c) 1996-2024 Anthony Green, Red Hat, Inc and others.

Permission is hereby granted, free of charge, to any person obtaining a copy of
this software and associated documentation files (the "Software"), to deal in
the Software without restriction, including without limitation the rights to
use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies
of the Software, and to permit persons to whom the Software is furnished to do
so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS
FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR
COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER
IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN
CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

3. zlib (Zlib License)
----------------------
Copyright (C) 1995-2024 Jean-loup Gailly and Mark Adler

This software is provided 'as-is', without any express or implied warranty. In
no event will the authors be held liable for any damages arising from the use
of this software.

Permission is granted to anyone to use this software for any purpose, including
commercial applications, and to alter it and redistribute it freely, subject to
the following restrictions:

1. The origin of this software must not be misrepresented; you must not claim
   that you wrote the original software. If you use this software in a product,
   an acknowledgment in the product documentation would be appreciated but is
   not required.
2. Altered source versions must be plainly marked as such, and must not be
   misrepresented as being the original software.
3. This notice may not be removed or altered from any source distribution.

4. Expat XML Parser (MIT License)
---------------------------------
Copyright (c) 1998-2000 Thai Open Source Software Center Ltd and Clark Cooper
Copyright (c) 2001-2024 Expat maintainers

Permission is hereby granted, free of charge, to any person obtaining a copy of
this software and associated documentation files (the "Software"), to deal in
the Software without restriction, including without limitation the rights to
use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies
of the Software, and to permit persons to whom the Software is furnished to do
so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS
FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR
COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER
IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN
CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

5. bzip2 / libbzip2 (BSD-Style License)
---------------------------------------
This program, "bzip2", the associated library "libbzip2", and all documentation,
are copyright (C) 1996-2019 Julian R Seward. All rights reserved.

Redistribution and use in source and binary forms, with or without modification,
are permitted provided that the following conditions are met:

1. Redistributions of source code must retain the above copyright notice, this
   list of conditions and the following disclaimer.
2. The origin of this software must not be misrepresented; you must not claim
   that you wrote the original software. If you use this software in a product,
   an acknowledgment in the product documentation would be appreciated but is
   not required.
3. Altered source versions must be plainly marked as such, and must not be
   misrepresented as being the original software.
4. The name of the author may not be used to endorse or promote products derived
   from this software without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE AUTHOR "AS IS" AND ANY EXPRESS OR IMPLIED
WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF
MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED.

6. XZ Utils / liblzma (0BSD / Public Domain)
--------------------------------------------
Author: Lasse Collin and contributors

liblzma in XZ Utils is distributed under the BSD Zero Clause License (0BSD) and
public domain dedications. Permission to use, copy, modify, and/or distribute
this software for any purpose with or without fee is hereby granted.

THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES WITH
REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF MERCHANTABILITY AND
FITNESS.

7. SQLite (Public Domain)
-------------------------
The author or authors of SQLite have dedicated the code to the public domain.
Anyone is free to copy, modify, publish, use, compile, sell, or distribute the
original SQLite code, either in source code form or as a compiled binary, for
any purpose, commercial or non-commercial, and by any means.

8. mpdecimal / libmpdec (BSD-2-Clause License)
----------------------------------------------
Copyright (c) 2008-2020 Stefan Krah. All rights reserved.

Redistribution and use in source and binary forms, with or without modification,
are permitted provided that the following conditions are met:

1. Redistributions of source code must retain the above copyright notice, this
   list of conditions and the following disclaimer.
2. Redistributions in binary form must reproduce the above copyright notice,
   this list of conditions and the following disclaimer in the documentation
   and/or other materials provided with the distribution.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND
ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED
WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
DISCLAIMED.
"""

_CPYTHON_PSF_FALLBACK = """\
PYTHON SOFTWARE FOUNDATION LICENSE VERSION 2
--------------------------------------------

1. This LICENSE AGREEMENT is between the Python Software Foundation ("PSF"), and
   the Individual or Organization ("Licensee") accessing and otherwise using
   this software ("Python") in source or binary form and its associated
   documentation.

2. Subject to the terms and conditions of this License Agreement, PSF hereby
   grants Licensee a nonexclusive, royalty-free, world-wide license to
   reproduce, analyze, test, perform and/or display publicly, prepare
   derivative works, distribute, and otherwise use Python alone or in any
   derivative version, provided, however, that PSF's License Agreement and
   PSF's notice of copyright, i.e., "Copyright (c) 2001-2026 Python Software
   Foundation; All Rights Reserved" are retained in Python alone or in any
   derivative version prepared by Licensee.
"""


def _norm(name: str) -> str:
    """PEP 503 normalize a distribution name."""
    return re.sub(r"[-_.]+", "-", name).lower()


def _is_stub_only_dist(norm_name: str) -> bool:
    """Return True for PEP 561 type-stub-only distributions (`types-*`)."""
    return norm_name.startswith("types-")


def _requires_source_line(license_label: str) -> bool:
    """Return True if license_label is in the MPL, LGPL, or GPL family."""
    return bool(_COPYLEFT_SOURCE_RE.search(license_label))


def _resolve_upstream_url(dist: md.Distribution) -> str | None:
    """Extract the upstream repository or homepage URL from distribution metadata."""
    meta = dist.metadata
    preferred_labels = (
        "source",
        "source code",
        "repository",
        "code",
        "github",
        "gitlab",
        "homepage",
        "home-page",
        "home",
    )
    project_urls: list[tuple[str, str]] = []
    for entry in meta.get_all("Project-URL") or []:
        if "," not in entry:
            continue
        label, url = entry.split(",", 1)
        clean_url = url.strip()
        if clean_url.startswith(("http://", "https://")):
            project_urls.append((label.strip().lower(), clean_url))

    for pref in preferred_labels:
        for label, url in project_urls:
            if label == pref or pref in label:
                return url

    homepage = (meta.get("Home-page") or "").strip()
    if homepage and homepage.upper() != "UNKNOWN" and homepage.startswith(("http://", "https://")):
        return homepage

    if project_urls:
        return project_urls[0][1]

    return None


def _format_source_line(name: str, ver: str, dist: md.Distribution) -> str:
    """Format the required Source: line for MPL/LGPL/GPL family distributions."""
    pypi_url = f"https://pypi.org/project/{name}/{ver}/#files"
    upstream = _resolve_upstream_url(dist)
    if upstream:
        return f"Source: {name}=={ver} — {pypi_url} | Upstream: {upstream}"
    return f"Source: {name}=={ver} — {pypi_url}"


def _locate_cpython_license_file() -> Path | None:
    """Locate CPython's LICENSE.txt across Windows, macOS, Linux, and python-build-standalone."""
    base = Path(sys.base_prefix)
    stdlib_str = sysconfig.get_path("stdlib")
    stdlib = Path(stdlib_str) if stdlib_str else None
    py_xy = f"python{sys.version_info.major}.{sys.version_info.minor}"

    candidates: list[Path] = [
        base / "LICENSE.txt",
        base / "LICENSE",
    ]
    if stdlib is not None:
        candidates.extend([
            stdlib / "LICENSE.txt",
            stdlib / "LICENSE",
        ])
    candidates.extend([
        base / "lib" / py_xy / "LICENSE.txt",
        base / "lib" / py_xy / "LICENSE",
        base / "Lib" / "LICENSE.txt",
        base / "Lib" / "LICENSE",
        base / "licenses" / "LICENSE.txt",
        base.parent / "LICENSE.txt",
        base.parent / "licenses" / "LICENSE.txt",
    ])

    for cand in candidates:
        try:
            if cand.is_file():
                return cand
        except Exception:
            continue
    return None


def _read_cpython_license_files() -> list[tuple[str, str]]:
    """Return [(display_name, text), ...] for CPython's PSF license and incorporated software."""
    results: list[tuple[str, str]] = []
    lic_path = _locate_cpython_license_file()
    lic_text = ""
    if lic_path is not None:
        try:
            lic_text = lic_path.read_text(encoding="utf-8", errors="replace").strip()
        except Exception:
            lic_text = ""

    if not lic_text:
        lic_text = _CPYTHON_PSF_FALLBACK.strip()

    results.append(("LICENSE.txt", lic_text))
    results.append((
        "CPython Incorporated Third-Party Software Notices",
        _CPYTHON_INCORPORATED_SOFTWARE_NOTICES.strip(),
    ))
    return results


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
        if (
            curr not in dists_by_norm
            or curr in EXCLUDED_BUILD_TOOLS
            or _is_stub_only_dist(curr)
        ):
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
            if (
                pkg in EXCLUDED_BUILD_TOOLS
                or _is_stub_only_dist(pkg)
                or pkg not in dists_by_norm
            ):
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
            if k != "escape-the-valley"
            and k not in EXCLUDED_BUILD_TOOLS
            and not _is_stub_only_dist(k)
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
    py_ver = sys.version.split()[0]
    cpython_files = _read_cpython_license_files()

    sep = "=" * 80
    lines: list[str] = [
        sep,
        f"Escape the Valley (escape-the-valley {__version__}) — License & Third-Party Notices",
        sep,
        "",
        _read_game_license_text(dists_by_norm),
        "",
        sep,
        "Name: Python (CPython Runtime and Standard Library)",
        f"Version: {py_ver}",
        "License: PSF-2.0 (Python Software Foundation License Version 2 + Incorporated Software)",
        sep,
        "",
    ]

    for label, text in cpython_files:
        lines.append(f"--- {label} ---")
        lines.append(text)
        lines.append("")

    lines.extend([
        sep,
        "Summary of Included Third-Party Distributions",
        sep,
        "",
        f"- Python (CPython Runtime and Standard Library) {py_ver} (PSF-2.0)",
    ])

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

        header_lines = [
            sep,
            f"Name: {name}",
            f"Version: {ver}",
            f"License: {lic}",
        ]
        if _requires_source_line(lic):
            header_lines.append(_format_source_line(name, ver, d))
        header_lines.extend([sep, ""])
        lines.extend(header_lines)

        if lic_files:
            for label, text in lic_files:
                lines.append(f"--- {label} ---")
                lines.append(text)
                lines.append("")
        else:
            lines.append(f"(License metadata: {lic})")
            lines.append("")

    return "\n".join(lines).rstrip() + "\n"


def _build_top_level_to_dist_map(
    distributions: list[md.Distribution],
) -> dict[str, str]:
    """Map top-level site-packages module/package/dist-info names to normalized dist names."""
    mapping: dict[str, str] = {}
    for d in distributions:
        raw_name = d.metadata.get("Name")
        if not raw_name:
            continue
        norm_name = _norm(raw_name)
        mapping[norm_name] = norm_name
        mapping[norm_name.replace("-", "_")] = norm_name

        try:
            top_txt = d.read_text("top_level.txt") or ""
        except Exception:
            top_txt = ""
        for line in top_txt.splitlines():
            tl = line.strip().split("/")[0]
            if tl:
                mapping[tl] = norm_name
                mapping[tl.lower()] = norm_name

        for f in d.files or []:
            rel = str(f).replace("\\", "/")
            if rel.startswith(".."):
                continue
            first = rel.split("/", 1)[0]
            if not first or first == "__pycache__":
                continue
            mapping[first] = norm_name
            mapping[first.lower()] = norm_name
            if first.endswith(".py"):
                stem = first[:-3]
                mapping[stem] = norm_name
                mapping[stem.lower()] = norm_name
            elif "." in first and not first.endswith((".dist-info", ".egg-info")):
                stem = first.split(".", 1)[0]
                if stem:
                    mapping[stem] = norm_name
                    mapping[stem.lower()] = norm_name

    return mapping


def verify_toc_against_notices(
    build_dir: str | Path,
    notices_path: str | Path = "THIRD_PARTY_NOTICES.txt",
    distributions: list[md.Distribution] | None = None,
) -> list[str]:
    """Verify every top-level site-packages distribution in PyInstaller's TOC has a notices entry.

    Reads all `*.toc` (and `xref-*.html`) files under `build_dir`, maps every
    `site-packages/<entry>` path back to its owning distribution via
    `importlib.metadata`, and raises `RuntimeError` if any bundled third-party
    distribution is absent from `notices_path`.
    """
    build_root = Path(build_dir)
    toc_files = sorted(build_root.rglob("*.toc")) if build_root.is_dir() else []
    if not toc_files:
        raise FileNotFoundError(
            f"No PyInstaller .toc files found under {build_root}"
        )

    notices_file = Path(notices_path)
    if not notices_file.is_file():
        raise FileNotFoundError(f"Notices file not found: {notices_file}")

    notices_text = notices_file.read_text(encoding="utf-8", errors="replace")
    noticed_norms: set[str] = set()
    for m in re.finditer(r"^Name:\s*(.+)$", notices_text, flags=re.MULTILINE):
        raw_entry = m.group(1).strip()
        noticed_norms.add(_norm(raw_entry))

    raw_dists = (
        list(distributions)
        if distributions is not None
        else list(md.distributions())
    )
    top_map = _build_top_level_to_dist_map(raw_dists)

    ignored_entries = {
        "__pycache__",
        "_virtualenv.py",
        "_virtualenv.pth",
        "distutils-precedence.pth",
    }
    ignored_dists = {
        "escape-the-valley",
        "pyinstaller",
        "pyinstaller-hooks-contrib",
    }

    bundled_dists: set[str] = set()
    site_pkg_re = re.compile(r"site-packages[\\/]+([A-Za-z0-9_.-]+)")

    scan_files = list(toc_files) + sorted(build_root.rglob("xref-*.html"))
    for tf in scan_files:
        try:
            text = tf.read_text(encoding="utf-8", errors="replace")
        except Exception:
            continue
        for match in site_pkg_re.finditer(text):
            entry = match.group(1)
            if (
                entry in ignored_entries
                or entry.endswith((".pth", ".egg-link"))
                or entry.startswith("_virtualenv")
            ):
                continue

            dist_norm = (
                top_map.get(entry)
                or top_map.get(entry.lower())
            )
            if dist_norm is None:
                # Strip .dist-info / .egg-info / .py suffix if unmapped
                cleaned = re.sub(
                    r"(-[0-9][A-Za-z0-9_.-]*)?\.(dist-info|egg-info|py|pyd|so)$",
                    "",
                    entry,
                    flags=re.IGNORECASE,
                )
                dist_norm = (
                    top_map.get(cleaned)
                    or top_map.get(cleaned.lower())
                    or _norm(cleaned)
                )

            if dist_norm in ignored_dists or _is_stub_only_dist(dist_norm):
                continue
            bundled_dists.add(dist_norm)

    missing = sorted(d for d in bundled_dists if d not in noticed_norms)
    if missing:
        raise RuntimeError(
            f"Bundled site-packages distributions in PyInstaller TOC missing "
            f"from {notices_file}: {', '.join(missing)}"
        )

    return sorted(bundled_dists)


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
    else:
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
