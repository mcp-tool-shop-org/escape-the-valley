#!/usr/bin/env python3
"""Reproducible generator for Escape the Valley Rust third-party license notices.

Reads:
  - rust/Cargo.lock
  - rust/etv-core/Cargo.toml
  - LICENSE
  - rust/etv-core/content/content_manifest.json
  - rust/etv-core/content/event_skeletons.json
  - Each compiled crate's Cargo.toml and license files from the local Cargo
    registry source ($CARGO_HOME/registry/src/index.crates.io-*/<name>-<version>/)

Writes LF-only output in deterministic sorted order:
  - rust/etv-core/notices/notices_manifest.json
  - rust/etv-core/notices/THIRD_PARTY_NOTICES.txt

Usage:
  python rust/tools/gen_notices.py           # regenerate checked-in files
  python rust/tools/gen_notices.py --check   # verify checked-in files match byte-for-byte
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import pathlib
import re
import subprocess
import sys

SCRIPT_DIR = pathlib.Path(__file__).resolve().parent
RUST_DIR = SCRIPT_DIR.parent
REPO_ROOT = RUST_DIR.parent
NOTICES_DIR = RUST_DIR / "etv-core" / "notices"

TARGETS = [
    "x86_64-pc-windows-msvc",
    "x86_64-unknown-linux-gnu",
    "aarch64-apple-darwin",
]

TARGET_PROPS = {
    "x86_64-pc-windows-msvc": {
        "target_arch": "x86_64",
        "target_os": "windows",
        "target_family": "windows",
        "target_env": "msvc",
        "target_vendor": "pc",
        "flags": {"windows"},
    },
    "x86_64-unknown-linux-gnu": {
        "target_arch": "x86_64",
        "target_os": "linux",
        "target_family": "unix",
        "target_env": "gnu",
        "target_vendor": "unknown",
        "flags": {"unix"},
    },
    "aarch64-apple-darwin": {
        "target_arch": "aarch64",
        "target_os": "macos",
        "target_family": "unix",
        "target_env": "",
        "target_vendor": "apple",
        "flags": {"unix"},
    },
}

LICENSE_FILE_RE = re.compile(
    r"^(LICENSE|LICENCE|UNLICENSE|COPYING|COPYRIGHT)", re.IGNORECASE
)


def normalize_lf(raw_bytes: bytes) -> tuple[str, str]:
    text = raw_bytes.decode("utf-8", errors="replace")
    text = text.replace("\r\n", "\n").replace("\r", "\n")
    lines = [line.rstrip(" \t") for line in text.split("\n")]
    normalized = "\n".join(lines).strip("\n") + "\n"
    sha = hashlib.sha256(normalized.encode("utf-8")).hexdigest()
    return normalized, sha


def parse_cargo_lock(lock_text: str) -> list[dict]:
    packages = []
    for block in lock_text.split("[[package]]")[1:]:
        name_m = re.search(r'^name\s*=\s*"([^"]+)"', block, re.M)
        ver_m = re.search(r'^version\s*=\s*"([^"]+)"', block, re.M)
        src_m = re.search(r'^source\s*=\s*"([^"]+)"', block, re.M)
        chk_m = re.search(r'^checksum\s*=\s*"([^"]+)"', block, re.M)
        deps_m = re.search(r"^dependencies\s*=\s*\[(.*?)\]", block, re.M | re.S)
        deps = []
        if deps_m:
            deps = re.findall(r'"([^"]+)"', deps_m.group(1))
        if name_m and ver_m:
            packages.append(
                {
                    "name": name_m.group(1),
                    "version": ver_m.group(1),
                    "source": src_m.group(1) if src_m else None,
                    "checksum": chk_m.group(1) if chk_m else None,
                    "dependencies": deps,
                }
            )
    return packages


def parse_non_dev_dep_names_from_toml(toml_text: str) -> set[str]:
    allowed: set[str] = set()
    in_non_dev_deps = False
    for raw_line in toml_text.splitlines():
        line = raw_line.split("#", 1)[0].strip()
        if not line:
            continue
        if line.startswith("[") and line.endswith("]"):
            header = line[1:-1].strip()
            if header in ("dependencies", "build-dependencies") or (
                header.startswith("target.")
                and (
                    header.endswith(".dependencies")
                    or header.endswith(".build-dependencies")
                )
                and not header.endswith(".dev-dependencies")
            ):
                in_non_dev_deps = True
            else:
                in_non_dev_deps = False
            continue
        if in_non_dev_deps and "=" in line:
            key = line.split("=", 1)[0].strip().strip('"').strip("'")
            if key:
                allowed.add(key)
    return allowed


def eval_cfg_expr(expr: str, target: str) -> bool:
    expr = expr.strip()
    if expr.startswith("cfg(") and expr.endswith(")"):
        expr = expr[4:-1].strip()
    props = TARGET_PROPS[target]
    if expr == target:
        return True
    if expr in ("unix", "windows"):
        return expr in props["flags"]
    if expr.startswith("not(") and expr.endswith(")"):
        return not eval_cfg_expr(expr[4:-1], target)
    if expr.startswith("all(") and expr.endswith(")"):
        parts = split_top_level_commas(expr[4:-1])
        return all(eval_cfg_expr(p, target) for p in parts if p.strip())
    if expr.startswith("any(") and expr.endswith(")"):
        parts = split_top_level_commas(expr[4:-1])
        return any(eval_cfg_expr(p, target) for p in parts if p.strip())
    kv_m = re.match(r'^([a-zA-Z0-9_]+)\s*=\s*"([^"]*)"$', expr)
    if kv_m:
        k, v = kv_m.group(1), kv_m.group(2)
        if k in props:
            return props[k] == v
        return False
    return False


def split_top_level_commas(s: str) -> list[str]:
    out = []
    depth = 0
    cur = []
    for ch in s:
        if ch == "(":
            depth += 1
            cur.append(ch)
        elif ch == ")":
            depth -= 1
            cur.append(ch)
        elif ch == "," and depth == 0:
            out.append("".join(cur))
            cur = []
        else:
            cur.append(ch)
    if cur:
        out.append("".join(cur))
    return out


def parse_crate_dep_targets(crate_toml_text: str) -> dict[str, set[str]]:
    """Map dependency crate/package key -> set of TARGETS where that dep is active."""
    dep_targets: dict[str, set[str]] = {}
    current_targets: set[str] = set()
    in_dep_table = False

    for raw_line in crate_toml_text.splitlines():
        line = raw_line.split("#", 1)[0].strip()
        if not line:
            continue
        if line.startswith("[") and line.endswith("]"):
            header = line[1:-1].strip()
            in_dep_table = False
            current_targets = set()
            if header in ("dependencies", "build-dependencies"):
                in_dep_table = True
                current_targets = set(TARGETS)
            elif header.startswith("dependencies.") or header.startswith(
                "build-dependencies."
            ):
                dep_key = header.split(".", 1)[1].strip().strip('"').strip("'")
                dep_targets.setdefault(dep_key, set()).update(TARGETS)
            elif header.startswith("target."):
                m = re.match(
                    r"^target\.(?:'([^']+)'|\"([^\"]+)\"|([^\.]+))\.(dependencies|build-dependencies)(?:\.([^\s]+))?$",
                    header,
                )
                if m:
                    cfg_str = m.group(1) or m.group(2) or m.group(3)
                    matched = {t for t in TARGETS if eval_cfg_expr(cfg_str, t)}
                    sub_dep = m.group(5)
                    if sub_dep:
                        dep_targets.setdefault(
                            sub_dep.strip('"').strip("'"), set()
                        ).update(matched)
                    else:
                        in_dep_table = True
                        current_targets = matched
            continue
        if in_dep_table and "=" in line and not line.startswith("{"):
            key = line.split("=", 1)[0].strip().strip('"').strip("'")
            if key:
                dep_targets.setdefault(key, set()).update(current_targets)
    return dep_targets


def parse_package_metadata_from_toml(crate_toml_text: str) -> tuple[str, str]:
    in_pkg = False
    lic = ""
    repo = ""
    for raw_line in crate_toml_text.splitlines():
        line = raw_line.split("#", 1)[0].strip()
        if not line:
            continue
        if line.startswith("[") and line.endswith("]"):
            in_pkg = line[1:-1].strip() == "package"
            continue
        if in_pkg and "=" in line:
            k, v = line.split("=", 1)
            k = k.strip()
            v = v.strip()
            if k == "license" and v.startswith('"') and v.endswith('"'):
                lic = v[1:-1]
            elif k == "repository" and v.startswith('"') and v.endswith('"'):
                repo = v[1:-1]
    return lic, repo


def find_local_registry_src_dirs() -> list[pathlib.Path]:
    cargo_home = pathlib.Path(
        os.environ.get("CARGO_HOME") or (pathlib.Path.home() / ".cargo")
    )
    src_root = cargo_home / "registry" / "src"
    if not src_root.is_dir():
        return []
    return sorted(
        [d for d in src_root.iterdir() if d.is_dir()], key=lambda p: p.name
    )


def locate_crate_dir(
    index_dirs: list[pathlib.Path], name: str, version: str
) -> pathlib.Path:
    for idx_dir in index_dirs:
        cand = idx_dir / f"{name}-{version}"
        if cand.is_dir():
            return cand
    # Fallback: ask cargo metadata (--locked --offline) to ensure local cache is unpacked
    subprocess.run(
        [
            "cargo",
            "metadata",
            "--manifest-path",
            str(RUST_DIR / "Cargo.toml"),
            "--format-version",
            "1",
            "--locked",
            "--offline",
        ],
        check=True,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    for idx_dir in find_local_registry_src_dirs():
        cand = idx_dir / f"{name}-{version}"
        if cand.is_dir():
            return cand
    raise RuntimeError(
        f"crate directory {name}-{version} not found in local Cargo registry source"
    )


def resolve_dep_entry(
    dep_spec: str, by_name: dict[str, list[dict]]
) -> dict | None:
    parts = dep_spec.split()
    name = parts[0]
    candidates = by_name.get(name, [])
    if not candidates:
        return None
    if len(parts) >= 2:
        ver = parts[1]
        for c in candidates:
            if c["version"] == ver:
                return c
    return candidates[0]


def generate_notices() -> tuple[dict, str, str]:
    lock_text = (RUST_DIR / "Cargo.lock").read_text(encoding="utf-8")
    etv_toml_text = (RUST_DIR / "etv-core" / "Cargo.toml").read_text(
        encoding="utf-8"
    )
    packages = parse_cargo_lock(lock_text)
    by_name: dict[str, list[dict]] = {}
    for p in packages:
        by_name.setdefault(p["name"], []).append(p)

    non_dev_root_deps = parse_non_dev_dep_names_from_toml(etv_toml_text)
    index_dirs = find_local_registry_src_dirs()

    # Track which targets reach each (name, version)
    reached_targets: dict[tuple[str, str], set[str]] = {}
    queue: list[tuple[dict, set[str]]] = []

    etv_pkg = None
    for p in packages:
        if p["source"] is None:
            if p["name"] == "etv-core":
                etv_pkg = p
            for dep_spec in p["dependencies"]:
                dep_name = dep_spec.split()[0]
                if dep_name not in non_dev_root_deps:
                    continue
                target_pkg = resolve_dep_entry(dep_spec, by_name)
                if target_pkg and target_pkg["source"] is not None:
                    queue.append((target_pkg, set(TARGETS)))

    assert etv_pkg is not None, "etv-core package not found in Cargo.lock"

    crate_toml_cache: dict[tuple[str, str], str] = {}
    while queue:
        pkg, active_targets = queue.pop(0)
        key = (pkg["name"], pkg["version"])
        prev = reached_targets.get(key, set())
        new_targets = active_targets - prev
        if not new_targets:
            continue
        reached_targets[key] = prev | new_targets

        if key not in crate_toml_cache:
            cdir = locate_crate_dir(index_dirs, pkg["name"], pkg["version"])
            crate_toml_cache[key] = (cdir / "Cargo.toml").read_text(
                encoding="utf-8"
            )
        dep_target_map = parse_crate_dep_targets(crate_toml_cache[key])

        for dep_spec in pkg["dependencies"]:
            dep_pkg = resolve_dep_entry(dep_spec, by_name)
            if dep_pkg is None or dep_pkg["source"] is None:
                continue
            dep_name = dep_pkg["name"]
            allowed_for_edge = dep_target_map.get(dep_name, set(TARGETS))
            propagated = reached_targets[key] & allowed_for_edge
            if propagated:
                queue.append((dep_pkg, propagated))

    repo_license_text, repo_license_sha = normalize_lf(
        (REPO_ROOT / "LICENSE").read_bytes()
    )

    bundled_files = []
    for rel_path, desc in [
        (
            "rust/etv-core/content/content_manifest.json",
            "Embedded event and callout content manifest (include_str! in etv-core)",
        ),
        (
            "rust/etv-core/content/event_skeletons.json",
            "Embedded 200 JSON event skeletons (include_str! in etv-core)",
        ),
    ]:
        full_path = REPO_ROOT / rel_path
        _, file_sha = normalize_lf(full_path.read_bytes())
        bundled_files.append(
            {
                "path": rel_path,
                "description": desc,
                "license": "MIT",
                "sha256": file_sha,
                "license_filename": "LICENSE",
                "license_sha256": repo_license_sha,
                "license_text": repo_license_text,
            }
        )

    pkg_lookup = {(p["name"], p["version"]): p for p in packages}
    crates_list = []
    for name, ver in sorted(
        reached_targets.keys(), key=lambda nv: (nv[0].lower(), nv[0], nv[1])
    ):
        p = pkg_lookup[(name, ver)]
        cdir = locate_crate_dir(index_dirs, name, ver)
        lic_expr, repo_url = parse_package_metadata_from_toml(
            crate_toml_cache[(name, ver)]
        )
        lic_files = []
        for child in sorted(
            cdir.iterdir(), key=lambda c: (c.name.lower(), c.name)
        ):
            if child.is_file() and LICENSE_FILE_RE.match(child.name):
                norm_text, norm_sha = normalize_lf(child.read_bytes())
                lic_files.append(
                    {
                        "filename": child.name,
                        "sha256": norm_sha,
                        "text": norm_text,
                    }
                )
        if not lic_files:
            raise RuntimeError(f"no license files found for {name} {ver}")
        crates_list.append(
            {
                "name": name,
                "version": ver,
                "license": lic_expr,
                "repository": repo_url
                or f"https://crates.io/crates/{name}/{ver}",
                "cargo_lock_checksum": p["checksum"] or "",
                "targets": sorted(reached_targets[(name, ver)]),
                "license_files": lic_files,
            }
        )

    etv_lic, _ = parse_package_metadata_from_toml(etv_toml_text)
    manifest = {
        "schema_version": 1,
        "package": {
            "name": "etv-core",
            "version": etv_pkg["version"],
            "license": etv_lic or "MIT",
            "source": "https://github.com/mcp-tool-shop-org/escape-the-valley/blob/rust-2.0/LICENSE",
            "license_filename": "LICENSE",
            "license_sha256": repo_license_sha,
            "license_text": repo_license_text,
        },
        "targets": TARGETS,
        "bundled_data_files": bundled_files,
        "crates": crates_list,
    }

    manifest_json = json.dumps(manifest, indent=2, ensure_ascii=True) + "\n"
    notices_txt = render_notices_text(manifest)
    return manifest, manifest_json, notices_txt


def render_notices_text(manifest: dict) -> str:
    sep_eq = "=" * 80
    out_lines: list[str] = [
        sep_eq,
        f"Escape the Valley ({manifest['package']['name']} {manifest['package']['version']}) \u2014 License & Third-Party Notices",
        f"Source: {manifest['package']['source']}",
        f"SHA256: {manifest['package']['license_sha256']}",
        sep_eq,
        "",
        manifest["package"]["license_text"].rstrip("\n"),
        "",
        sep_eq,
        "Summary of Bundled Data Files",
        sep_eq,
        "",
    ]
    for bf in manifest["bundled_data_files"]:
        out_lines.append(
            f"- {bf['path']} ({bf['license']}, sha256: {bf['sha256']})"
        )
    out_lines.extend(
        [
            "",
            sep_eq,
            "Summary of Included Third-Party Distributions",
            sep_eq,
            "",
        ]
    )
    for c in manifest["crates"]:
        out_lines.append(f"- {c['name']} {c['version']} ({c['license']})")

    for bf in manifest["bundled_data_files"]:
        out_lines.extend(
            [
                "",
                sep_eq,
                f"Bundled Data File: {bf['path']}",
                f"Description: {bf['description']}",
                f"License: {bf['license']}",
                f"Data SHA256: {bf['sha256']}",
                sep_eq,
                "",
                f"--- {bf['license_filename']} ---",
                f"Source: {bf['path']} ({bf['license_filename']})",
                f"SHA256: {bf['license_sha256']}",
                bf["license_text"].rstrip("\n"),
            ]
        )

    for c in manifest["crates"]:
        out_lines.extend(
            [
                "",
                sep_eq,
                f"Name: {c['name']}",
                f"Version: {c['version']}",
                f"License: {c['license']}",
                f"Source: {c['name']}=={c['version']} (Cargo.lock sha256: {c['cargo_lock_checksum']}) | Upstream: {c['repository']}",
                f"Targets: {', '.join(c['targets'])}",
                sep_eq,
            ]
        )
        for lf in c["license_files"]:
            out_lines.extend(
                [
                    "",
                    f"--- {lf['filename']} ---",
                    f"Source: {c['name']}-{c['version']}/{lf['filename']}",
                    f"SHA256: {lf['sha256']}",
                    lf["text"].rstrip("\n"),
                ]
            )

    return "\n".join(out_lines) + "\n"


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Generate or verify Escape the Valley Rust third-party notices."
    )
    parser.add_argument(
        "--check",
        action="store_true",
        help="Verify that checked-in notices_manifest.json and THIRD_PARTY_NOTICES.txt match generator output byte-for-byte.",
    )
    args = parser.parse_args()

    _, manifest_json, notices_txt = generate_notices()
    manifest_path = NOTICES_DIR / "notices_manifest.json"
    notices_path = NOTICES_DIR / "THIRD_PARTY_NOTICES.txt"

    if args.check:
        disk_manifest = manifest_path.read_bytes()
        disk_notices = notices_path.read_bytes()
        if disk_manifest != manifest_json.encode("utf-8"):
            print(
                "ERROR: notices_manifest.json does not match generator output byte-for-byte",
                file=sys.stderr,
            )
            return 1
        if disk_notices != notices_txt.encode("utf-8"):
            print(
                "ERROR: THIRD_PARTY_NOTICES.txt does not match generator output byte-for-byte",
                file=sys.stderr,
            )
            return 1
        print("OK: notices_manifest.json and THIRD_PARTY_NOTICES.txt match byte-for-byte")
        return 0

    NOTICES_DIR.mkdir(parents=True, exist_ok=True)
    manifest_path.write_bytes(manifest_json.encode("utf-8"))
    notices_path.write_bytes(notices_txt.encode("utf-8"))
    print(
        f"Wrote {manifest_path.relative_to(REPO_ROOT)} and {notices_path.relative_to(REPO_ROOT)}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
