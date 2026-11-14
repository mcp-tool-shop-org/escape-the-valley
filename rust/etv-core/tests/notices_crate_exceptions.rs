//! The notices allow-list's per-crate exception: CDLA-Permissive-2.0 is allowed for
//! `webpki-root-certs` only, and the notices carry the agreement's full text.

use etv_core::notices::{
    embedded_license_allowlist, embedded_notices_manifest, third_party_notices_text,
    validate_notices_against_allowlist,
};

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

#[test]
fn the_notices_carry_the_cdla_agreement_text() {
    let text = third_party_notices_text();
    assert!(text.contains("webpki-root-certs"));
    assert!(text.contains("Community Data License Agreement - Permissive - Version 2.0"));
    assert!(text.contains("A Data Recipient may use, modify, and share the Data made"));
}
