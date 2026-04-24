/// Google Drive relay — desktop side (T-0c-002).
///
/// This module is compiled only for non-Android targets (Cargo.toml scopes
/// `reqwest` to `cfg(not(target_os = "android"))`).
///
/// File naming in Drive AppData (flat, TA-approved):
///   fw-desktop-<device_id>-pending-<event_id>.json  — desktop emits, Android reads
///   fw-desktop-<device_id>-acked-<event_id>.json    — Android writes ACK, desktop reads
///   fw-android-<device_id>-pending-<event_id>.json  — Android emits, desktop reads
///   fw-android-<device_id>-acked-<event_id>.json    — desktop writes ACK, Android reads
///
/// drive_config.json is encrypted (AES-256-GCM fw1a) with the local db_key.
/// The relay is best-effort: if Drive is unreachable the loop sleeps 30s and retries.
///
/// R12: this module transports raw_events only. Episode Detector, Pattern
/// Detector and Session Builder are never invoked here.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

use crate::crypto;
use crate::raw_event::RawEvent;
use crate::storage::Db;

const DRIVE_FILES: &str = "https://www.googleapis.com/drive/v3/files";
const DRIVE_UPLOAD: &str = "https://www.googleapis.com/upload/drive/v3/files";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";

// ── Config ───────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DriveConfig {
    pub client_id: String,
    pub client_secret: String,
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: i64,              // Unix seconds
    pub device_id: String,            // "desktop-{uuid}"
    pub paired_android_id: String,    // "android-{uuid}" of the paired phone
    pub shared_key_hex: String,       // 32-byte relay key for transport encryption
}

impl DriveConfig {
    pub fn is_configured(&self) -> bool {
        !self.client_id.is_empty()
            && !self.refresh_token.is_empty()
            && !self.shared_key_hex.is_empty()
            && !self.paired_android_id.is_empty()
    }

    pub fn load_encrypted(path: &Path, local_key: &str) -> Option<Self> {
        let hex = std::fs::read_to_string(path).ok()?;
        let json = crypto::decrypt_aes(hex.trim(), local_key)?;
        serde_json::from_str(&json).ok()
    }

    pub fn save_encrypted(&self, path: &Path, local_key: &str) {
        if let Ok(json) = serde_json::to_string(self) {
            let enc = crypto::encrypt_aes(&json, local_key);
            let _ = std::fs::write(path, enc);
        }
    }
}

// ── Time helpers ─────────────────────────────────────────────────────────────

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

// ── File name helpers (flat naming, TA-approved) ─────────────────────────────

fn desktop_pending(device_id: &str, event_id: &str) -> String {
    format!("fw-{device_id}-pending-{event_id}.json")
}

fn desktop_acked(device_id: &str, event_id: &str) -> String {
    format!("fw-{device_id}-acked-{event_id}.json")
}

fn android_pending_prefix(android_id: &str) -> String {
    format!("fw-{android_id}-pending-")
}

fn android_acked(android_id: &str, event_id: &str) -> String {
    format!("fw-{android_id}-acked-{event_id}.json")
}

// ── OAuth token refresh ──────────────────────────────────────────────────────

async fn ensure_token(
    config: &mut DriveConfig,
    config_path: &Path,
    local_key: &str,
) -> Result<String, String> {
    if !config.access_token.is_empty() && now_secs() < config.expires_at - 60 {
        return Ok(config.access_token.clone());
    }
    let client = reqwest::Client::new();
    let resp = client
        .post(TOKEN_URL)
        .form(&[
            ("grant_type", "refresh_token"),
            ("client_id", &config.client_id),
            ("client_secret", &config.client_secret),
            ("refresh_token", &config.refresh_token),
        ])
        .send()
        .await
        .map_err(|e| format!("token refresh failed: {e}"))?;
    let j: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("token json: {e}"))?;
    let token = j["access_token"]
        .as_str()
        .ok_or("no access_token in response")?
        .to_string();
    let expires_in = j["expires_in"].as_i64().unwrap_or(3600);
    config.access_token = token.clone();
    config.expires_at = now_secs() + expires_in;
    config.save_encrypted(config_path, local_key);
    Ok(token)
}

// ── Drive REST helpers ───────────────────────────────────────────────────────

async fn drive_upload(token: &str, name: &str, body: &str) -> Result<(), String> {
    let client = reqwest::Client::new();
    let metadata = serde_json::json!({"name": name, "parents": ["appDataFolder"]}).to_string();
    let boundary = format!("fw_{}", now_ms());
    let multipart = format!(
        "--{boundary}\r\nContent-Type: application/json; charset=UTF-8\r\n\r\n\
         {metadata}\r\n--{boundary}\r\nContent-Type: application/json\r\n\r\n\
         {body}\r\n--{boundary}--"
    );
    let resp = client
        .post(format!("{DRIVE_UPLOAD}?uploadType=multipart&fields=id"))
        .bearer_auth(token)
        .header(
            "Content-Type",
            format!("multipart/related; boundary={boundary}"),
        )
        .body(multipart)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if resp.status().is_success() {
        Ok(())
    } else {
        Err(format!("upload failed: {}", resp.status()))
    }
}

async fn drive_list_prefix(token: &str, prefix: &str) -> Result<Vec<(String, String)>, String> {
    let client = reqwest::Client::new();
    let q = format!("name contains '{prefix}' and trashed = false");
    let resp = client
        .get(DRIVE_FILES)
        .bearer_auth(token)
        .query(&[("spaces", "appDataFolder"), ("fields", "files(id,name)"), ("q", &q)])
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let j: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let files = j["files"].as_array().cloned().unwrap_or_default();
    Ok(files
        .iter()
        .map(|f| {
            (
                f["id"].as_str().unwrap_or("").to_string(),
                f["name"].as_str().unwrap_or("").to_string(),
            )
        })
        .collect())
}

async fn drive_download(token: &str, file_id: &str) -> Result<String, String> {
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{DRIVE_FILES}/{file_id}?alt=media"))
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    resp.text().await.map_err(|e| e.to_string())
}

// ── Build a RawEvent from a resource in the DB ───────────────────────────────

fn build_raw_event(
    resource_uuid: &str,
    event_id: &str,
    db: &Db,
    local_key: &str,
    shared_key: &str,
    device_id: &str,
) -> Option<RawEvent> {
    let r = db.get_by_uuid(resource_uuid).ok()??;
    let url_plain = crypto::decrypt_any(&r.url, local_key)?;
    let title_plain = crypto::decrypt_any(&r.title, local_key).unwrap_or_default();
    Some(RawEvent {
        event_id: event_id.to_string(),
        device_id: device_id.to_string(),
        source: "desktop".to_string(),
        captured_at: r.captured_at,
        domain: r.domain,
        category: r.category,
        url_encrypted: crypto::encrypt_aes(&url_plain, shared_key),
        title_encrypted: crypto::encrypt_aes(&title_plain, shared_key),
        schema_version: 1,
    })
}

// ── Process an incoming Android event ────────────────────────────────────────

fn process_android_event(
    event: &RawEvent,
    db: &Db,
    local_key: &str,
    shared_key: &str,
) -> Result<(), String> {
    let url_plain = crypto::decrypt_aes(&event.url_encrypted, shared_key)
        .ok_or("decrypt url failed")?;
    let title_plain =
        crypto::decrypt_aes(&event.title_encrypted, shared_key).unwrap_or_default();
    let resource = crate::storage::NewResource {
        uuid: Uuid::new_v5(&Uuid::NAMESPACE_URL, format!("{}{}",event.domain, url_plain).as_bytes())
            .to_string(),
        url: crypto::encrypt_aes(&url_plain, local_key),
        title: crypto::encrypt_aes(&title_plain, local_key),
        domain: event.domain.clone(),
        category: event.category.clone(),
        captured_at: event.captured_at,
    };
    db.insert_or_ignore(&resource).map_err(|e| e.to_string())?;
    Ok(())
}

// ── Main relay cycle ─────────────────────────────────────────────────────────

pub async fn run_relay_cycle(
    config_path: &Path,
    db: &Mutex<Db>,
    local_key: &str,
) -> Result<(), String> {
    let mut config =
        DriveConfig::load_encrypted(config_path, local_key).ok_or("Drive not configured")?;
    if !config.is_configured() {
        return Ok(());
    }
    let token = ensure_token(&mut config, config_path, local_key).await?;

    // 1. Upload pending desktop events
    let pending = {
        let db = db.lock().map_err(|e| e.to_string())?;
        db.pending_relay_events().map_err(|e| e.to_string())?
    };
    for (event_id, resource_uuid) in &pending {
        let event = {
            let db = db.lock().map_err(|e| e.to_string())?;
            build_raw_event(
                resource_uuid,
                event_id,
                &db,
                local_key,
                &config.shared_key_hex,
                &config.device_id,
            )
        };
        if let Some(ev) = event {
            let name = desktop_pending(&config.device_id, event_id);
            let body = serde_json::to_string(&ev).map_err(|e| e.to_string())?;
            match drive_upload(&token, &name, &body).await {
                Ok(()) => {
                    let db = db.lock().map_err(|e| e.to_string())?;
                    let _ = db.mark_relay_uploaded(event_id, now_ms());
                }
                Err(e) => {
                    let db = db.lock().map_err(|e| e.to_string())?;
                    let _ = db.increment_relay_retry(event_id);
                    eprintln!("[relay] upload {event_id} failed: {e}");
                }
            }
        }
    }

    // 2. Read Android ACKs for desktop events
    let ack_prefix = desktop_acked(&config.device_id, "");
    if let Ok(ack_files) = drive_list_prefix(&token, &ack_prefix).await {
        for (_, name) in ack_files {
            // Extract event_id: fw-<device_id>-acked-<event_id>.json
            let suffix = ".json";
            if let Some(event_id) = name
                .strip_prefix(&format!("fw-{}-acked-", config.device_id))
                .and_then(|s| s.strip_suffix(suffix))
            {
                let db = db.lock().map_err(|e| e.to_string())?;
                let _ = db.mark_relay_acked(event_id, now_ms());
            }
        }
    }

    // 3. Download Android pending events
    let android_prefix = android_pending_prefix(&config.paired_android_id);
    if let Ok(files) = drive_list_prefix(&token, &android_prefix).await {
        for (file_id, _) in files {
            if let Ok(content) = drive_download(&token, &file_id).await {
                if let Ok(event) = serde_json::from_str::<RawEvent>(&content) {
                    // Idempotence: if we already processed this event_id, just write ACK
                    let already = {
                        let db = db.lock().map_err(|e| e.to_string())?;
                        db.get_by_uuid(&Uuid::new_v5(
                            &Uuid::NAMESPACE_URL,
                            format!("{}{}", event.domain, event.event_id).as_bytes(),
                        ).to_string()).ok().flatten().is_some()
                    };
                    if !already {
                        let db = db.lock().map_err(|e| e.to_string())?;
                        let _ = process_android_event(&event, &db, local_key, &config.shared_key_hex);
                    }
                    // Write ACK regardless (idempotent)
                    let ack_name = android_acked(&config.paired_android_id, &event.event_id);
                    let ack_body = serde_json::json!({"acked_at": now_ms()}).to_string();
                    let _ = drive_upload(&token, &ack_name, &ack_body).await;
                }
            }
        }
    }

    Ok(())
}

// ── Deterministic device ID from installation path ───────────────────────────

pub fn desktop_device_id(app_data_dir: &Path) -> String {
    format!(
        "desktop-{}",
        Uuid::new_v5(&Uuid::NAMESPACE_URL, app_data_dir.to_string_lossy().as_bytes())
    )
}

/// Config file path.
pub fn config_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("drive_config.json")
}
