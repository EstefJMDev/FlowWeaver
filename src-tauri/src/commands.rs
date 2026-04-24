use serde::{Deserialize, Serialize};
use tauri::{Manager, State};
use uuid::Uuid;

use crate::{
    classifier,
    crypto,
    episode_detector,
    grouper,
    importer,
    session_builder,
    storage::{Db, NewResource, PrivacyStats, Resource},
};

pub struct DbState(pub std::sync::Mutex<Db>);

// ── Types exposed to the frontend ────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct ImportResourceInput {
    pub url: String,
    pub title: String,
    pub domain: String,
}

#[derive(Debug, Serialize)]
pub struct ResourceView {
    pub id: i64,
    pub uuid: String,
    pub title: String,
    pub domain: String,
    pub category: String,
    // url is intentionally omitted from the view layer (D1)
}

#[derive(Debug, Serialize)]
pub struct MobileResource {
    pub uuid: String,
    pub domain: String,
    pub category: String,
    pub title: String,
    pub captured_at: i64,
}

#[derive(Debug, Serialize)]
pub struct CategoryGroup {
    pub category: String,
    pub resources: Vec<MobileResource>,
}

// ── Tauri commands ────────────────────────────────────────────────────────────

/// Import a single resource into SQLCipher.
/// url and title are encrypted before storage (D1).
/// Called by the Bookmark Importer (T-0a-002).
#[tauri::command]
pub fn import_resource(
    input: ImportResourceInput,
    state: State<'_, DbState>,
    app: tauri::AppHandle,
) -> Result<String, String> {
    let key = db_key(&app);
    let db = state.0.lock().map_err(|e| e.to_string())?;
    let new = NewResource {
        uuid: Uuid::new_v4().to_string(),
        url: crypto::encrypt_aes(&input.url, &key),
        title: crypto::encrypt_aes(&input.title, &key),
        domain: input.domain,
        category: String::new(),
        captured_at: 0,
    };
    let uuid = new.uuid.clone();
    db.insert_resource(&new).map_err(|e| e.to_string())?;
    #[cfg(not(target_os = "android"))]
    {
        let event_id = Uuid::new_v4().to_string();
        let _ = db.enqueue_relay_event(&event_id, &uuid, &relay_device_id(&app));
    }
    Ok(uuid)
}

/// Import browser bookmarks as bootstrap data (T-0a-002).
/// path: optional explicit file path; if omitted, auto-detects Chrome/Edge/Brave.
#[tauri::command]
pub fn import_bookmarks(
    path: Option<String>,
    state: State<'_, DbState>,
    app: tauri::AppHandle,
) -> Result<importer::ImportResult, String> {
    let key = db_key(&app);
    let db = state.0.lock().map_err(|e| e.to_string())?;
    let result = importer::import(path.as_deref(), &db, &key);
    #[cfg(not(target_os = "android"))]
    let _ = db.enqueue_unrelayed_resources(&relay_device_id(&app));
    Ok(result)
}

/// Import bookmarks from HTML content sent by the frontend file picker.
/// Used when auto-detect finds no browser files and the user exports to HTML.
#[tauri::command]
pub fn import_bookmarks_html(
    content: String,
    state: State<'_, DbState>,
    app: tauri::AppHandle,
) -> Result<importer::ImportResult, String> {
    let key = db_key(&app);
    let db = state.0.lock().map_err(|e| e.to_string())?;
    let result = importer::import_html_content(&content, &db, &key);
    #[cfg(not(target_os = "android"))]
    let _ = db.enqueue_unrelayed_resources(&relay_device_id(&app));
    Ok(result)
}

/// Update the category of a resource — called by the Classifier (T-0a-003).
#[tauri::command]
pub fn set_resource_category(
    uuid: String,
    category: String,
    state: State<'_, DbState>,
) -> Result<(), String> {
    let db = state.0.lock().map_err(|e| e.to_string())?;
    db.set_category(&uuid, &category).map_err(|e| e.to_string())?;
    Ok(())
}

/// Return all resources with title decrypted — consumed by the Grouper (T-0a-004).
/// url is not exposed to the frontend layer (D1).
#[tauri::command]
pub fn get_resources(
    state: State<'_, DbState>,
    app: tauri::AppHandle,
) -> Result<Vec<ResourceView>, String> {
    let key = db_key(&app);
    let db = state.0.lock().map_err(|e| e.to_string())?;
    let rows: Vec<Resource> = db.all_resources().map_err(|e| e.to_string())?;
    let views = rows
        .into_iter()
        .map(|r| ResourceView {
            id: r.id,
            uuid: r.uuid,
            title: crypto::decrypt_any(&r.title, &key).unwrap_or_default(),
            domain: r.domain,
            category: r.category,
        })
        .collect();
    Ok(views)
}

/// Return grouped clusters for Panel A and Panel C (T-0a-004).
/// Level 1: domain+category. Level 2: shared title tokens.
/// Stateless — each call re-processes all resources; no DB writes.
#[tauri::command]
pub fn get_clusters(
    state: State<'_, DbState>,
    app: tauri::AppHandle,
) -> Result<Vec<grouper::Cluster>, String> {
    let key = db_key(&app);
    let db = state.0.lock().map_err(|e| e.to_string())?;
    grouper::group(&db, &key)
}

/// Return the number of stored resources.
#[tauri::command]
pub fn resource_count(state: State<'_, DbState>) -> Result<i64, String> {
    let db = state.0.lock().map_err(|e| e.to_string())?;
    db.count().map_err(|e| e.to_string())
}

// ── Phase 0b commands ─────────────────────────────────────────────────────────

/// Return sessions produced by the Session Builder (Phase 0b).
/// Sessions group resources by temporal proximity (< 24 h window, 3 h gap).
#[tauri::command]
pub fn get_sessions(
    state: State<'_, DbState>,
    app: tauri::AppHandle,
) -> Result<Vec<session_builder::Session>, String> {
    let key = db_key(&app);
    let db = state.0.lock().map_err(|e| e.to_string())?;
    session_builder::build_sessions(&db, &key)
}

/// Return episodes detected by the Episode Detector (Phase 0b).
/// Runs Session Builder then Episode Detector on each session. Stateless.
#[tauri::command]
pub fn get_episodes(
    state: State<'_, DbState>,
    app: tauri::AppHandle,
) -> Result<Vec<episode_detector::Episode>, String> {
    let key = db_key(&app);
    let db = state.0.lock().map_err(|e| e.to_string())?;
    let sessions = session_builder::build_sessions(&db, &key)?;
    let episodes: Vec<episode_detector::Episode> = sessions
        .iter()
        .flat_map(|s| episode_detector::detect(s))
        .collect();
    Ok(episodes)
}

/// Simulate a Share Extension capture: insert a URL with the current timestamp.
/// Used for desktop testing of the Session Builder and Episode Detector.
/// In production 0b the iOS Share Extension feeds this path directly.
#[tauri::command]
pub fn add_capture(
    url: String,
    title: String,
    state: State<'_, DbState>,
    app: tauri::AppHandle,
) -> Result<String, String> {
    let key = db_key(&app);
    let db = state.0.lock().map_err(|e| e.to_string())?;

    let classified = classifier::classify(&url);
    let uuid = Uuid::new_v5(&Uuid::NAMESPACE_URL, url.as_bytes()).to_string();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    let display_title = if title.is_empty() { classified.domain.clone() } else { title.clone() };
    let new = NewResource {
        uuid: uuid.clone(),
        url: crypto::encrypt_aes(&url, &key),
        title: crypto::encrypt_aes(&display_title, &key),
        domain: classified.domain,
        category: classified.category,
        captured_at: now,
    };

    db.insert_or_ignore(&new).map_err(|e| e.to_string())?;
    #[cfg(not(target_os = "android"))]
    {
        let event_id = Uuid::new_v4().to_string();
        let _ = db.enqueue_relay_event(&event_id, &uuid, &relay_device_id(&app));
    }
    Ok(uuid)
}

// ── Phase 0b — Privacy Dashboard (D14) ───────────────────────────────────────

/// Return aggregate stats for the Privacy Dashboard.
/// Only category and domain columns are exposed — url/title remain encrypted (D1).
#[tauri::command]
pub fn get_privacy_stats(state: State<'_, DbState>) -> Result<PrivacyStats, String> {
    let db = state.0.lock().map_err(|e| e.to_string())?;
    db.privacy_stats().map_err(|e| e.to_string())
}

/// Delete all resources. Called from the Privacy Dashboard clear action.
/// Irreversible — the frontend must confirm before invoking this.
#[tauri::command]
pub fn clear_all_resources(state: State<'_, DbState>) -> Result<usize, String> {
    let db = state.0.lock().map_err(|e| e.to_string())?;
    db.delete_all().map_err(|e| e.to_string())
}

// ── Phase 0c — Platform + URL opener ─────────────────────────────────────────

/// Return "android" or "desktop" — lets the React frontend choose which view to render.
#[tauri::command]
pub fn get_platform() -> &'static str {
    #[cfg(target_os = "android")]
    return "android";
    #[cfg(not(target_os = "android"))]
    return "desktop";
}

/// Decrypt the URL for the given uuid and open it in the system browser.
/// The URL never reaches the frontend (D1 compliance).
#[tauri::command]
pub fn open_resource_url(
    uuid: String,
    state: State<'_, DbState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    use tauri_plugin_shell::ShellExt;
    let key = db_key(&app);
    let db = state.0.lock().map_err(|e| e.to_string())?;
    let resource = db
        .get_by_uuid(&uuid)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("not found: {uuid}"))?;
    let url = crypto::decrypt_any(&resource.url, &key)
        .ok_or_else(|| "decrypt failed".to_string())?;
    app.shell().open(&url, None).map_err(|e| e.to_string())
}

// ── Phase 0c — Mobile commands ────────────────────────────────────────────────

/// Return resources grouped by category for the Android gallery (T-0c-001).
/// title is decrypted; url is omitted (D1). Groups sorted alphabetically;
/// resources within each group sorted by captured_at descending.
#[tauri::command]
pub fn get_mobile_resources(
    state: State<'_, DbState>,
    app: tauri::AppHandle,
) -> Result<Vec<CategoryGroup>, String> {
    let key = db_key(&app);
    let db = state.0.lock().map_err(|e| e.to_string())?;
    let rows = db.all_resources().map_err(|e| e.to_string())?;

    let mut map: std::collections::HashMap<String, Vec<MobileResource>> =
        std::collections::HashMap::new();

    for r in rows {
        let title = crypto::decrypt_any(&r.title, &key).unwrap_or_default();
        map.entry(r.category.clone()).or_default().push(MobileResource {
            uuid: r.uuid,
            domain: r.domain,
            category: r.category,
            title,
            captured_at: r.captured_at,
        });
    }

    let mut groups: Vec<CategoryGroup> = map
        .into_iter()
        .map(|(category, mut resources)| {
            resources.sort_by(|a, b| b.captured_at.cmp(&a.captured_at));
            CategoryGroup { category, resources }
        })
        .collect();
    groups.sort_by(|a, b| a.category.cmp(&b.category));

    Ok(groups)
}

// ── Phase 0c — Drive relay configuration ─────────────────────────────────────

#[derive(Debug, serde::Deserialize)]
pub struct DriveConfigInput {
    pub client_id: String,
    pub client_secret: String,
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: i64,
    pub paired_android_id: String,
    pub shared_key_hex: String,
}

/// Write the Google Drive relay configuration (encrypted).
/// Required before the relay loop can upload or download events.
/// Returns the desktop device_id for display / QR pairing.
#[tauri::command]
pub fn configure_drive(
    input: DriveConfigInput,
    app: tauri::AppHandle,
) -> Result<String, String> {
    #[cfg(not(target_os = "android"))]
    {
        let app_data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
        let local_key = format!("fw-{}", app_data_dir.to_string_lossy());
        let device_id = crate::drive_relay::desktop_device_id(&app_data_dir);
        let config = crate::drive_relay::DriveConfig {
            client_id: input.client_id,
            client_secret: input.client_secret,
            access_token: input.access_token,
            refresh_token: input.refresh_token,
            expires_at: input.expires_at,
            device_id: device_id.clone(),
            paired_android_id: input.paired_android_id,
            shared_key_hex: input.shared_key_hex,
        };
        let config_path = crate::drive_relay::config_path(&app_data_dir);
        config.save_encrypted(&config_path, &local_key);
        return Ok(device_id);
    }
    #[cfg(target_os = "android")]
    Err("configure_drive is not available on Android".to_string())
}

/// Return the stable device_id for this installation (for QR pairing display).
#[tauri::command]
pub fn get_relay_device_id(app: tauri::AppHandle) -> String {
    #[cfg(not(target_os = "android"))]
    {
        app.path()
            .app_data_dir()
            .map(|p| crate::drive_relay::desktop_device_id(&p))
            .unwrap_or_else(|_| "desktop-fallback".to_string())
    }
    #[cfg(target_os = "android")]
    "android-not-configured".to_string()
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Derive the field-level encryption key for url/title in SQLite.
///
/// Desktop: derived from app_data_dir path (installation-bound, never transmitted).
/// Android: stable constant that matches FieldCrypto.FIELD_KEY_PASSPHRASE in Kotlin.
///   A path-based key on Android cannot be guaranteed to align with the Kotlin layer,
///   so a constant is used. The Android app data dir provides file system isolation.
fn db_key(app: &tauri::AppHandle) -> String {
    #[cfg(target_os = "android")]
    {
        let _ = app;
        // Must match FieldCrypto.FIELD_KEY_PASSPHRASE in FieldCrypto.kt.
        return "flowweaver-android-field-key-v1".to_string();
    }
    #[cfg(not(target_os = "android"))]
    {
        app.path()
            .app_data_dir()
            .map(|p| format!("fw-{}", p.to_string_lossy()))
            .unwrap_or_else(|_| "flowweaver-fallback-key".to_string())
    }
}

/// Return the relay device_id for the current installation (desktop only).
#[cfg(not(target_os = "android"))]
fn relay_device_id(app: &tauri::AppHandle) -> String {
    app.path()
        .app_data_dir()
        .map(|p| crate::drive_relay::desktop_device_id(&p))
        .unwrap_or_else(|_| "desktop-fallback".to_string())
}
