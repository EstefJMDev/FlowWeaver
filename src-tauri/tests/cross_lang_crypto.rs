//! Phase 2.2 — cross-language crypto parity tests.
//!
//! Loads the single golden vector at tests/fixtures/cross_lang_vectors.json and
//! verifies that crypto.rs produces / consumes byte-identical ciphertext to the
//! Kotlin side (see app/src/test/java/com/flowweaver/app/RelayCryptoTest.kt,
//! which loads the same fixture).
//!
//! INC-002 lesson: never let each side validate against itself. The fixture is
//! the only source of truth; both languages reference it.

#![cfg(not(target_os = "android"))]

use std::fs;
use std::path::PathBuf;

use serde::Deserialize;

use flowweaver_lib::crypto;

#[derive(Debug, Deserialize)]
struct CrossLangVector {
    vector_id: String,
    key_hex: String,
    plain_utf8: String,
    nonce_hex: String,
    expected_ciphertext_hex: String,
}

fn load_vector() -> CrossLangVector {
    let path: PathBuf = [
        env!("CARGO_MANIFEST_DIR"),
        "tests",
        "fixtures",
        "cross_lang_vectors.json",
    ]
    .iter()
    .collect();
    let raw = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read fixture {path:?}: {e}"));
    serde_json::from_str(&raw).expect("parse cross_lang_vectors.json")
}

fn nonce_bytes(v: &CrossLangVector) -> [u8; 12] {
    let bytes = hex::decode(&v.nonce_hex).expect("nonce_hex must be valid hex");
    assert_eq!(bytes.len(), 12, "AES-GCM nonce must be 12 bytes");
    let mut out = [0u8; 12];
    out.copy_from_slice(&bytes);
    out
}

// ── Test A.1 — deterministic encrypt matches fixture ────────────────────────

#[test]
fn rust_encrypt_with_fixture_nonce_matches_expected_hex() {
    let v = load_vector();
    let nonce = nonce_bytes(&v);
    let actual = crypto::encrypt_aes_for_test_with_explicit_nonce(&v.plain_utf8, &v.key_hex, &nonce);
    assert_eq!(
        actual, v.expected_ciphertext_hex,
        "vector_id={}: Rust encrypt produced different bytes than fixture. \
         If you intentionally changed crypto.rs, regenerate the fixture and \
         the Kotlin RelayCryptoTest must validate the new value too.",
        v.vector_id
    );
}

// ── Test A.2 — decrypt of fixture ciphertext recovers plaintext ─────────────

#[test]
fn rust_decrypt_of_fixture_ciphertext_recovers_plaintext() {
    let v = load_vector();
    let plain = crypto::decrypt_aes(&v.expected_ciphertext_hex, &v.key_hex)
        .expect("decrypt_aes of golden ciphertext must succeed");
    assert_eq!(plain, v.plain_utf8, "vector_id={}", v.vector_id);
}

// ── Test A.3 — production API uses random nonce (refuerzo 1.2) ──────────────
//
// Guards against future regressions where someone replaces the random nonce
// in `encrypt_aes` with a fixed value. Two consecutive encrypts of the same
// plaintext with the same key MUST yield different ciphertexts.

#[test]
fn production_encrypt_aes_uses_random_nonce() {
    let v = load_vector();
    let a = crypto::encrypt_aes(&v.plain_utf8, &v.key_hex);
    let b = crypto::encrypt_aes(&v.plain_utf8, &v.key_hex);
    assert_ne!(
        a, b,
        "encrypt_aes must use a fresh random nonce on every call. Reusing a \
         nonce with the same key in AES-GCM destroys confidentiality and \
         authentication. If this assertion ever fires, revert the offending \
         change immediately."
    );
    // Sanity: both must still round-trip.
    assert_eq!(crypto::decrypt_aes(&a, &v.key_hex).as_deref(), Some(v.plain_utf8.as_str()));
    assert_eq!(crypto::decrypt_aes(&b, &v.key_hex).as_deref(), Some(v.plain_utf8.as_str()));
}
