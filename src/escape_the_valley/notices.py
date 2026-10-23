"""Third-party license notices generator and bundled-file resolver (issue #28)."""

from __future__ import annotations

import hashlib
import importlib.metadata as md
import json
import platform
import re
import sys
import sysconfig
from pathlib import Path
from typing import Any

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

_LICENSES_DIR: Path = Path(__file__).resolve().parent / "_licenses"
_SOURCES_JSON_PATH: Path = _LICENSES_DIR / "SOURCES.json"

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


def _norm(name: str) -> str:
    """PEP 503 normalize a distribution name."""
    return re.sub(r"[-_.]+", "-", name).lower()


def _is_stub_only_dist(norm_name: str) -> bool:
    """Return True for PEP 561 type-stub-only distributions (`types-*`)."""
    return norm_name.startswith("types-")


def _requires_source_line(license_label: str) -> bool:
    """Return True if license_label is in the MPL, LGPL, or GPL family."""
    return bool(_COPYLEFT_SOURCE_RE.search(license_label))


def _sha256_bytes(data: bytes) -> str:
    """Return lowercase hex SHA-256 digest of raw bytes."""
    return hashlib.sha256(data).hexdigest()


def _load_sources_manifest() -> dict[str, Any]:
    """Load and return `src/escape_the_valley/_licenses/SOURCES.json`."""
    if not _SOURCES_JSON_PATH.is_file():
        raise FileNotFoundError(
            f"Vendored license manifest not found: {_SOURCES_JSON_PATH}"
        )
    return json.loads(_SOURCES_JSON_PATH.read_text(encoding="utf-8"))


def _detect_target_triple(
    platform_str: str | None = None,
    machine_str: str | None = None,
) -> str:
    """Determine the python-build-standalone target triple for the running platform."""
    plat = (platform_str if platform_str is not None else sys.platform).lower()
    mach = (machine_str if machine_str is not None else platform.machine()).lower()

    if plat.startswith("linux") and mach in {"x86_64", "amd64"}:
        return "x86_64-unknown-linux-gnu"
    if plat == "darwin" and mach in {"arm64", "aarch64"}:
        return "aarch64-apple-darwin"
    if plat in {"win32", "cygwin", "msys"} and mach in {"amd64", "x86_64"}:
        return "x86_64-pc-windows-msvc"
    return f"{mach}-{plat}"


def _read_interpreter_pbs_build_tag() -> str | None:
    """Read the pbs build tag from `<sys.base_prefix>/BUILD` or `sys.version`."""
    base = Path(sys.base_prefix)
    build_file = base / "BUILD"
    if build_file.is_file():
        tag = build_file.read_text(encoding="utf-8", errors="replace").strip()
        if tag:
            return tag

    python_json = base / "PYTHON.json"
    if python_json.is_file():
        try:
            meta = json.loads(python_json.read_text(encoding="utf-8"))
            tag = str(meta.get("build_tag") or "").strip()
            if tag:
                return tag
        except Exception:
            pass

    m = re.search(r"\+(\d{8})\b", sys.version)
    if m:
        return m.group(1)
    return None


def _verify_vendored_licenses_match(
    sources: dict[str, Any],
    target: str | None = None,
) -> str:
    """Verify running CPython version, pbs build tag, and target match `_licenses/`.

    Refuses with `RuntimeError` if `_licenses/` is vendored for a different
    CPython version, pbs build tag, or target platform.
    """
    resolved_target = target if target is not None else _detect_target_triple()
    targets = sources.get("targets") or {}
    if resolved_target not in targets or not (_LICENSES_DIR / resolved_target).is_dir():
        raise RuntimeError(
            f"Vendored _licenses/ does not cover target {resolved_target!r} "
            f"(available targets: {sorted(targets)})"
        )

    expected_py_ver = str(sources.get("python_version") or "").strip()
    running_py_ver = sys.version.split()[0].split("+")[0]
    if expected_py_ver and running_py_ver != expected_py_ver:
        raise RuntimeError(
            f"Running interpreter CPython version {running_py_ver!r} does not match "
            f"vendored _licenses/ CPython version {expected_py_ver!r}"
        )

    expected_pbs_tag = str(sources.get("pbs_build_tag") or "").strip()
    running_pbs_tag = _read_interpreter_pbs_build_tag()
    if running_pbs_tag is not None and expected_pbs_tag and running_pbs_tag != expected_pbs_tag:
        raise RuntimeError(
            f"Running interpreter python-build-standalone build tag {running_pbs_tag!r} "
            f"does not match vendored _licenses/ build tag {expected_pbs_tag!r}"
        )

    return resolved_target


def verify_vendored_licenses_integrity() -> dict[str, Any]:
    """Verify every vendored file under `_licenses/` exists and matches `SOURCES.json` SHA-256."""
    sources = _load_sources_manifest()

    game_lic = sources["game_license"]
    game_lic_path = _LICENSES_DIR / game_lic["vendored_path"]
    if not game_lic_path.is_file():
        raise FileNotFoundError(f"Missing vendored game license: {game_lic_path}")
    actual_game_sha = _sha256_bytes(game_lic_path.read_bytes())
    if actual_game_sha != game_lic["sha256"]:
        raise RuntimeError(
            f"SHA-256 mismatch for {game_lic_path}: "
            f"expected {game_lic['sha256']}, got {actual_game_sha}"
        )

    inc = sources["cpython_incorporated_software"]
    doc_path = _LICENSES_DIR / inc["doc_license_rst_vendored_path"]
    inc_path = _LICENSES_DIR / inc["vendored_path"]
    for p in (doc_path, inc_path):
        if not p.is_file():
            raise FileNotFoundError(f"Missing vendored CPython file: {p}")

    doc_bytes = doc_path.read_bytes()
    inc_bytes = inc_path.read_bytes()
    actual_doc_sha = _sha256_bytes(doc_bytes)
    actual_inc_sha = _sha256_bytes(inc_bytes)
    if actual_doc_sha != inc["doc_license_rst_sha256"]:
        raise RuntimeError(
            f"SHA-256 mismatch for {doc_path}: "
            f"expected {inc['doc_license_rst_sha256']}, got {actual_doc_sha}"
        )
    if actual_inc_sha != inc["sha256"]:
        raise RuntimeError(
            f"SHA-256 mismatch for {inc_path}: "
            f"expected {inc['sha256']}, got {actual_inc_sha}"
        )

    doc_text = doc_bytes.decode("utf-8")
    inc_text = inc_bytes.decode("utf-8")
    if not doc_text.endswith(inc_text):
        raise RuntimeError(
            f"{inc_path} is not a verbatim suffix section of {doc_path}"
        )

    for target_name, target_info in (sources.get("targets") or {}).items():
        for entry in target_info.get("files") or []:
            rel_path = entry["vendored_path"]
            fpath = _LICENSES_DIR / rel_path
            if not fpath.is_file():
                raise FileNotFoundError(
                    f"Missing vendored pbs license file for {target_name}: {fpath}"
                )
            actual_sha = _sha256_bytes(fpath.read_bytes())
            if actual_sha != entry["sha256"]:
                raise RuntimeError(
                    f"SHA-256 mismatch for {fpath}: "
                    f"expected {entry['sha256']}, got {actual_sha}"
                )

    return sources


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


def _read_cpython_license_blocks(
    sources: dict[str, Any],
    target: str,
) -> list[tuple[str, str, str, str]]:
    """Return `[(title, source_url, sha256_line, verbatim_text), ...]` for CPython and pbs."""
    lic_path = _locate_cpython_license_file()
    if lic_path is None:
        raise FileNotFoundError(
            f"CPython LICENSE.txt not found under sys.base_prefix ({sys.base_prefix!r}); "
            "refusing to generate partial PSF license notices."
        )

    raw_lic = lic_path.read_bytes()
    lic_text = raw_lic.decode("utf-8", errors="replace").strip()
    if not lic_text:
        raise FileNotFoundError(
            f"CPython LICENSE file at {lic_path} is empty; "
            "refusing to generate partial PSF license notices."
        )
    lic_sha = _sha256_bytes(raw_lic)

    target_info = sources["targets"][target]
    cpython_tag = sources["cpython_tag"]
    interp_source_url = (
        f"{target_info['pbs_archive_url']} "
        f"(path: {target_info['install_license_archive_path']}, "
        f"archive sha256: {target_info['archive_sha256']})"
    )

    blocks: list[tuple[str, str, str, str]] = [
        (
            f"CPython Interpreter {lic_path.name} ({target})",
            interp_source_url,
            lic_sha,
            lic_text,
        )
    ]

    inc_info = sources["cpython_incorporated_software"]
    inc_path = _LICENSES_DIR / inc_info["vendored_path"]
    if not inc_path.is_file():
        raise FileNotFoundError(
            f"Vendored CPython incorporated-software section missing: {inc_path}"
        )
    inc_raw = inc_path.read_bytes()
    inc_sha = _sha256_bytes(inc_raw)
    if inc_sha != inc_info["sha256"]:
        raise RuntimeError(
            f"SHA-256 mismatch for {inc_path}: expected {inc_info['sha256']}, got {inc_sha}"
        )
    inc_text = inc_raw.decode("utf-8").strip()
    blocks.append((
        f"CPython {cpython_tag} Doc/license.rst — {inc_info['section_heading']}",
        f"{inc_info['source_url']} (Doc/license.rst sha256: {inc_info['doc_license_rst_sha256']})",
        inc_sha,
        inc_text,
    ))

    for entry in target_info["files"]:
        vpath = _LICENSES_DIR / entry["vendored_path"]
        if not vpath.is_file():
            raise FileNotFoundError(
                f"Vendored python-build-standalone license file missing: {vpath}"
            )
        raw_bytes = vpath.read_bytes()
        fsha = _sha256_bytes(raw_bytes)
        if fsha != entry["sha256"]:
            raise RuntimeError(
                f"SHA-256 mismatch for {vpath}: expected {entry['sha256']}, got {fsha}"
            )
        text = raw_bytes.decode("utf-8").replace("\r\n", "\n").strip()
        src_desc = (
            f"{entry['pbs_archive_url']} "
            f"(archive: {entry['archive_name']}, "
            f"archive sha256: {entry['archive_sha256']}, "
            f"path: {entry['archive_path']})"
        )
        blocks.append((
            f"python-build-standalone ({target}): {entry['archive_path']} — {entry['component']}",
            src_desc,
            fsha,
            text,
        ))

    return blocks


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


def _extract_dist_license_files(
    dist: md.Distribution,
) -> list[tuple[str, str, str]]:
    """Return `[(display_name, sha256, text), ...]` for all license/copying/notice files in dist."""
    files = list(dist.files or [])
    # Prefer files inside *.dist-info/ so deduplication keeps canonical metadata names.
    files.sort(
        key=lambda p: (
            ".dist-info/" not in str(p).replace("\\", "/"),
            str(p).replace("\\", "/"),
        )
    )

    results: list[tuple[str, str, str]] = []
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
            located = Path(str(f.locate()))
            raw_bytes = located.read_bytes()
            text = raw_bytes.decode("utf-8", errors="replace").strip()
        except Exception:
            continue

        if not text or text in seen_texts:
            continue
        seen_texts.add(text)
        file_sha = _sha256_bytes(raw_bytes)

        # Strip the leading `<pkg>-<ver>.dist-info/(licenses/)?` prefix so no
        # installation layout noise leaks into the output.
        display_rel = re.sub(
            r"^[^/]+\.dist-info/(?:licenses?/)?",
            "",
            rel,
            flags=re.IGNORECASE,
        )
        results.append((display_rel or base, file_sha, text))

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


def _read_game_license_block(
    sources: dict[str, Any],
    dists_by_norm: dict[str, md.Distribution],
) -> tuple[str, str, str]:
    """Read Escape the Valley's MIT LICENSE verbatim as `(source_url, sha256, text)`."""
    game_meta = sources.get("game_license") or {}
    source_url = str(
        game_meta.get("source_url")
        or f"https://github.com/mcp-tool-shop-org/escape-the-valley/blob/v{__version__}/LICENSE"
    )

    candidates = [
        Path(__file__).resolve().parent.parent.parent / "LICENSE",
        _LICENSES_DIR / str(game_meta.get("vendored_path") or "LICENSE"),
    ]
    for cand in candidates:
        if cand.is_file():
            raw = cand.read_bytes()
            text = raw.decode("utf-8", errors="replace").strip()
            if text:
                return source_url, _sha256_bytes(raw), text

    etv_dist = dists_by_norm.get("escape-the-valley")
    if etv_dist is not None:
        files = _extract_dist_license_files(etv_dist)
        if files:
            _, fsha, text = files[0]
            return source_url, fsha, text

    raise FileNotFoundError(
        "Escape the Valley LICENSE file not found in repository or _licenses/."
    )


def generate_third_party_notices(
    distributions: list[md.Distribution] | None = None,
    *,
    target: str | None = None,
    include_only_dists: set[str] | None = None,
) -> str:
    """Generate the full THIRD_PARTY_NOTICES.txt content from installed metadata and vendored files.

    Contains no local filesystem paths.
    """
    sources = _load_sources_manifest()
    resolved_target = _verify_vendored_licenses_match(sources, target=target)

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
    if include_only_dists is not None:
        allowed_norms = {_norm(x) for x in include_only_dists}
        runtime_dists = [
            d
            for d in runtime_dists
            if _norm(d.metadata.get("Name") or "") in allowed_norms
        ]

    pyinstaller_dist = dists_by_norm.get("pyinstaller")
    py_ver = sys.version.split()[0]
    cpython_blocks = _read_cpython_license_blocks(sources, resolved_target)
    game_src_url, game_sha, game_text = _read_game_license_block(
        sources, dists_by_norm
    )

    sep = "=" * 80
    lines: list[str] = [
        sep,
        f"Escape the Valley (escape-the-valley {__version__}) — License & Third-Party Notices",
        f"Source: {game_src_url}",
        f"SHA256: {game_sha}",
        sep,
        "",
        game_text,
        "",
        sep,
        "Name: Python (CPython Runtime and Standard Library)",
        f"Version: {py_ver}",
        f"Target: {resolved_target} (python-build-standalone {sources['pbs_build_tag']})",
        "License: PSF-2.0 (Python Software Foundation License Version 2 + Incorporated Software)",
        sep,
        "",
    ]

    for label, src_url, fsha, text in cpython_blocks:
        lines.append(f"--- {label} ---")
        lines.append(f"Source: {src_url}")
        lines.append(f"SHA256: {fsha}")
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
        pyi_src_url = f"https://pypi.org/project/pyinstaller/{pyi_ver}/#files"
        lines.extend([
            sep,
            "Name: PyInstaller (Bootloader and Runtime Hooks)",
            f"Version: {pyi_ver}",
            f"License: {pyi_lic}",
            f"Source: {pyi_src_url}",
            sep,
            "",
        ])
        for label, fsha, text in pyi_files:
            lines.append(f"--- {label} ---")
            lines.append(f"Source: {pyi_src_url} ({label})")
            lines.append(f"SHA256: {fsha}")
            lines.append(text)
            lines.append("")

    for d in runtime_dists:
        name = d.metadata.get("Name") or "Unknown"
        ver = d.metadata.get("Version") or "Unknown"
        lic = _resolve_license_label(d)
        lic_files = _extract_dist_license_files(d)
        dist_src_line = _format_source_line(name, ver, d)
        pypi_files_url = f"https://pypi.org/project/{name}/{ver}/#files"

        header_lines = [
            sep,
            f"Name: {name}",
            f"Version: {ver}",
            f"License: {lic}",
            dist_src_line,
            sep,
            "",
        ]
        lines.extend(header_lines)

        if lic_files:
            for label, fsha, text in lic_files:
                lines.append(f"--- {label} ---")
                lines.append(f"Source: {pypi_files_url} ({label})")
                lines.append(f"SHA256: {fsha}")
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


def collect_toc_bundled_distributions(
    build_dir: str | Path,
    distributions: list[md.Distribution] | None = None,
) -> list[str]:
    """Return sorted normalized names of site-packages dists in PyInstaller's TOC."""
    build_root = Path(build_dir)
    toc_files = sorted(build_root.rglob("*.toc")) if build_root.is_dir() else []
    if not toc_files:
        raise FileNotFoundError(
            f"No PyInstaller .toc files found under {build_root}"
        )

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

    return sorted(bundled_dists)


def prune_notices_to_toc(
    build_dir: str | Path,
    notices_path: str | Path = "THIRD_PARTY_NOTICES.txt",
    distributions: list[md.Distribution] | None = None,
    *,
    target: str | None = None,
) -> list[str]:
    """Regenerate `notices_path` pruned to the exact distributions present in `build_dir`'s TOC."""
    bundled = collect_toc_bundled_distributions(
        build_dir, distributions=distributions
    )
    content = generate_third_party_notices(
        distributions=distributions,
        target=target,
        include_only_dists=set(bundled),
    )
    Path(notices_path).write_text(content, encoding="utf-8")
    return bundled


def verify_toc_against_notices(
    build_dir: str | Path,
    notices_path: str | Path = "THIRD_PARTY_NOTICES.txt",
    distributions: list[md.Distribution] | None = None,
) -> list[str]:
    """Verify the site-packages distributions in PyInstaller's TOC match `notices_path` both ways.

    Raises `RuntimeError` if any bundled third-party distribution is missing from
    `notices_path` OR if `notices_path` lists any unbundled third-party distribution.
    """
    bundled_list = collect_toc_bundled_distributions(
        build_dir, distributions=distributions
    )
    bundled_dists = set(bundled_list)

    notices_file = Path(notices_path)
    if not notices_file.is_file():
        raise FileNotFoundError(f"Notices file not found: {notices_file}")

    notices_text = notices_file.read_text(encoding="utf-8", errors="replace")
    non_site_package_entries = {
        _norm("Python (CPython Runtime and Standard Library)"),
        _norm("PyInstaller (Bootloader and Runtime Hooks)"),
        "pyinstaller",
        "python",
    }
    noticed_norms: set[str] = set()
    for m in re.finditer(r"^Name:\s*(.+)$", notices_text, flags=re.MULTILINE):
        raw_entry = m.group(1).strip()
        norm_entry = _norm(raw_entry)
        if norm_entry not in non_site_package_entries:
            noticed_norms.add(norm_entry)

    missing = sorted(d for d in bundled_dists if d not in noticed_norms)
    if missing:
        raise RuntimeError(
            f"Bundled site-packages distributions in PyInstaller TOC missing "
            f"from {notices_file}: {', '.join(missing)}"
        )

    extra = sorted(d for d in noticed_norms if d not in bundled_dists)
    if extra:
        raise RuntimeError(
            f"Unbundled site-packages distributions in {notices_file} absent "
            f"from PyInstaller TOC: {', '.join(extra)}"
        )

    return bundled_list


def write_third_party_notices(
    output_path: str | Path = "THIRD_PARTY_NOTICES.txt",
    *,
    target: str | None = None,
    toc_build_dir: str | Path | None = None,
    distributions: list[md.Distribution] | None = None,
) -> Path:
    """Generate and write THIRD_PARTY_NOTICES.txt to output_path."""
    path = Path(output_path)
    if toc_build_dir is not None:
        prune_notices_to_toc(
            toc_build_dir,
            path,
            distributions=distributions,
            target=target,
        )
        return path
    content = generate_third_party_notices(
        distributions=distributions,
        target=target,
    )
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
