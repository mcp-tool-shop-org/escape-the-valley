//! Third-party license notices (`etv --notices`), allow-list validation, and deterministic
//! LF-only notice rendering for all crates and bundled data files compiled into the binary.
//!
//! All collections use [`Vec`] and [`BTreeMap`] only (no `HashMap` or `HashSet`).

use std::collections::BTreeMap;

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

/// Repository SPDX license allow-list for crates and bundled data files.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LicenseAllowlist {
    pub schema_version: u32,
    pub description: String,
    pub allowed_spdx_identifiers: Vec<String>,
    pub allowed_license_expressions: Vec<String>,
    pub prohibited_copyleft_prefixes: Vec<String>,
}

/// Validation failure returned by [`validate_notices_against_allowlist`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoticesValidationError {
    pub subject: String,
    pub code: String,
    pub message: String,
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

/// Parse `[[package]]` blocks from `Cargo.lock` into a deterministic `BTreeMap` mapping
/// `(name, version)` to its optional `checksum`.
pub fn parse_cargo_lock_packages(
    cargo_lock_text: &str,
) -> BTreeMap<(String, String), Option<String>> {
    let mut out = BTreeMap::new();
    for block in cargo_lock_text.split("[[package]]").skip(1) {
        let mut name: Option<String> = None;
        let mut version: Option<String> = None;
        let mut checksum: Option<String> = None;
        for line in block.lines() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("name = ") {
                name = Some(rest.trim_matches('"').to_string());
            } else if let Some(rest) = trimmed.strip_prefix("version = ") {
                version = Some(rest.trim_matches('"').to_string());
            } else if let Some(rest) = trimmed.strip_prefix("checksum = ") {
                checksum = Some(rest.trim_matches('"').to_string());
            }
        }
        if let (Some(n), Some(v)) = (name, version) {
            out.insert((n, v), checksum);
        }
    }
    out
}
