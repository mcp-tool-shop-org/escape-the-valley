//! Third-party license notices (`etv --notices`), allow-list validation, `Cargo.lock` compiled
//! crate coverage verification, and deterministic LF-only notice generation/rendering for all
//! crates and bundled data files compiled into the binary.
//!
//! # Reproducible Notices Generation
//!
//! `rust/etv-core/notices/notices_manifest.json` and `rust/etv-core/notices/THIRD_PARTY_NOTICES.txt`
//! are generated from `rust/Cargo.lock`, `rust/etv-core/Cargo.toml`, `LICENSE`, the bundled JSON
//! content files, and each compiled crate's license files in the local Cargo registry source
//! (`$CARGO_HOME/registry/src/index.crates.io-*/<name>-<version>/`).
//!
//! Each license text is tied to its crate source via:
//! 1. `cargo_lock_checksum`: the SHA-256 checksum of `<name>-<version>.crate` recorded in
//!    `rust/Cargo.lock`, and
//! 2. `license_files[].sha256`: the SHA-256 digest of the LF-normalized license file read from
//!    `$CARGO_HOME/registry/src/index.crates.io-*/<name>-<version>/<filename>`.
//!
//! Regenerate or verify from the repository root with:
//! ```bash
//! python rust/tools/gen_notices.py
//! python rust/tools/gen_notices.py --check
//! ```
//!
//! All collections use [`Vec`] and [`BTreeMap`] only (no `HashMap` or `HashSet`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::content::hex_encode_32;

/// Checked-in generated third-party notices document (LF-only).
pub const EMBEDDED_THIRD_PARTY_NOTICES_TXT: &str =
    include_str!("../notices/THIRD_PARTY_NOTICES.txt");

/// Checked-in machine-readable manifest of crates and bundled data files compiled into `etv`.
pub const EMBEDDED_NOTICES_MANIFEST_JSON: &str = include_str!("../notices/notices_manifest.json");

/// Checked-in permissive SPDX license allow-list.
pub const EMBEDDED_LICENSE_ALLOWLIST_JSON: &str = include_str!("../notices/license_allowlist.json");

/// Supported release target triples whose union defines the compiled-in crate set.
pub const RELEASE_TARGETS: [&str; 3] = [
    "x86_64-pc-windows-msvc",
    "x86_64-unknown-linux-gnu",
    "aarch64-apple-darwin",
];

/// First-party package entry in [`NoticesManifest`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FirstPartyPackageNotice {
    pub name: String,
    pub version: String,
    pub license: String,
    pub source: String,
    pub license_filename: String,
    pub license_sha256: String,
    pub license_text: String,
}

/// Bundled data file entry in [`NoticesManifest`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BundledDataFileNotice {
    pub path: String,
    pub description: String,
    pub license: String,
    pub sha256: String,
    pub license_filename: String,
    pub license_sha256: String,
    pub license_text: String,
}

/// Individual license file belonging to a compiled-in crate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrateLicenseFile {
    pub filename: String,
    pub sha256: String,
    pub text: String,
}

/// Third-party crate entry in [`NoticesManifest`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrateNotice {
    pub name: String,
    pub version: String,
    pub license: String,
    pub repository: String,
    pub cargo_lock_checksum: String,
    pub targets: Vec<String>,
    pub license_files: Vec<CrateLicenseFile>,
}

/// Full manifest of first-party package, bundled data files, and third-party crates compiled
/// into the `etv` binary across release targets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoticesManifest {
    pub schema_version: u32,
    pub package: FirstPartyPackageNotice,
    pub targets: Vec<String>,
    pub bundled_data_files: Vec<BundledDataFileNotice>,
    pub crates: Vec<CrateNotice>,
}

/// Pinned crate entry in [`LicenseAllowlist::reviewed_crates`] recording version and license
/// re-read at pin time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewedCratePin {
    pub name: String,
    pub version: String,
    pub license: String,
    pub reviewed_on: String,
}

/// Repository SPDX license allow-list for crates and bundled data files.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LicenseAllowlist {
    pub schema_version: u32,
    pub description: String,
    pub allowed_spdx_identifiers: Vec<String>,
    pub allowed_license_expressions: Vec<String>,
    #[serde(default)]
    pub reviewed_crates: Vec<ReviewedCratePin>,
    pub prohibited_copyleft_prefixes: Vec<String>,
}

/// Validation failure returned by [`validate_notices_against_allowlist`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoticesValidationError {
    pub subject: String,
    pub code: String,
    pub message: String,
}

/// Parsed `[[package]]` block from `rust/Cargo.lock`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CargoLockPackage {
    pub name: String,
    pub version: String,
    pub source: Option<String>,
    pub checksum: Option<String>,
    pub dependencies: Vec<String>,
}

/// Normalize raw bytes to canonical LF-only text (stripping trailing horizontal whitespace on
/// each line and ensuring a single trailing `\n`) and compute its hex SHA-256 digest.
///
/// Mirrors `_normalize_license_text_and_sha256` in legacy `src/escape_the_valley/notices.py:109-125`.
pub fn normalize_lf_and_sha256(raw: &[u8]) -> (String, String) {
    let decoded = String::from_utf8_lossy(raw);
    let lf = decoded.replace("\r\n", "\n").replace('\r', "\n");
    let trimmed_lines: Vec<&str> = lf
        .split('\n')
        .map(|l| l.trim_end_matches([' ', '\t']))
        .collect();
    let joined = trimmed_lines.join("\n");
    let normalized = format!("{}\n", joined.trim_matches('\n'));

    let mut hasher = Sha256::new();
    hasher.update(normalized.as_bytes());
    let digest = hasher.finalize();
    let mut bytes = [0u8; 32];
    bytes.copy_from_slice(&digest);
    (normalized, hex_encode_32(&bytes))
}

/// Parse the embedded [`NoticesManifest`].
pub fn embedded_notices_manifest() -> NoticesManifest {
    serde_json::from_str(EMBEDDED_NOTICES_MANIFEST_JSON)
        .expect("embedded notices_manifest.json must be valid")
}

/// Parse the embedded [`LicenseAllowlist`].
pub fn embedded_license_allowlist() -> LicenseAllowlist {
    serde_json::from_str(EMBEDDED_LICENSE_ALLOWLIST_JSON)
        .expect("embedded license_allowlist.json must be valid")
}

/// Return the canonical LF-only third-party notices text shipped with the `etv` binary.
pub fn third_party_notices_text() -> String {
    let lf = EMBEDDED_THIRD_PARTY_NOTICES_TXT
        .replace("\r\n", "\n")
        .replace('\r', "\n");
    if lf.ends_with('\n') {
        lf
    } else {
        format!("{lf}\n")
    }
}

/// Deterministically render the complete LF-only `THIRD_PARTY_NOTICES.txt` document from a
/// [`NoticesManifest`].
pub fn render_third_party_notices(manifest: &NoticesManifest) -> String {
    let sep_eq = "=".repeat(80);
    let mut out_lines: Vec<String> = vec![
        sep_eq.clone(),
        format!(
            "Escape the Valley ({} {}) \u{2014} License & Third-Party Notices",
            manifest.package.name, manifest.package.version
        ),
        format!("Source: {}", manifest.package.source),
        format!("SHA256: {}", manifest.package.license_sha256),
        sep_eq.clone(),
        String::new(),
        manifest
            .package
            .license_text
            .trim_end_matches('\n')
            .to_string(),
        String::new(),
        sep_eq.clone(),
        "Summary of Bundled Data Files".to_string(),
        sep_eq.clone(),
        String::new(),
    ];

    for bf in &manifest.bundled_data_files {
        out_lines.push(format!(
            "- {} ({}, sha256: {})",
            bf.path, bf.license, bf.sha256
        ));
    }

    out_lines.extend([
        String::new(),
        sep_eq.clone(),
        "Summary of Included Third-Party Distributions".to_string(),
        sep_eq.clone(),
        String::new(),
    ]);

    for c in &manifest.crates {
        out_lines.push(format!("- {} {} ({})", c.name, c.version, c.license));
    }

    for bf in &manifest.bundled_data_files {
        out_lines.extend([
            String::new(),
            sep_eq.clone(),
            format!("Bundled Data File: {}", bf.path),
            format!("Description: {}", bf.description),
            format!("License: {}", bf.license),
            format!("Data SHA256: {}", bf.sha256),
            sep_eq.clone(),
            String::new(),
            format!("--- {} ---", bf.license_filename),
            format!("Source: {} ({})", bf.path, bf.license_filename),
            format!("SHA256: {}", bf.license_sha256),
            bf.license_text.trim_end_matches('\n').to_string(),
        ]);
    }

    for c in &manifest.crates {
        out_lines.extend([
            String::new(),
            sep_eq.clone(),
            format!("Name: {}", c.name),
            format!("Version: {}", c.version),
            format!("License: {}", c.license),
            format!(
                "Source: {}=={} (Cargo.lock sha256: {}) | Upstream: {}",
                c.name, c.version, c.cargo_lock_checksum, c.repository
            ),
            format!("Targets: {}", c.targets.join(", ")),
            sep_eq.clone(),
        ]);
        for lf in &c.license_files {
            out_lines.extend([
                String::new(),
                format!("--- {} ---", lf.filename),
                format!("Source: {}-{}/{}", c.name, c.version, lf.filename),
                format!("SHA256: {}", lf.sha256),
                lf.text.trim_end_matches('\n').to_string(),
            ]);
        }
    }

    format!("{}\n", out_lines.join("\n"))
}

fn extract_spdx_tokens(expr: &str) -> Vec<String> {
    let cleaned = expr.replace(['(', ')', '/'], " ");
    cleaned
        .split_whitespace()
        .filter(|&tok| tok != "OR" && tok != "AND" && tok != "WITH")
        .map(ToString::to_string)
        .collect()
}

fn validate_license_expression(
    subject: &str,
    expr: &str,
    allowlist: &LicenseAllowlist,
    errors: &mut Vec<NoticesValidationError>,
) {
    let trimmed = expr.trim();
    if trimmed.is_empty() {
        errors.push(NoticesValidationError {
            subject: subject.to_string(),
            code: "MISSING_LICENSE_EXPRESSION".to_string(),
            message: format!("'{subject}' has an empty SPDX license expression"),
        });
        return;
    }

    let tokens = extract_spdx_tokens(trimmed);
    for tok in &tokens {
        let upper = tok.to_ascii_uppercase();
        for copyleft in &allowlist.prohibited_copyleft_prefixes {
            let prefix = copyleft.to_ascii_uppercase();
            if upper == prefix || upper.starts_with(&format!("{prefix}-")) {
                errors.push(NoticesValidationError {
                    subject: subject.to_string(),
                    code: "COPYLEFT_LICENSE_PROHIBITED".to_string(),
                    message: format!(
                        "'{subject}' carries prohibited copyleft license token '{tok}' in '{trimmed}'"
                    ),
                });
            }
        }
        if !allowlist
            .allowed_spdx_identifiers
            .iter()
            .any(|allowed| allowed == tok)
        {
            errors.push(NoticesValidationError {
                subject: subject.to_string(),
                code: "UNALLOWED_SPDX_IDENTIFIER".to_string(),
                message: format!(
                    "'{subject}' uses SPDX identifier '{tok}' which is not on the allow-list"
                ),
            });
        }
    }

    if !allowlist
        .allowed_license_expressions
        .iter()
        .any(|allowed| allowed == trimmed)
    {
        errors.push(NoticesValidationError {
            subject: subject.to_string(),
            code: "UNALLOWED_LICENSE_EXPRESSION".to_string(),
            message: format!(
                "'{subject}' carries license expression '{trimmed}' which is not on the allow-list"
            ),
        });
    }
}

/// Validate a [`NoticesManifest`] against a [`LicenseAllowlist`].
///
/// Fails if:
/// - the allow-list itself contains any prohibited copyleft token,
/// - any crate or bundled data file has no license expression or no license text,
/// - any license text contains CR (`\r`) or mismatches its recorded SHA-256,
/// - any crate or bundled data file carries a license not on the allow-list or a copyleft license,
/// - crates are not sorted deterministically by `(name.to_lowercase(), name, version)`.
pub fn validate_notices_against_allowlist(
    manifest: &NoticesManifest,
    allowlist: &LicenseAllowlist,
) -> Result<(), Vec<NoticesValidationError>> {
    let mut errors = Vec::new();

    // Guardrail: ensure the allow-list itself never admits copyleft tokens.
    for allowed_id in &allowlist.allowed_spdx_identifiers {
        let upper = allowed_id.to_ascii_uppercase();
        for copyleft in &allowlist.prohibited_copyleft_prefixes {
            let prefix = copyleft.to_ascii_uppercase();
            if upper == prefix || upper.starts_with(&format!("{prefix}-")) {
                errors.push(NoticesValidationError {
                    subject: "license_allowlist.json".to_string(),
                    code: "ALLOWLIST_CONTAINS_COPYLEFT".to_string(),
                    message: format!(
                        "allow-list contains prohibited copyleft identifier '{allowed_id}'"
                    ),
                });
            }
        }
    }

    // Validate pinned reviewed_crates entries against the allow-list and manifest
    for pin in &allowlist.reviewed_crates {
        let pin_subj = format!("{}@{}", pin.name, pin.version);
        validate_license_expression(&pin_subj, &pin.license, allowlist, &mut errors);
        if let Some(found) = manifest.crates.iter().find(|c| c.name == pin.name) {
            if found.version != pin.version {
                errors.push(NoticesValidationError {
                    subject: pin_subj.clone(),
                    code: "PINNED_CRATE_VERSION_MISMATCH".to_string(),
                    message: format!(
                        "pinned crate '{}' expected version '{}', found '{}'",
                        pin.name, pin.version, found.version
                    ),
                });
            }
            if found.license != pin.license {
                errors.push(NoticesValidationError {
                    subject: pin_subj,
                    code: "PINNED_CRATE_LICENSE_MISMATCH".to_string(),
                    message: format!(
                        "pinned crate '{}' expected license '{}', found '{}'",
                        pin.name, pin.license, found.license
                    ),
                });
            }
        }
    }

    // Validate first-party package
    let pkg_subj = format!("{}@{}", manifest.package.name, manifest.package.version);
    validate_license_expression(&pkg_subj, &manifest.package.license, allowlist, &mut errors);
    if manifest.package.license_text.trim().is_empty() {
        errors.push(NoticesValidationError {
            subject: pkg_subj.clone(),
            code: "MISSING_LICENSE_TEXT".to_string(),
            message: format!("'{pkg_subj}' has empty license text"),
        });
    } else {
        let (_, sha) = normalize_lf_and_sha256(manifest.package.license_text.as_bytes());
        if sha != manifest.package.license_sha256 {
            errors.push(NoticesValidationError {
                subject: pkg_subj,
                code: "LICENSE_SHA256_MISMATCH".to_string(),
                message: "first-party package license SHA-256 mismatch".to_string(),
            });
        }
    }

    // Validate bundled data files
    for bf in &manifest.bundled_data_files {
        validate_license_expression(&bf.path, &bf.license, allowlist, &mut errors);
        if bf.license_text.trim().is_empty() {
            errors.push(NoticesValidationError {
                subject: bf.path.clone(),
                code: "MISSING_LICENSE_TEXT".to_string(),
                message: format!("bundled data file '{}' has no license text", bf.path),
            });
        }
        if bf.sha256.trim().is_empty() {
            errors.push(NoticesValidationError {
                subject: bf.path.clone(),
                code: "MISSING_DATA_SHA256".to_string(),
                message: format!("bundled data file '{}' has no data SHA-256", bf.path),
            });
        }
    }

    // Validate crates and deterministic sort order
    let mut prev_key: Option<(String, String, String)> = None;
    for c in &manifest.crates {
        let subj = format!("{}@{}", c.name, c.version);
        let sort_key = (c.name.to_lowercase(), c.name.clone(), c.version.clone());
        if let Some(prev) = &prev_key {
            if &sort_key <= prev {
                errors.push(NoticesValidationError {
                    subject: subj.clone(),
                    code: "UNSORTED_CRATE_ORDER".to_string(),
                    message: format!("crate '{subj}' is out of deterministic sort order"),
                });
            }
        }
        prev_key = Some(sort_key);

        validate_license_expression(&subj, &c.license, allowlist, &mut errors);

        if c.cargo_lock_checksum.trim().is_empty() {
            errors.push(NoticesValidationError {
                subject: subj.clone(),
                code: "MISSING_CARGO_LOCK_CHECKSUM".to_string(),
                message: format!("crate '{subj}' is missing its Cargo.lock checksum"),
            });
        }

        if c.license_files.is_empty() {
            errors.push(NoticesValidationError {
                subject: subj.clone(),
                code: "MISSING_LICENSE_TEXT".to_string(),
                message: format!("crate '{subj}' has no license files"),
            });
        }

        for lf in &c.license_files {
            if lf.text.trim().is_empty() {
                errors.push(NoticesValidationError {
                    subject: subj.clone(),
                    code: "MISSING_LICENSE_TEXT".to_string(),
                    message: format!(
                        "crate '{subj}' license file '{}' has empty text",
                        lf.filename
                    ),
                });
                continue;
            }
            if lf.text.contains('\r') {
                errors.push(NoticesValidationError {
                    subject: subj.clone(),
                    code: "NON_LF_LICENSE_TEXT".to_string(),
                    message: format!(
                        "crate '{subj}' license file '{}' contains CR bytes",
                        lf.filename
                    ),
                });
            }
            let (_, computed_sha) = normalize_lf_and_sha256(lf.text.as_bytes());
            if computed_sha != lf.sha256 {
                errors.push(NoticesValidationError {
                    subject: subj.clone(),
                    code: "LICENSE_SHA256_MISMATCH".to_string(),
                    message: format!(
                        "crate '{subj}' license file '{}' SHA-256 mismatch (expected {}, got {})",
                        lf.filename, lf.sha256, computed_sha
                    ),
                });
            }
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// Parse all `[[package]]` entries from `Cargo.lock` (including `source`, `checksum`, and
/// `dependencies`).
pub fn parse_cargo_lock_entries(cargo_lock_text: &str) -> Vec<CargoLockPackage> {
    let mut out = Vec::new();
    for block in cargo_lock_text.split("[[package]]").skip(1) {
        let mut name: Option<String> = None;
        let mut version: Option<String> = None;
        let mut source: Option<String> = None;
        let mut checksum: Option<String> = None;
        let mut dependencies: Vec<String> = Vec::new();
        let mut in_deps = false;

        for line in block.lines() {
            let trimmed = line.trim();
            if in_deps {
                if trimmed.starts_with(']') {
                    in_deps = false;
                } else {
                    let dep = trimmed.trim_end_matches(',').trim().trim_matches('"');
                    if !dep.is_empty() {
                        dependencies.push(dep.to_string());
                    }
                }
                continue;
            }
            if let Some(rest) = trimmed.strip_prefix("name = ") {
                name = Some(rest.trim_matches('"').to_string());
            } else if let Some(rest) = trimmed.strip_prefix("version = ") {
                version = Some(rest.trim_matches('"').to_string());
            } else if let Some(rest) = trimmed.strip_prefix("source = ") {
                source = Some(rest.trim_matches('"').to_string());
            } else if let Some(rest) = trimmed.strip_prefix("checksum = ") {
                checksum = Some(rest.trim_matches('"').to_string());
            } else if let Some(rest) = trimmed.strip_prefix("dependencies = [") {
                if let Some(inner) = rest.strip_suffix(']') {
                    for item in inner.split(',') {
                        let dep = item.trim().trim_matches('"');
                        if !dep.is_empty() {
                            dependencies.push(dep.to_string());
                        }
                    }
                } else {
                    in_deps = true;
                }
            }
        }
        if let (Some(n), Some(v)) = (name, version) {
            out.push(CargoLockPackage {
                name: n,
                version: v,
                source,
                checksum,
                dependencies,
            });
        }
    }
    out
}

/// Parse `[[package]]` blocks from `Cargo.lock` into a deterministic `BTreeMap` mapping
/// `(name, version)` to its optional `checksum`.
pub fn parse_cargo_lock_packages(
    cargo_lock_text: &str,
) -> BTreeMap<(String, String), Option<String>> {
    let mut out = BTreeMap::new();
    for pkg in parse_cargo_lock_entries(cargo_lock_text) {
        out.insert((pkg.name, pkg.version), pkg.checksum);
    }
    out
}

/// Extract non-dev dependency names (`[dependencies]` and `[build-dependencies]`, excluding
/// `[dev-dependencies]`) from a crate's `Cargo.toml`.
pub fn parse_non_dev_dependency_names(cargo_toml_text: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut in_non_dev_deps = false;

    for raw_line in cargo_toml_text.lines() {
        let line = raw_line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if let Some(header) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            let h = header.trim();
            in_non_dev_deps = h == "dependencies"
                || h == "build-dependencies"
                || (h.starts_with("target.")
                    && (h.ends_with(".dependencies") || h.ends_with(".build-dependencies"))
                    && !h.ends_with(".dev-dependencies"));
            continue;
        }
        if in_non_dev_deps {
            if let Some((key, _)) = line.split_once('=') {
                let k = key.trim().trim_matches('"').trim_matches('\'').to_string();
                if !k.is_empty() && !names.contains(&k) {
                    names.push(k);
                }
            }
        }
    }
    names
}

fn is_non_release_target_only_crate(name: &str) -> bool {
    // `getrandom` declares target-gated dependencies on `r-efi` (`target_os = "uefi"`) and
    // `wasip2` (`target_os = "wasi"` -> `wit-bindgen`). None of those targets are in
    // `RELEASE_TARGETS` (`x86_64-pc-windows-msvc`, `x86_64-unknown-linux-gnu`,
    // `aarch64-apple-darwin`).
    matches!(name, "r-efi" | "wasip2" | "wit-bindgen")
}

fn resolve_dep_package<'a>(
    dep_spec: &str,
    by_name: &'a BTreeMap<String, Vec<CargoLockPackage>>,
) -> Option<&'a CargoLockPackage> {
    let mut parts = dep_spec.split_whitespace();
    let name = parts.next()?;
    let candidates = by_name.get(name)?;
    if let Some(ver) = parts.next() {
        candidates.iter().find(|c| c.version == ver)
    } else {
        candidates.first()
    }
}

/// Compute the deterministic sorted list of `(name, version)` pairs for every third-party package
/// compiled into the `etv` binary from `rust/Cargo.lock` and `rust/etv-core/Cargo.toml`.
///
/// How packages are included and excluded:
/// 1. **Workspace crates excluded:** packages in `Cargo.lock` with `source == None` (`etv-core`)
///    are workspace members and are excluded from the third-party crate set.
/// 2. **Dev-only packages excluded:** root traversal from `etv-core` follows only dependency
///    names present in `[dependencies]` or `[build-dependencies]` of `rust/etv-core/Cargo.toml`
///    (excluding `[dev-dependencies]` `flate2` and `tempfile` and their transitive closure).
/// 3. **Union across release targets:** all transitive dependencies reachable across
///    [`RELEASE_TARGETS`] (`x86_64-pc-windows-msvc`, `x86_64-unknown-linux-gnu`,
///    `aarch64-apple-darwin`) are included, excluding only `uefi`/`wasi`-only leaf crates.
pub fn compute_compiled_crates_from_cargo_lock(
    cargo_lock_text: &str,
    etv_cargo_toml_text: &str,
) -> Vec<(String, String)> {
    let entries = parse_cargo_lock_entries(cargo_lock_text);
    let mut by_name: BTreeMap<String, Vec<CargoLockPackage>> = BTreeMap::new();
    for entry in &entries {
        by_name
            .entry(entry.name.clone())
            .or_default()
            .push(entry.clone());
    }

    let allowed_root_deps = parse_non_dev_dependency_names(etv_cargo_toml_text);
    let mut queue: Vec<CargoLockPackage> = Vec::new();
    let mut visited: BTreeMap<(String, String), ()> = BTreeMap::new();

    for entry in &entries {
        if entry.source.is_none() {
            for dep_spec in &entry.dependencies {
                let dep_name = dep_spec.split_whitespace().next().unwrap_or("");
                if !allowed_root_deps.iter().any(|d| d == dep_name) {
                    continue;
                }
                if let Some(pkg) = resolve_dep_package(dep_spec, &by_name) {
                    if pkg.source.is_some() && !is_non_release_target_only_crate(&pkg.name) {
                        queue.push(pkg.clone());
                    }
                }
            }
        }
    }

    while !queue.is_empty() {
        let pkg = queue.remove(0);
        let key = (pkg.name.clone(), pkg.version.clone());
        if visited.contains_key(&key) {
            continue;
        }
        visited.insert(key, ());

        for dep_spec in &pkg.dependencies {
            if let Some(dep_pkg) = resolve_dep_package(dep_spec, &by_name) {
                if dep_pkg.source.is_some() && !is_non_release_target_only_crate(&dep_pkg.name) {
                    let dep_key = (dep_pkg.name.clone(), dep_pkg.version.clone());
                    if !visited.contains_key(&dep_key) {
                        queue.push(dep_pkg.clone());
                    }
                }
            }
        }
    }

    let mut result: Vec<(String, String)> = visited.into_keys().collect();
    result.sort_by(|a, b| (a.0.to_lowercase(), &a.0, &a.1).cmp(&(b.0.to_lowercase(), &b.0, &b.1)));
    result
}

/// Compare the `(name, version)` set derived from `rust/Cargo.lock` against `manifest.crates`.
///
/// Returns `Ok(compiled_set)` when the sets are equal, or `Err(diagnostics)` naming every crate
/// that is missing from `notices_manifest.json`, extra in `notices_manifest.json`, or has a
/// version mismatch.
pub fn verify_manifest_coverage_against_cargo_lock(
    manifest: &NoticesManifest,
    cargo_lock_text: &str,
    etv_cargo_toml_text: &str,
) -> Result<Vec<(String, String)>, Vec<String>> {
    let lock_set = compute_compiled_crates_from_cargo_lock(cargo_lock_text, etv_cargo_toml_text);
    let manifest_set: Vec<(String, String)> = manifest
        .crates
        .iter()
        .map(|c| (c.name.clone(), c.version.clone()))
        .collect();

    let mut lock_by_name: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (n, v) in &lock_set {
        lock_by_name.entry(n.clone()).or_default().push(v.clone());
    }
    let mut manifest_by_name: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (n, v) in &manifest_set {
        manifest_by_name
            .entry(n.clone())
            .or_default()
            .push(v.clone());
    }

    let mut errors = Vec::new();

    for (name, lock_vers) in &lock_by_name {
        match manifest_by_name.get(name) {
            None => {
                for ver in lock_vers {
                    errors.push(format!(
                        "missing crate '{name}@{ver}': compiled in Cargo.lock but missing from notices_manifest.json"
                    ));
                }
            }
            Some(man_vers) if man_vers != lock_vers => {
                errors.push(format!(
                    "version mismatch for crate '{name}': Cargo.lock has [{}], notices_manifest.json has [{}]",
                    lock_vers.join(", "),
                    man_vers.join(", ")
                ));
            }
            Some(_) => {}
        }
    }

    for (name, man_vers) in &manifest_by_name {
        if !lock_by_name.contains_key(name) {
            for ver in man_vers {
                errors.push(format!(
                    "extra crate '{name}@{ver}': present in notices_manifest.json but not compiled in Cargo.lock"
                ));
            }
        }
    }

    if errors.is_empty() && lock_set == manifest_set {
        Ok(lock_set)
    } else {
        if errors.is_empty() {
            errors.push(
                "crate ordering in notices_manifest.json does not match canonical sort order"
                    .to_string(),
            );
        }
        Err(errors)
    }
}

fn find_local_cargo_registry_src_dirs() -> Vec<PathBuf> {
    let cargo_home = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("USERPROFILE")
                .or_else(|| std::env::var_os("HOME"))
                .map(|h| PathBuf::from(h).join(".cargo"))
        });
    let Some(home) = cargo_home else {
        return Vec::new();
    };
    let src_root = home.join("registry").join("src");
    let Ok(rd) = std::fs::read_dir(src_root) else {
        return Vec::new();
    };
    let mut dirs: Vec<PathBuf> = rd
        .filter_map(|e| e.ok().map(|d| d.path()))
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    dirs
}

fn is_license_filename(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    upper.starts_with("LICENSE")
        || upper.starts_with("LICENCE")
        || upper.starts_with("UNLICENSE")
        || upper.starts_with("COPYING")
        || upper.starts_with("COPYRIGHT")
}

fn parse_package_license_and_repo(crate_toml_text: &str) -> (String, String) {
    let mut in_package = false;
    let mut license = String::new();
    let mut repository = String::new();
    for raw_line in crate_toml_text.lines() {
        let line = raw_line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if let Some(header) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            in_package = header.trim() == "package";
            continue;
        }
        if in_package {
            if let Some((k, v)) = line.split_once('=') {
                let key = k.trim();
                let val = v.trim();
                if key == "license" && val.starts_with('"') && val.ends_with('"') {
                    license = val.trim_matches('"').to_string();
                } else if key == "repository" && val.starts_with('"') && val.ends_with('"') {
                    repository = val.trim_matches('"').to_string();
                }
            }
        }
    }
    (license, repository)
}

fn crate_targets_in_build(name: &str) -> Vec<String> {
    if name == "libc" {
        vec![
            "aarch64-apple-darwin".to_string(),
            "x86_64-unknown-linux-gnu".to_string(),
        ]
    } else {
        vec![
            "aarch64-apple-darwin".to_string(),
            "x86_64-pc-windows-msvc".to_string(),
            "x86_64-unknown-linux-gnu".to_string(),
        ]
    }
}

/// Run the reproducible notices generator from `repo_root`, reading `rust/Cargo.lock`,
/// `rust/etv-core/Cargo.toml`, `LICENSE`, the bundled content files, and each compiled crate's
/// `Cargo.toml` and license files from the local Cargo registry source
/// (`$CARGO_HOME/registry/src/index.crates.io-*/<name>-<version>/`).
///
/// Returns `(manifest, manifest_json, third_party_notices_txt)`.
pub fn generate_notices_from_workspace(
    repo_root: &Path,
) -> Result<(NoticesManifest, String, String), String> {
    let lock_path = repo_root.join("rust/Cargo.lock");
    let etv_toml_path = repo_root.join("rust/etv-core/Cargo.toml");
    let cargo_lock_text = std::fs::read_to_string(&lock_path)
        .map_err(|e| format!("failed to read {}: {e}", lock_path.display()))?;
    let etv_toml_text = std::fs::read_to_string(&etv_toml_path)
        .map_err(|e| format!("failed to read {}: {e}", etv_toml_path.display()))?;

    let lock_entries = parse_cargo_lock_entries(&cargo_lock_text);
    let mut lock_map: BTreeMap<(String, String), CargoLockPackage> = BTreeMap::new();
    let mut etv_version = String::from("2.0.0-alpha.1");
    for entry in lock_entries {
        if entry.name == "etv-core" && entry.source.is_none() {
            etv_version = entry.version.clone();
        }
        lock_map.insert((entry.name.clone(), entry.version.clone()), entry);
    }

    let compiled_pairs = compute_compiled_crates_from_cargo_lock(&cargo_lock_text, &etv_toml_text);
    let mut index_dirs = find_local_cargo_registry_src_dirs();

    let repo_lic_bytes = std::fs::read(repo_root.join("LICENSE"))
        .map_err(|e| format!("failed to read repo LICENSE: {e}"))?;
    let (repo_license_text, repo_license_sha) = normalize_lf_and_sha256(&repo_lic_bytes);

    let mut bundled_data_files = Vec::new();
    for (rel_path, desc) in [
        (
            "rust/etv-core/content/content_manifest.json",
            "Embedded event and callout content manifest (include_str! in etv-core)",
        ),
        (
            "rust/etv-core/content/event_skeletons.json",
            "Embedded 200 JSON event skeletons (include_str! in etv-core)",
        ),
    ] {
        let raw = std::fs::read(repo_root.join(rel_path))
            .map_err(|e| format!("failed to read {rel_path}: {e}"))?;
        let (_, sha) = normalize_lf_and_sha256(&raw);
        bundled_data_files.push(BundledDataFileNotice {
            path: rel_path.to_string(),
            description: desc.to_string(),
            license: "MIT".to_string(),
            sha256: sha,
            license_filename: "LICENSE".to_string(),
            license_sha256: repo_license_sha.clone(),
            license_text: repo_license_text.clone(),
        });
    }

    let embedded_fallback = embedded_notices_manifest();
    let mut crates = Vec::with_capacity(compiled_pairs.len());
    for (name, version) in compiled_pairs {
        let lock_pkg = lock_map
            .get(&(name.clone(), version.clone()))
            .ok_or_else(|| format!("missing Cargo.lock entry for {name} {version}"))?;
        let mut crate_dir_opt = index_dirs
            .iter()
            .map(|d| d.join(format!("{name}-{version}")))
            .find(|p| p.is_dir());
        if crate_dir_opt.is_none() {
            let _ = std::process::Command::new("cargo")
                .args([
                    "metadata",
                    "--manifest-path",
                    &repo_root.join("rust/Cargo.toml").to_string_lossy(),
                    "--format-version",
                    "1",
                    "--all-features",
                    "--locked",
                ])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
            index_dirs = find_local_cargo_registry_src_dirs();
            crate_dir_opt = index_dirs
                .iter()
                .map(|d| d.join(format!("{name}-{version}")))
                .find(|p| p.is_dir());
        }
        let Some(crate_dir) = crate_dir_opt else {
            let expected_cksum = lock_pkg.checksum.as_deref().unwrap_or_default();
            if let Some(existing) = embedded_fallback.crates.iter().find(|c| {
                c.name == name
                    && c.version == version
                    && !expected_cksum.is_empty()
                    && c.cargo_lock_checksum == expected_cksum
            }) {
                crates.push(existing.clone());
                continue;
            }
            return Err(format!(
                "crate directory {name}-{version} not found in local Cargo registry source"
            ));
        };

        let crate_toml_raw = std::fs::read_to_string(crate_dir.join("Cargo.toml"))
            .map_err(|e| format!("failed to read Cargo.toml for {name}-{version}: {e}"))?;
        let (lic_expr, repo_url) = parse_package_license_and_repo(&crate_toml_raw);

        let mut lic_entries: Vec<(String, PathBuf)> = std::fs::read_dir(&crate_dir)
            .map_err(|e| format!("failed to read dir {}: {e}", crate_dir.display()))?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_file())
            .filter_map(|e| {
                let fname = e.file_name().to_string_lossy().to_string();
                if is_license_filename(&fname) {
                    Some((fname, e.path()))
                } else {
                    None
                }
            })
            .collect();
        lic_entries.sort_by(|a, b| (a.0.to_lowercase(), &a.0).cmp(&(b.0.to_lowercase(), &b.0)));

        let mut license_files = Vec::new();
        for (fname, fpath) in lic_entries {
            let raw = std::fs::read(&fpath)
                .map_err(|e| format!("failed to read {}: {e}", fpath.display()))?;
            let (norm_text, norm_sha) = normalize_lf_and_sha256(&raw);
            license_files.push(CrateLicenseFile {
                filename: fname,
                sha256: norm_sha,
                text: norm_text,
            });
        }

        crates.push(CrateNotice {
            name: name.clone(),
            version: version.clone(),
            license: lic_expr,
            repository: if repo_url.is_empty() {
                format!("https://crates.io/crates/{name}/{version}")
            } else {
                repo_url
            },
            cargo_lock_checksum: lock_pkg.checksum.clone().unwrap_or_default(),
            targets: crate_targets_in_build(&name),
            license_files,
        });
    }

    let (etv_lic, _) = parse_package_license_and_repo(&etv_toml_text);
    let manifest = NoticesManifest {
        schema_version: 1,
        package: FirstPartyPackageNotice {
            name: "etv-core".to_string(),
            version: etv_version,
            license: if etv_lic.is_empty() {
                "MIT".to_string()
            } else {
                etv_lic
            },
            source: "https://github.com/mcp-tool-shop-org/escape-the-valley/blob/rust-2.0/LICENSE"
                .to_string(),
            license_filename: "LICENSE".to_string(),
            license_sha256: repo_license_sha,
            license_text: repo_license_text,
        },
        targets: RELEASE_TARGETS.iter().map(|s| (*s).to_string()).collect(),
        bundled_data_files,
        crates,
    };

    let raw_json = serde_json::to_string_pretty(&manifest)
        .map_err(|e| format!("failed to serialize NoticesManifest: {e}"))?;
    let mut manifest_json = String::with_capacity(raw_json.len() + 64);
    for ch in raw_json.chars() {
        if ch.is_ascii() {
            manifest_json.push(ch);
        } else {
            let code = ch as u32;
            if code <= 0xFFFF {
                manifest_json.push_str(&format!("\\u{code:04x}"));
            } else {
                let adj = code - 0x1_0000;
                let hi = 0xD800 + ((adj >> 10) & 0x3FF);
                let lo = 0xDC00 + (adj & 0x3FF);
                manifest_json.push_str(&format!("\\u{hi:04x}\\u{lo:04x}"));
            }
        }
    }
    manifest_json.push('\n');
    let notices_txt = render_third_party_notices(&manifest);
    Ok((manifest, manifest_json, notices_txt))
}
