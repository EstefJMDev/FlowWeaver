use serde::{Deserialize, Serialize};
use tauri::{Manager, State};
use uuid::Uuid;

use crate::{crypto, grouper, importer, storage::{Db, NewResource, Resource}};

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
        url: crypto::encrypt(&input.url, &key),
        title: crypto::encrypt(&input.title, &key),
        domain: input.domain,
        category: String::new(),
    };
    let uuid = new.uuid.clone();
    db.insert_resource(&new).map_err(|e| e.to_string())?;
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
    Ok(importer::import(path.as_deref(), &db, &key))
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
    Ok(importer::import_html_content(&content, &db, &key))
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
            title: crypto::decrypt(&r.title, &key).unwrap_or_default(),
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

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Derive the database key from the app's data directory path.
/// This binds the key to the installation — never transmitted (invariant 2).
fn db_key(app: &tauri::AppHandle) -> String {
    let path = app
        .path()
        .app_data_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| "flowweaver-fallback-key".to_string());
    format!("fw-{path}")
}
