//! The notices allow-list's per-crate exception: CDLA-Permissive-2.0 is allowed for
//! `webpki-root-certs` only, and the notices carry the agreement's full text.

use etv_core::notices::{
    embedded_license_allowlist, embedded_notices_manifest, third_party_notices_text,
    validate_notices_against_allowlist,
};
use sha2::{Digest, Sha256};

#[test]
fn cdla_is_allowed_for_webpki_root_certs_only() {
    let allowlist = embedded_license_allowlist();
    assert!(!allowlist
        .allowed_spdx_identifiers
        .iter()
        .any(|id| id == "CDLA-Permissive-2.0"));
    assert_eq!(allowlist.crate_exceptions.len(), 1);
    assert_eq!(allowlist.crate_exceptions[0].name, "webpki-root-certs");
    assert_eq!(allowlist.crate_exceptions[0].license, "CDLA-Permissive-2.0");

    // The shipped manifest passes.
    let manifest = embedded_notices_manifest();
    assert_eq!(
        validate_notices_against_allowlist(&manifest, &allowlist),
        Ok(())
    );

    // The same licence on any other crate still fails.
    let mut other = manifest.clone();
    let i = other
        .crates
        .iter()
        .position(|c| c.name == "native-tls")
        .expect("native-tls is in the notices");
    other.crates[i].license = "CDLA-Permissive-2.0".to_string();
    let errors = validate_notices_against_allowlist(&other, &allowlist).unwrap_err();
    assert!(errors
        .iter()
        .any(|e| e.subject.starts_with("native-tls@") && e.code == "UNALLOWED_SPDX_IDENTIFIER"));
}

/// SHA-256 of the agreement's text as `webpki-root-certs` 1.0.12 ships it (its `LICENSE`).
const CDLA_LICENSE_SHA256: &str =
    "e271993808fec50ab29350b39539cdec611a9103f827e0aa26d61da70e2d33f8";

#[test]
fn the_notices_carry_the_cdla_agreement_text() {
    // The licence asks that the agreement's text travel with the data: the whole of it, from
    // its title to its last definition (5.4, "Results"), not an excerpt.
    let manifest = embedded_notices_manifest();
    let certs = manifest
        .crates
        .iter()
        .find(|c| c.name == "webpki-root-certs")
        .expect("webpki-root-certs is in the notices");
    assert_eq!(certs.license, "CDLA-Permissive-2.0");
    let file = certs
        .license_files
        .iter()
        .find(|f| f.filename == "LICENSE")
        .expect("the agreement is recorded");
    assert_eq!(file.sha256, CDLA_LICENSE_SHA256);
    let digest: [u8; 32] = Sha256::digest(file.text.as_bytes()).into();
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(
        hex, CDLA_LICENSE_SHA256,
        "the recorded text is the shipped text"
    );
    assert!(file
        .text
        .starts_with("# Community Data License Agreement - Permissive - Version 2.0"));
    assert!(file.text.trim_end().ends_with(
        "5.4. \"Results\" means any outcome obtained by computational analysis\nof Data, including for example machine learning models and models'\ninsights."
    ));

    // And the notices file the game prints carries that whole text.
    let text = third_party_notices_text();
    assert!(text.contains("webpki-root-certs"));
    assert!(
        text.contains(file.text.trim_end()),
        "the full agreement is in the notices"
    );
}
