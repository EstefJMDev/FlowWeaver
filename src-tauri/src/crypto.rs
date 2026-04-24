/// Field-level encryption for url and title (D1 — Privacy Level 1).
///
/// Two algorithms are supported:
///
/// XOR (legacy, magic "fw0a"): used in Fase 0a/0b desktop and T-0c-001 Android baseline.
///   Simple XOR with a key-derived stream. Preserved for backward-compatible decryption.
///
/// AES-256-GCM (current, magic "fw1a"): used from T-0c-002 onwards on Android and
///   for all new records. Key derived from passphrase via SHA-256. Nonce is 12 random
///   bytes prepended to the ciphertext. Authenticated encryption — detects tampering.
///
/// Both algorithms encode output as lowercase hex. The 4-byte magic prefix identifies
/// which algorithm was used so `decrypt_any` can route correctly.

const MAGIC_XOR: &[u8] = b"fw0a";
const MAGIC_AES: &[u8] = b"fw1a";

// ── XOR (legacy) ──────────────────────────────────────────────────────────────

pub fn encrypt(plaintext: &str, key: &str) -> String {
    let key_bytes = derive_key_xor(key);
    let cipher: Vec<u8> = plaintext
        .bytes()
        .enumerate()
        .map(|(i, b)| b ^ key_bytes[i % key_bytes.len()])
        .collect();
    let mut out = MAGIC_XOR.to_vec();
    out.extend_from_slice(&cipher);
    hex::encode(out)
}

pub fn decrypt(ciphertext: &str, key: &str) -> Option<String> {
    let bytes = hex::decode(ciphertext).ok()?;
    if bytes.len() < MAGIC_XOR.len() || &bytes[..MAGIC_XOR.len()] != MAGIC_XOR {
        return None;
    }
    let cipher = &bytes[MAGIC_XOR.len()..];
    let key_bytes = derive_key_xor(key);
    let plain: Vec<u8> = cipher
        .iter()
        .enumerate()
        .map(|(i, b)| b ^ key_bytes[i % key_bytes.len()])
        .collect();
    String::from_utf8(plain).ok()
}

fn derive_key_xor(passphrase: &str) -> Vec<u8> {
    let mut key = passphrase.as_bytes().to_vec();
    while key.len() < 32 {
        let extra: Vec<u8> = key.iter().map(|b| b.wrapping_add(0x5c)).collect();
        key.extend_from_slice(&extra);
    }
    key.truncate(32);
    key
}

// ── AES-256-GCM (T-0c-002+) ──────────────────────────────────────────────────

/// Encrypt with AES-256-GCM. Output: hex(MAGIC_AES | 12-byte-nonce | ciphertext+tag).
pub fn encrypt_aes(plaintext: &str, key: &str) -> String {
    use aes_gcm::{
        aead::{Aead, AeadCore, KeyInit, OsRng},
        Aes256Gcm,
    };
    let aes_key = derive_key_aes(key);
    let cipher = Aes256Gcm::new((&aes_key).into());
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let ct = cipher
        .encrypt(&nonce, plaintext.as_bytes())
        .expect("AES-256-GCM encrypt failed");
    let mut out = MAGIC_AES.to_vec();
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ct);
    hex::encode(out)
}

/// Decrypt AES-256-GCM ciphertext produced by `encrypt_aes`.
pub fn decrypt_aes(ciphertext: &str, key: &str) -> Option<String> {
    use aes_gcm::{aead::{Aead, KeyInit}, Aes256Gcm, Nonce};
    let bytes = hex::decode(ciphertext).ok()?;
    // 4 magic + 12 nonce + 16 GCM tag minimum
    if bytes.len() < MAGIC_AES.len() + 12 + 16 {
        return None;
    }
    if &bytes[..MAGIC_AES.len()] != MAGIC_AES {
        return None;
    }
    let nonce = Nonce::from_slice(&bytes[MAGIC_AES.len()..MAGIC_AES.len() + 12]);
    let ct = &bytes[MAGIC_AES.len() + 12..];
    let aes_key = derive_key_aes(key);
    let cipher = Aes256Gcm::new((&aes_key).into());
    let plain = cipher.decrypt(nonce, ct).ok()?;
    String::from_utf8(plain).ok()
}

/// SHA-256 of the passphrase → 32-byte AES key.
fn derive_key_aes(passphrase: &str) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(passphrase.as_bytes());
    h.finalize().into()
}

// ── Unified API ───────────────────────────────────────────────────────────────

/// Detect whether a hex ciphertext was produced by AES-256-GCM (fw1a prefix).
pub fn is_aes_encrypted(ciphertext: &str) -> bool {
    hex::decode(ciphertext)
        .ok()
        .map(|b| b.starts_with(MAGIC_AES))
        .unwrap_or(false)
}

/// Decrypt regardless of algorithm: tries AES-256-GCM first (fw1a), falls back to XOR (fw0a).
pub fn decrypt_any(ciphertext: &str, key: &str) -> Option<String> {
    if is_aes_encrypted(ciphertext) {
        decrypt_aes(ciphertext, key)
    } else {
        decrypt(ciphertext, key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "fw-test-key";

    #[test]
    fn xor_roundtrip() {
        let ct = encrypt("https://github.com/rust-lang/rust", KEY);
        assert!(ct.starts_with("66773061")); // hex of MAGIC_XOR "fw0a"
        let pt = decrypt(&ct, KEY).unwrap();
        assert_eq!(pt, "https://github.com/rust-lang/rust");
    }

    #[test]
    fn aes_roundtrip() {
        let ct = encrypt_aes("https://github.com/rust-lang/rust", KEY);
        assert!(ct.starts_with("66773161")); // hex of MAGIC_AES "fw1a"
        let pt = decrypt_aes(&ct, KEY).unwrap();
        assert_eq!(pt, "https://github.com/rust-lang/rust");
    }

    #[test]
    fn aes_different_nonce_each_call() {
        let ct1 = encrypt_aes("same text", KEY);
        let ct2 = encrypt_aes("same text", KEY);
        // Different nonce → different ciphertext
        assert_ne!(ct1, ct2);
    }

    #[test]
    fn decrypt_any_routes_correctly() {
        let xor_ct = encrypt("hello", KEY);
        let aes_ct = encrypt_aes("hello", KEY);
        assert_eq!(decrypt_any(&xor_ct, KEY).unwrap(), "hello");
        assert_eq!(decrypt_any(&aes_ct, KEY).unwrap(), "hello");
    }

    #[test]
    fn is_aes_encrypted_detection() {
        assert!(!is_aes_encrypted(&encrypt("test", KEY)));
        assert!(is_aes_encrypted(&encrypt_aes("test", KEY)));
    }
}
