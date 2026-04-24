use serde::{Deserialize, Serialize};

/// A capture event transported via the Google Drive relay (T-0c-002).
///
/// url_encrypted and title_encrypted use AES-256-GCM (fw1a) with the shared
/// pairing key — NOT the local SQLite key. The receiver decrypts with the
/// pairing key and re-encrypts with its local key before storing in SQLite.
///
/// domain and category travel in clear (D1 — permitted abstraction level).
/// R12: this struct carries raw capture data only — no episode, session or
/// pattern information is included or derivable from it.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RawEvent {
    pub event_id: String,
    pub device_id: String,
    pub source: String,           // "desktop" | "android"
    pub captured_at: i64,         // Unix ms
    pub domain: String,
    pub category: String,
    pub url_encrypted: String,    // hex(fw1a nonce | AES-GCM ct) with shared key
    pub title_encrypted: String,
    pub schema_version: u32,
}
