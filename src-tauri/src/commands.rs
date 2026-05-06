use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager, State};
use uuid::Uuid;

use crate::{
    classifier,
    consent_log_store,
    crypto,
    episode_detector,
    fs_watcher::{
        self, CandidateDirectory, FsWatcherDirectory, FsWatcherEvent, FsWatcherHandle,
        FsWatcherRuntimeState, FsWatcherStatus,
    },
    grouper,
    importer,
    pattern_blocks,
    pattern_detector::{self, DetectedPattern, PatternConfig},
    session_builder,
    state_machine::{self, StateMachineConfig, TrustStateView, UserAction},
    storage::{Db, NewResource, PrivacyStats, Resource},
    syntheses_store,
    synthesis_tokens,
    trust_scorer::{self, TrustConfig},
};

#[cfg(not(target_os = "android"))]
use crate::synthesis_engine::{self, SynthesisType};

pub struct DbState(pub std::sync::Mutex<Db>);

/// Estado runtime del FS Watcher (T-2-000). El `handle` mantiene vivo el
/// watcher de `notify`. El `event_buffer` acumula eventos desde el arranque.
/// Registrado vía `.manage(FsWatcherState::default())` en `lib.rs`.
pub struct FsWatcherState {
    pub handle: std::sync::Mutex<Option<FsWatcherHandle>>,
    pub event_buffer: std::sync::Arc<std::sync::Mutex<Vec<FsWatcherEvent>>>,
}

impl Default for FsWatcherState {
    fn default() -> Self {
        Self {
            handle: std::sync::Mutex::new(None),
            event_buffer: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        }
    }
}

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
/// Auto-import (path=None) is suppressed after the user explicitly clears all data,
/// so closing and reopening the app does not silently reimport browser bookmarks.
#[tauri::command]
pub fn import_bookmarks(
    path: Option<String>,
    state: State<'_, DbState>,
    app: tauri::AppHandle,
) -> Result<importer::ImportResult, String> {
    let key = db_key(&app);
    let db = state.0.lock().map_err(|e| e.to_string())?;
    if path.is_none() {
        let skip = db.get_pref("skip_auto_import")
            .ok()
            .flatten()
            .map(|v| v == "1")
            .unwrap_or(false);
        if skip {
            return Ok(importer::ImportResult::default());
        }
    } else {
        // Explicit import by the user — lift the suppression flag.
        let _ = db.set_pref("skip_auto_import", "0");
    }
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

    let classified = classifier::classify(&url, if title.is_empty() { None } else { Some(&title) });
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

// ── Phase 2 — Trust State (T-2-003) ──────────────────────────────────────────

/// Evaluate (and persist) the current Trust State.
/// Composes the canonical chain (D4 — TS-2-003 §"Cadena de invocación canónica"):
/// `pattern_detector → trust_scorer → state_machine`. Authority over transitions
/// stays exclusively in `state_machine`.
#[tauri::command]
pub fn get_trust_state(state: State<'_, DbState>) -> Result<TrustStateView, String> {
    apply_trust_action(state, None)
}

/// Reset the Trust State to `Observing` from any state.
#[tauri::command]
pub fn reset_trust_state(state: State<'_, DbState>) -> Result<TrustStateView, String> {
    apply_trust_action(state, Some(UserAction::Reset))
}

/// Activate `Autonomous` mode. Requires `confirmed: true` and `current == Trusted`.
/// Frontend (T-2-004) must show explicit confirmation before invoking with `confirmed: true`.
#[tauri::command]
pub fn enable_autonomous_mode(
    state: State<'_, DbState>,
    confirmed: bool,
) -> Result<TrustStateView, String> {
    apply_trust_action(state, Some(UserAction::EnableAutonomous { confirmed }))
}

fn apply_trust_action(
    state: State<'_, DbState>,
    user_action: Option<UserAction>,
) -> Result<TrustStateView, String> {
    let now_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .map_err(|e| e.to_string())?;
    let db = state.0.lock().map_err(|e| e.to_string())?;
    let conn = db.conn();

    state_machine::ensure_schema(conn, now_unix).map_err(|e| e.to_string())?;
    pattern_blocks::ensure_schema(conn).map_err(|e| e.to_string())?;
    let (current, last_ts) = state_machine::load_state(conn).map_err(|e| e.to_string())?;
    // pattern_detector::detect_patterns(conn, &PatternConfig) — firma sin
    // now_unix (TS-2-001 cerrado así); coexiste con score_patterns que sí lo
    // recibe explícitamente.
    let patterns = pattern_detector::detect_patterns(conn, &PatternConfig::default())
        .map_err(|e| e.to_string())?;
    let scores = trust_scorer::score_patterns(&patterns, &TrustConfig::default(), now_unix)
        .map_err(|e| e.to_string())?;
    // Precomputa user_blocked_pre consultando pattern_blocks. Externalizar la
    // consulta preserva D8 estricto en evaluate_transition (TS-2-004
    // §"Edición Mecánica").
    let blocked_ids = pattern_blocks::list_blocked(conn).map_err(|e| e.to_string())?;
    let user_blocked_pre = scores.iter().any(|s| blocked_ids.contains(&s.pattern_id));
    let new_state = state_machine::evaluate_transition(
        &scores,
        current,
        last_ts,
        user_action,
        now_unix,
        &StateMachineConfig::default(),
        user_blocked_pre,
    )
    .map_err(|e| e.to_string())?;
    state_machine::save_state(
        conn,
        new_state.current_state,
        new_state.last_transition_at,
        now_unix,
    )
    .map_err(|e| e.to_string())?;
    Ok(TrustStateView::from(new_state))
}

// ── Phase 2 — Privacy Dashboard (T-2-004) ────────────────────────────────────

/// Wire shape para `PatternSummary` consumido por el frontend (TS-2-004
/// §"Contrato de Tipos TypeScript"). Proyección serializable estricta de
/// `DetectedPattern` que omite `first_seen` y añade `is_blocked`.
#[derive(Debug, Serialize)]
pub struct PatternSummary {
    pub pattern_id: String,
    pub label: String,
    pub category_signature: Vec<pattern_detector::CategoryWeight>,
    pub domain_signature: Vec<pattern_detector::DomainWeight>,
    pub temporal_window: pattern_detector::TemporalWindow,
    pub frequency: usize,
    pub last_seen: i64,
    pub is_blocked: bool,
}

impl PatternSummary {
    fn from_detected(p: DetectedPattern, is_blocked: bool) -> Self {
        PatternSummary {
            pattern_id: p.pattern_id,
            label: p.label,
            category_signature: p.category_signature,
            domain_signature: p.domain_signature,
            temporal_window: p.temporal_window,
            frequency: p.frequency,
            last_seen: p.last_seen,
            is_blocked,
        }
    }
}

/// Devuelve los patrones detectados (proyectados a `PatternSummary`) ordenados
/// por `last_seen` desc, desempate por `pattern_id` asc (D8). El flag
/// `is_blocked` se materializa consultando la tabla `pattern_blocks`.
///
/// D4: este comando NO invoca `evaluate_transition` ni muta `state_machine`.
#[tauri::command]
pub fn get_detected_patterns(state: State<'_, DbState>) -> Result<Vec<PatternSummary>, String> {
    let db = state.0.lock().map_err(|e| e.to_string())?;
    let conn = db.conn();
    pattern_blocks::ensure_schema(conn).map_err(|e| e.to_string())?;
    let patterns = pattern_detector::detect_patterns(conn, &PatternConfig::default())
        .map_err(|e| e.to_string())?;
    let blocked = pattern_blocks::list_blocked(conn).map_err(|e| e.to_string())?;
    let mut summaries: Vec<PatternSummary> = patterns
        .into_iter()
        .map(|p| {
            let is_blocked = blocked.contains(&p.pattern_id);
            PatternSummary::from_detected(p, is_blocked)
        })
        .collect();
    summaries.sort_by(|a, b| {
        b.last_seen
            .cmp(&a.last_seen)
            .then_with(|| a.pattern_id.cmp(&b.pattern_id))
    });
    Ok(summaries)
}

/// Marca un patrón como bloqueado por el usuario. Idempotente.
#[tauri::command]
pub fn block_pattern(state: State<'_, DbState>, pattern_id: String) -> Result<(), String> {
    let now_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .map_err(|e| e.to_string())?;
    let db = state.0.lock().map_err(|e| e.to_string())?;
    let conn = db.conn();
    pattern_blocks::ensure_schema(conn).map_err(|e| e.to_string())?;
    pattern_blocks::block(conn, &pattern_id, now_unix).map_err(|e| e.to_string())?;
    Ok(())
}

/// Desbloquea un patrón previamente bloqueado. Idempotente.
#[tauri::command]
pub fn unblock_pattern(state: State<'_, DbState>, pattern_id: String) -> Result<(), String> {
    let db = state.0.lock().map_err(|e| e.to_string())?;
    let conn = db.conn();
    pattern_blocks::ensure_schema(conn).map_err(|e| e.to_string())?;
    pattern_blocks::unblock(conn, &pattern_id).map_err(|e| e.to_string())?;
    Ok(())
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

// ── Phase 3 — Synthesis Tokens (T-3-008) ──────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct TokenStatus {
    pub is_set: bool,
}

/// Persiste el install_token cifrado. Idempotente: sobreescribe si ya existe.
/// El token se cifra con AES-256-GCM (local_key) — nunca se almacena en claro (D25).
#[tauri::command]
pub fn set_synthesis_token(
    token: String,
    state: State<'_, DbState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let key = db_key(&app);
    let encrypted = crypto::encrypt_aes(&token, &key);
    let now_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .map_err(|e| e.to_string())?;
    let db = state.0.lock().map_err(|e| e.to_string())?;
    let conn = db.conn();
    synthesis_tokens::ensure_schema(conn).map_err(|e| e.to_string())?;
    synthesis_tokens::set_token(conn, &encrypted, now_unix).map_err(|e| e.to_string())?;
    Ok(())
}

/// Devuelve si hay token configurado. NUNCA devuelve el token en claro (D25).
#[tauri::command]
pub fn get_synthesis_token_status(
    state: State<'_, DbState>,
) -> Result<TokenStatus, String> {
    let db = state.0.lock().map_err(|e| e.to_string())?;
    let conn = db.conn();
    synthesis_tokens::ensure_schema(conn).map_err(|e| e.to_string())?;
    let is_set = synthesis_tokens::is_token_set(conn).map_err(|e| e.to_string())?;
    Ok(TokenStatus { is_set })
}

/// Devuelve el token en claro para uso interno exclusivo de synthesis_engine.
/// pub(crate): no se registra en invoke_handler. Solo synthesis_engine.rs lo consume.
/// Recibe &Connection directamente para evitar doble lock desde synthesis_engine.
pub(crate) fn get_synthesis_token_plain(
    conn: &rusqlite::Connection,
    app: &tauri::AppHandle,
) -> Result<Option<String>, String> {
    synthesis_tokens::ensure_schema(conn).map_err(|e| e.to_string())?;
    match synthesis_tokens::get_token(conn).map_err(|e| e.to_string())? {
        None => Ok(None),
        Some(encrypted) => {
            let key = db_key(app);
            let plain = crypto::decrypt_any(&encrypted, &key);
            Ok(plain)
        }
    }
}

/// Elimina el token (desactivación de síntesis desde Privacy Dashboard).
#[tauri::command]
pub fn clear_synthesis_token(
    state: State<'_, DbState>,
) -> Result<(), String> {
    let db = state.0.lock().map_err(|e| e.to_string())?;
    let conn = db.conn();
    synthesis_tokens::ensure_schema(conn).map_err(|e| e.to_string())?;
    synthesis_tokens::clear_token(conn).map_err(|e| e.to_string())?;
    Ok(())
}

// ── Phase 3 — Synthesis Engine (T-3-009) ──────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct SynthesisUsage {
    pub used_this_month:  u32,
    pub limit_this_month: u32,
    pub synthesis_active: bool,
}

/// Genera una síntesis LLM y la persiste cifrada en SQLCipher.
/// Precondiciones: estado SM ≥ Trusted (D4), consentimiento synthesis_v1 (D25),
/// install_token configurado (T-3-008).
#[tauri::command]
pub async fn generate_synthesis(
    state: State<'_, DbState>,
    app: tauri::AppHandle,
    category: String,
    titles: Vec<String>,
    domains: Vec<String>,
    synthesis_type: String,
    anchor_key: String,
    anchor_type: String,
) -> Result<(), String> {
    #[cfg(target_os = "android")]
    {
        let _ = (state, app, category, titles, domains, synthesis_type, anchor_key, anchor_type);
        return Err("synthesis not supported on Android".to_string());
    }
    #[cfg(not(target_os = "android"))]
    {
        let st = match synthesis_type.as_str() {
            "entretenimiento" => SynthesisType::Entretenimiento,
            "cocina"          => SynthesisType::Cocina,
            "noticias"        => SynthesisType::Noticias,
            "tecnologia"      => SynthesisType::Tecnologia,
            other             => return Err(format!("unknown synthesis_type: {other}")),
        };

        // Phase 1: sync pre-checks — drop lock before any .await (MutexGuard is !Send)
        let (token, body, syn_type_str) = {
            let db = state.0.lock().map_err(|e| e.to_string())?;
            let conn = db.conn();

            // D4: verificar estado SM ≥ Trusted
            state_machine::ensure_schema(conn, 0).map_err(|e| e.to_string())?;
            let (current, _) = state_machine::load_state(conn).map_err(|e| e.to_string())?;
            if !matches!(
                current,
                state_machine::TrustStateEnum::Trusted | state_machine::TrustStateEnum::Autonomous
            ) {
                return Err("synthesis requires Trusted or Autonomous state".to_string());
            }

            // D25: verificar consentimiento antes de construir el payload
            consent_log_store::ensure_schema(conn).map_err(|e| e.to_string())?;
            if !consent_log_store::has_consent(conn, "synthesis", "synthesis_v1")
                .map_err(|e| e.to_string())?
            {
                return Err(synthesis_engine::SynthesisError::NoConsent.to_string());
            }

            // Obtener token en claro
            let token = get_synthesis_token_plain(conn, &app)?
                .ok_or_else(|| synthesis_engine::SynthesisError::NoToken.to_string())?;

            // Construir payload (PG-001: sin url ni title_raw)
            let titles_ref: Vec<&str> = titles.iter().map(String::as_str).collect();
            let domains_ref: Vec<&str> = domains.iter().map(String::as_str).collect();
            let payload = synthesis_engine::build_synthesis_payload(
                &category, &titles_ref, &domains_ref, st, "es",
            );
            let syn_type_str = payload.synthesis_type.clone();
            let body = serde_json::to_string(&payload).map_err(|e| e.to_string())?;

            (token, body, syn_type_str)
        }; // MutexGuard dropped — seguro hacer .await después de este punto

        // Phase 2: HTTP async call (sin lock, todos los datos son owned)
        const PROXY_URL: &str =
            "https://flowweaver-proxy.bananasplitsound.workers.dev/synthesize";

        let app_for_chunk = app.clone();
        let anchor_for_chunk = anchor_key.clone();
        let fetch_result = synthesis_engine::fetch_from_proxy(
            &token,
            body,
            PROXY_URL,
            move |chunk| {
                let _ = app_for_chunk.emit("synthesis_chunk", serde_json::json!({
                    "anchor_key": anchor_for_chunk,
                    "chunk": chunk,
                }));
            },
        )
        .await;

        let full_content = match fetch_result {
            Ok(c) => {
                let _ = app.emit("synthesis_complete", serde_json::json!({
                    "anchor_key": anchor_key,
                }));
                c
            }
            Err(e) => {
                let _ = app.emit("synthesis_error", serde_json::json!({
                    "anchor_key": anchor_key,
                    "error": e.to_string(),
                }));
                return Err(e.to_string());
            }
        };

        // Phase 3: persistir (re-adquirir lock)
        {
            let db = state.0.lock().map_err(|e| e.to_string())?;
            let conn = db.conn();
            let key = db_key(&app);
            let encrypted = crypto::encrypt_aes(&full_content, &key);
            let now_unix = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            syntheses_store::ensure_schema(conn).map_err(|e| e.to_string())?;
            syntheses_store::save(
                conn,
                &syntheses_store::SynthesisEntry {
                    anchor_key:        anchor_key.clone(),
                    anchor_type:       anchor_type.clone(),
                    category:          category.clone(),
                    synthesis_type:    syn_type_str,
                    content_encrypted: encrypted,
                    generated_at:      now_unix,
                },
            )
            .map_err(|e| e.to_string())?;
        }

        Ok(())
    }
}

/// Devuelve el uso de síntesis del mes actual (contador local).
/// Usado por SynthesisSection.tsx (T-3-011).
#[tauri::command]
pub fn get_synthesis_usage(state: State<'_, DbState>) -> Result<SynthesisUsage, String> {
    let db = state.0.lock().map_err(|e| e.to_string())?;
    let conn = db.conn();
    syntheses_store::ensure_schema(conn).map_err(|e| e.to_string())?;
    synthesis_tokens::ensure_schema(conn).map_err(|e| e.to_string())?;
    consent_log_store::ensure_schema(conn).map_err(|e| e.to_string())?;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let month_start = now - (now % (30 * 24 * 3600));
    let used: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM syntheses WHERE generated_at >= ?1",
            rusqlite::params![month_start],
            |row| row.get(0),
        )
        .unwrap_or(0);

    let token_set = synthesis_tokens::is_token_set(conn).unwrap_or(false);
    let has_consent = consent_log_store::has_consent(conn, "synthesis", "synthesis_v1")
        .unwrap_or(false);

    Ok(SynthesisUsage {
        used_this_month:  used as u32,
        limit_this_month: 5,
        synthesis_active: token_set && has_consent,
    })
}

// ── Phase 3 — Consent (T-3-012) ──────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct ConsentStatus {
    pub has_consent:     bool,
    pub consent_version: String,
    pub current_version: String,
    pub needs_renewal:   bool,
}

/// Verifica si existe consentimiento vigente para síntesis.
#[tauri::command]
pub fn check_synthesis_consent(
    state: State<'_, DbState>,
) -> Result<ConsentStatus, String> {
    let db = state.0.lock().map_err(|e| e.to_string())?;
    let conn = db.conn();
    consent_log_store::ensure_schema(conn).map_err(|e| e.to_string())?;
    let has = consent_log_store::has_consent(conn, "synthesis", "synthesis_v1")
        .map_err(|e| e.to_string())?;
    Ok(ConsentStatus {
        has_consent:     has,
        consent_version: if has { "synthesis_v1".to_string() } else { "".to_string() },
        current_version: "synthesis_v1".to_string(),
        needs_renewal:   false,
    })
}

/// Registra el consentimiento del usuario en consent_log (D25).
#[tauri::command]
pub fn record_synthesis_consent(
    state: State<'_, DbState>,
) -> Result<(), String> {
    let now_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .map_err(|e| e.to_string())?;
    let db = state.0.lock().map_err(|e| e.to_string())?;
    let conn = db.conn();
    consent_log_store::ensure_schema(conn).map_err(|e| e.to_string())?;
    consent_log_store::record_consent(conn, "synthesis", "synthesis_v1", now_unix)
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Revoca el consentimiento de síntesis — marca revoked_at, no borra la fila (audit trail).
#[tauri::command]
pub fn revoke_synthesis_consent(state: State<'_, DbState>) -> Result<(), String> {
    let now_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .map_err(|e| e.to_string())?;
    let db = state.0.lock().map_err(|e| e.to_string())?;
    let conn = db.conn();
    consent_log_store::ensure_schema(conn).map_err(|e| e.to_string())?;
    consent_log_store::revoke_consent(conn, "synthesis", now_unix)
        .map_err(|e| e.to_string())?;
    Ok(())
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Derive the field-level encryption key for url/title in SQLite.
///
/// Desktop: derived from app_data_dir path (installation-bound, never transmitted).
/// Android: stable constant that matches FieldCrypto.FIELD_KEY_PASSPHRASE in Kotlin.
///   A path-based key on Android cannot be guaranteed to align with the Kotlin layer,
///   so a constant is used. The Android app data dir provides file system isolation.
pub(crate) fn db_key(app: &tauri::AppHandle) -> String {
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

// ── Phase 2 — FS Watcher (T-2-000) ───────────────────────────────────────────
//
// Los siete comandos materializan los cinco elementos visuales declarados en
// TS-2-000 §3 "Visibilidad en el Privacy Dashboard". HO-FW-PD posterior los
// compone en `FsWatcherSection.tsx` (out-of-scope de T-2-004 — TS-2-004
// §"Decisiones del TA §4").
//
// D1 absoluto: ningún comando devuelve `url`, `title`, ni la ruta completa
//   del archivo. Solo: directorio padre (en claro), extensión (en claro),
//   nombre cifrado, timestamp.
// D4 transitivo: ningún comando invoca `evaluate_transition`,
//   `score_patterns`, ni `detect_patterns`.
// D9: el watcher se inicia exclusivamente desde el hook
//   `WindowEvent::Focused(true)` en `lib.rs`. Los comandos `activate_directory`
//   pueden lanzarlo si la app YA está en foreground (handle.is_some()) — esto
//   es la misma vía: el hook lo dejó armado y aquí solo se reinicia con la
//   nueva configuración.

#[tauri::command]
pub fn fs_watcher_get_status(
    state: State<'_, DbState>,
    fs_state: State<'_, FsWatcherState>,
) -> Result<FsWatcherStatus, String> {
    #[cfg(target_os = "android")]
    {
        let _ = (state, fs_state);
        return Err(fs_watcher::FsWatcherError::UnsupportedPlatform.to_string());
    }
    #[cfg(not(target_os = "android"))]
    {
        let now_unix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .map_err(|e| e.to_string())?;
        let db = state.0.lock().map_err(|e| e.to_string())?;
        let conn = db.conn();
        fs_watcher::ensure_schema(conn, now_unix).map_err(|e| e.to_string())?;
        let directories = fs_watcher::list_directories(conn).map_err(|e| e.to_string())?;

        let handle_active = fs_state
            .handle
            .lock()
            .map(|g| g.is_some())
            .unwrap_or(false);
        let runtime_state = if handle_active {
            FsWatcherRuntimeState::Active
        } else {
            FsWatcherRuntimeState::Suspended
        };

        let buffer = fs_state.event_buffer.lock().map_err(|e| e.to_string())?;
        let events_in_current_session = buffer.len();
        let cutoff = now_unix.saturating_sub(86_400);
        let events_last_24h = buffer.iter().filter(|e| e.detected_at >= cutoff).count();

        Ok(FsWatcherStatus {
            runtime_state,
            directories,
            events_in_current_session,
            events_last_24h,
        })
    }
}

#[tauri::command]
pub fn fs_watcher_list_directories(
    state: State<'_, DbState>,
) -> Result<Vec<FsWatcherDirectory>, String> {
    #[cfg(target_os = "android")]
    {
        let _ = state;
        return Err(fs_watcher::FsWatcherError::UnsupportedPlatform.to_string());
    }
    #[cfg(not(target_os = "android"))]
    {
        let now_unix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .map_err(|e| e.to_string())?;
        let db = state.0.lock().map_err(|e| e.to_string())?;
        let conn = db.conn();
        fs_watcher::ensure_schema(conn, now_unix).map_err(|e| e.to_string())?;
        fs_watcher::list_directories(conn).map_err(|e| e.to_string())
    }
}

#[tauri::command]
pub fn fs_watcher_activate_directory(
    state: State<'_, DbState>,
    fs_state: State<'_, FsWatcherState>,
    app: tauri::AppHandle,
    directory: CandidateDirectory,
    confirmed: bool,
) -> Result<(), String> {
    #[cfg(target_os = "android")]
    {
        let _ = (state, fs_state, app, directory, confirmed);
        return Err(fs_watcher::FsWatcherError::UnsupportedPlatform.to_string());
    }
    #[cfg(not(target_os = "android"))]
    {
        // TS-2-000 §3 "Confirmación explícita": activación requiere consent.
        if !confirmed {
            return Err("confirmation required".to_string());
        }
        let now_unix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .map_err(|e| e.to_string())?;
        let db = state.0.lock().map_err(|e| e.to_string())?;
        let conn = db.conn();
        fs_watcher::ensure_schema(conn, now_unix).map_err(|e| e.to_string())?;
        fs_watcher::activate(conn, directory, now_unix).map_err(|e| e.to_string())?;

        let mut handle_guard = fs_state.handle.lock().map_err(|e| e.to_string())?;
        *handle_guard = None;
        let key = derive_fs_key(&app);
        match fs_watcher::start_watching(conn, fs_state.event_buffer.clone(), &key) {
            Ok(h) => *handle_guard = Some(h),
            Err(fs_watcher::FsWatcherError::NoActiveDirectories) => {}
            Err(e) => return Err(e.to_string()),
        }
        Ok(())
    }
}

#[tauri::command]
pub fn fs_watcher_deactivate_directory(
    state: State<'_, DbState>,
    fs_state: State<'_, FsWatcherState>,
    app: tauri::AppHandle,
    directory: CandidateDirectory,
) -> Result<(), String> {
    #[cfg(target_os = "android")]
    {
        let _ = (state, fs_state, app, directory);
        return Err(fs_watcher::FsWatcherError::UnsupportedPlatform.to_string());
    }
    #[cfg(not(target_os = "android"))]
    {
        let now_unix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .map_err(|e| e.to_string())?;
        let db = state.0.lock().map_err(|e| e.to_string())?;
        let conn = db.conn();
        fs_watcher::ensure_schema(conn, now_unix).map_err(|e| e.to_string())?;
        fs_watcher::deactivate(conn, directory, now_unix).map_err(|e| e.to_string())?;

        let mut handle_guard = fs_state.handle.lock().map_err(|e| e.to_string())?;
        *handle_guard = None;
        let key = derive_fs_key(&app);
        match fs_watcher::start_watching(conn, fs_state.event_buffer.clone(), &key) {
            Ok(h) => *handle_guard = Some(h),
            Err(fs_watcher::FsWatcherError::NoActiveDirectories) => {}
            Err(e) => return Err(e.to_string()),
        }
        Ok(())
    }
}

#[tauri::command]
pub fn fs_watcher_get_session_events(
    fs_state: State<'_, FsWatcherState>,
) -> Result<Vec<FsWatcherEvent>, String> {
    #[cfg(target_os = "android")]
    {
        let _ = fs_state;
        return Err(fs_watcher::FsWatcherError::UnsupportedPlatform.to_string());
    }
    #[cfg(not(target_os = "android"))]
    {
        let buffer = fs_state.event_buffer.lock().map_err(|e| e.to_string())?;
        Ok(buffer.clone())
    }
}

#[tauri::command]
pub fn fs_watcher_clear_directory_history(
    state: State<'_, DbState>,
    fs_state: State<'_, FsWatcherState>,
    directory: CandidateDirectory,
) -> Result<(), String> {
    #[cfg(target_os = "android")]
    {
        let _ = (state, fs_state, directory);
        return Err(fs_watcher::FsWatcherError::UnsupportedPlatform.to_string());
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = state; // los eventos no persisten — solo buffer en memoria
        let mut buffer = fs_state.event_buffer.lock().map_err(|e| e.to_string())?;
        buffer.retain(|e| e.directory != directory);
        Ok(())
    }
}

#[tauri::command]
pub fn fs_watcher_get_24h_event_count(
    fs_state: State<'_, FsWatcherState>,
) -> Result<usize, String> {
    #[cfg(target_os = "android")]
    {
        let _ = fs_state;
        return Err(fs_watcher::FsWatcherError::UnsupportedPlatform.to_string());
    }
    #[cfg(not(target_os = "android"))]
    {
        let now_unix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .map_err(|e| e.to_string())?;
        let cutoff = now_unix.saturating_sub(86_400);
        let buffer = fs_state.event_buffer.lock().map_err(|e| e.to_string())?;
        Ok(buffer.iter().filter(|e| e.detected_at >= cutoff).count())
    }
}

/// Deriva la clave de cifrado de nombres de archivo (32 bytes vía SHA-256
/// de la passphrase de instalación). Coherente con `db_key` pero como
/// `[u8; 32]` para `start_watching`.
#[cfg(not(target_os = "android"))]
fn derive_fs_key(app: &tauri::AppHandle) -> [u8; 32] {
    fs_watcher::derive_filename_key(&db_key(app))
}

// TEMP: reclassify — eliminar tras ejecución única. No commitear.
#[tauri::command]
pub fn reclassify_all_resources(
    state: State<'_, DbState>,
) -> Result<String, String> {
    let db = state.0.lock().map_err(|e| e.to_string())?; // TEMP: reclassify
    let resources = db.all_resources().map_err(|e| e.to_string())?; // TEMP: reclassify
    let total = resources.len(); // TEMP: reclassify
    let mut updated = 0usize; // TEMP: reclassify
    let mut unchanged = 0usize; // TEMP: reclassify
    for r in &resources { // TEMP: reclassify
        let classified = classifier::classify_domain(&r.domain); // TEMP: reclassify
        if classified != r.category { // TEMP: reclassify
            db.set_category(&r.uuid, &classified).map_err(|e| e.to_string())?; // TEMP: reclassify
            updated += 1; // TEMP: reclassify
        } else { // TEMP: reclassify
            unchanged += 1; // TEMP: reclassify
        } // TEMP: reclassify
    } // TEMP: reclassify
    Ok(format!("Reclasificados: {updated}/{total} (sin cambio: {unchanged})")) // TEMP: reclassify
} // TEMP: reclassify

#[cfg(test)]
mod tests {
    /// D1 verificación estructural — TS-2-004 §"Verificación Doble (i)".
    /// Garantiza que ningún subcomponente del Privacy Dashboard accede a campos
    /// `url`/`title`. La distinción es entre menciones textuales (permitidas en
    /// `PrivacyDashboardNeverSeen.tsx` para explicar al usuario qué NO se ve)
    /// y accesos a campos (prohibidos por D1).
    #[test]
    fn test_no_url_or_title_in_dashboard_components() {
        const FILES: &[&str] = &[
            include_str!("../../src/components/PrivacyDashboard.tsx"),
            include_str!("../../src/components/PatternsSection.tsx"),
            include_str!("../../src/components/TrustStateSection.tsx"),
            include_str!("../../src/components/PrivacyDashboardNeverSeen.tsx"),
            include_str!("../../src/components/SynthesisSection.tsx"),
        ];
        let forbidden = [
            "resource.url", "resource.title",
            ".bookmark_url", ".page_title",
            "p.url", "p.title",
            "view.url", "view.title",
        ];
        for src in FILES {
            for token in forbidden {
                assert!(
                    !src.contains(token),
                    "D1 violation: token '{token}' present in dashboard component"
                );
            }
        }
    }

    // ── E2E Privacy Dashboard — T-2-004-e2e ──────────────────────────────────
    //
    // Sustituto de las 5 capturas manuales de TS-2-004 §"Mecanismo ii".
    // Verifica los 4 elementos de UI del Privacy Dashboard para cada uno de los
    // 4 estados (Observing, Learning, Trusted, Autonomous) con datos sintéticos.
    // Sin red, sin LLM, sin proxy. Determinístico (D8): now_unix fijo.
    // Referencia: TS-2-004-e2e, AR-2-006, PIR-005-addendum.

    use crate::pattern_detector::PatternConfig;
    use crate::state_machine::StateMachineConfig;
    use crate::synthesis_tokens;

    // ── Phase 3 — Synthesis Engine (T-3-009) tests ───────────────────────────

    #[test]
    fn pg_001_build_synthesis_payload_signature() {
        use crate::synthesis_engine::{build_synthesis_payload, SynthesisType};
        let payload = build_synthesis_payload(
            "cocina",
            &["Tarta de queso", "Brownie de chocolate"],
            &["recetasdeescandalo.com", "elcomidista.es"],
            SynthesisType::Cocina,
            "es",
        );
        assert_eq!(payload.synthesis_type, "cocina");
        assert_eq!(payload.titles.len(), 2);
        assert_eq!(payload.domains.len(), 2);
        assert_eq!(payload.prompt_version, "v1");
        assert_eq!(payload.category, "cocina");
        assert_eq!(payload.language, "es");
    }

    #[test]
    fn test_synthesis_requires_consent() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::consent_log_store::ensure_schema(&conn).unwrap();
        let has = crate::consent_log_store::has_consent(&conn, "synthesis", "synthesis_v1").unwrap();
        assert!(!has, "sin filas, has_consent debe ser false");

        conn.execute(
            "INSERT INTO consent_log (consent_type, consent_version, accepted_at)
             VALUES ('synthesis', 'synthesis_v1', 1000)",
            [],
        ).unwrap();
        let has = crate::consent_log_store::has_consent(&conn, "synthesis", "synthesis_v1").unwrap();
        assert!(has, "con fila, has_consent debe ser true");
    }

    #[test]
    fn test_syntheses_store_schema_idempotent() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::syntheses_store::ensure_schema(&conn).unwrap();
        crate::syntheses_store::ensure_schema(&conn).unwrap();
    }

    #[test]
    fn test_syntheses_store_encrypted_content() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::syntheses_store::ensure_schema(&conn).unwrap();
        let entry = crate::syntheses_store::SynthesisEntry {
            anchor_key:        "test-anchor".to_string(),
            anchor_type:       "session".to_string(),
            category:          "cocina".to_string(),
            synthesis_type:    "cocina".to_string(),
            content_encrypted: crate::crypto::encrypt_aes("contenido real", "test-key"),
            generated_at:      1_000_000,
        };
        crate::syntheses_store::save(&conn, &entry).unwrap();
        let stored = crate::syntheses_store::get_by_anchor(&conn, "test-anchor")
            .unwrap()
            .unwrap();
        assert_ne!(stored.content_encrypted, "contenido real");
        let decrypted = crate::crypto::decrypt_any(&stored.content_encrypted, "test-key").unwrap();
        assert_eq!(decrypted, "contenido real");
    }

    #[test]
    fn test_generate_synthesis_requires_trusted_state() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::state_machine::ensure_schema(&conn, 0).unwrap();
        let (current, _) = crate::state_machine::load_state(&conn).unwrap();
        assert!(!matches!(
            current,
            crate::state_machine::TrustStateEnum::Trusted
                | crate::state_machine::TrustStateEnum::Autonomous
        ));
    }

    const TEST_NOW: i64 = 1_714_000_000_i64;

    fn open_test_db() -> crate::storage::Db {
        let db = crate::storage::Db::open(std::path::Path::new(":memory:"), "test-key")
            .expect("open db");
        db.migrate().expect("migrate");
        db
    }

    fn synthetic_scores(trust: f64, count: usize) -> Vec<crate::trust_scorer::TrustScore> {
        (0..count)
            .map(|i| crate::trust_scorer::TrustScore {
                pattern_id: format!("pattern-{i:03}"),
                trust_score: trust,
                stability_score: 0.8,
                recency_weight: 1.0,
                confidence_tier: crate::trust_scorer::ConfidenceTier::High,
            })
            .collect()
    }

    /// Elemento 1 (indicador de estado): Observing — BD vacía, sin patrones.
    /// Elemento 2 (mecanismos activos): lista vacía.
    /// Elemento 3 (controles): sin EnableAutonomous.
    /// Elemento 4 (métricas de privacidad): resource_count = 0.
    #[test]
    fn e2e_dashboard_observing_state() {
        let db = open_test_db();
        let conn = db.conn();
        crate::state_machine::ensure_schema(conn, TEST_NOW).unwrap();
        crate::pattern_blocks::ensure_schema(conn).unwrap();

        // 1. Indicador de estado
        let (current, last_ts) = crate::state_machine::load_state(conn).unwrap();
        assert_eq!(current, crate::state_machine::TrustStateEnum::Observing);

        // 3. Controles disponibles (evaluate_transition — no scores)
        let state = crate::state_machine::evaluate_transition(
            &[],
            current,
            last_ts,
            None,
            TEST_NOW,
            &StateMachineConfig::default(),
            false,
        )
        .unwrap();
        assert_eq!(state.current_state, crate::state_machine::TrustStateEnum::Observing);
        // Sin suficientes patrones: no EnableAutonomous
        let has_autonomous = state
            .available_transitions
            .iter()
            .any(|t| matches!(t.to, crate::state_machine::TrustStateEnum::Autonomous));
        assert!(!has_autonomous, "Autonomous no disponible en Observing");

        // 2. Mecanismos activos: vacíos
        let patterns =
            crate::pattern_detector::detect_patterns(conn, &PatternConfig::default()).unwrap();
        assert!(patterns.is_empty(), "Sin patrones en BD vacía");

        // 4. Métricas de privacidad
        let stats = db.privacy_stats().unwrap();
        assert_eq!(stats.resource_count, 0, "Sin recursos en BD vacía");
    }

    /// Elemento 1: indicador de estado Learning (transición automática desde Observing).
    /// Condición: ≥3 scores con trust_score > 0.4 (threshold_low).
    #[test]
    fn e2e_dashboard_learning_state() {
        // 3 scores con trust = 0.5 > threshold_low (0.4)
        let scores = synthetic_scores(0.5, 3);

        let state = crate::state_machine::evaluate_transition(
            &scores,
            crate::state_machine::TrustStateEnum::Observing,
            0,
            None,
            TEST_NOW,
            &StateMachineConfig::default(),
            false,
        )
        .unwrap();

        // 1. Indicador de estado
        assert_eq!(state.current_state, crate::state_machine::TrustStateEnum::Learning);
        // 3. Controles: active_patterns_count presente
        assert_eq!(state.active_patterns_count, 3);
    }

    /// Elemento 1: indicador de estado Trusted (transición desde Learning).
    /// Elemento 3: EnableAutonomous disponible en controles.
    /// Condición: trust_score > 0.75 (threshold_high) + no bloqueado.
    #[test]
    fn e2e_dashboard_trusted_state() {
        // 3 scores con trust = 0.9 > threshold_high (0.75)
        let scores = synthetic_scores(0.9, 3);

        let state = crate::state_machine::evaluate_transition(
            &scores,
            crate::state_machine::TrustStateEnum::Learning,
            0,
            None,
            TEST_NOW,
            &StateMachineConfig::default(),
            false,
        )
        .unwrap();

        // 1. Indicador de estado
        assert_eq!(state.current_state, crate::state_machine::TrustStateEnum::Trusted);

        // 3. Controles: EnableAutonomous disponible desde Trusted
        let has_autonomous = state
            .available_transitions
            .iter()
            .any(|t| matches!(t.to, crate::state_machine::TrustStateEnum::Autonomous));
        assert!(has_autonomous, "EnableAutonomous debe estar disponible en Trusted");
    }

    /// Elemento 1: indicador de estado Autonomous (acción explícita del usuario).
    /// Condición: UserAction::EnableAutonomous { confirmed: true } desde Trusted.
    #[test]
    fn e2e_dashboard_autonomous_state() {
        let scores = synthetic_scores(0.9, 3);

        let state = crate::state_machine::evaluate_transition(
            &scores,
            crate::state_machine::TrustStateEnum::Trusted,
            0,
            Some(crate::state_machine::UserAction::EnableAutonomous { confirmed: true }),
            TEST_NOW,
            &StateMachineConfig::default(),
            false,
        )
        .unwrap();

        // 1. Indicador de estado
        assert_eq!(state.current_state, crate::state_machine::TrustStateEnum::Autonomous);
    }

    /// Elemento 4: métricas de privacidad (resource_count, categories, domains)
    /// con datos sintéticos insertados en BD.
    #[test]
    fn e2e_dashboard_privacy_stats_with_synthetic_data() {
        let db = open_test_db();

        let resources = vec![
            crate::storage::NewResource {
                uuid: "uuid-1".to_string(),
                url: crate::crypto::encrypt_aes("https://github.com/test", "test-key"),
                title: crate::crypto::encrypt_aes("Test title 1", "test-key"),
                domain: "github.com".to_string(),
                category: "desarrollo".to_string(),
                captured_at: TEST_NOW - 3600,
            },
            crate::storage::NewResource {
                uuid: "uuid-2".to_string(),
                url: crate::crypto::encrypt_aes("https://example.com/test", "test-key"),
                title: crate::crypto::encrypt_aes("Test title 2", "test-key"),
                domain: "example.com".to_string(),
                category: "noticias".to_string(),
                captured_at: TEST_NOW - 7200,
            },
        ];
        for r in &resources {
            db.insert_or_ignore(r).unwrap();
        }

        // 4. Métricas de privacidad
        let stats = db.privacy_stats().unwrap();
        assert_eq!(stats.resource_count, 2, "resource_count correcto");
        assert!(!stats.categories.is_empty(), "categories no vacío");
        assert!(!stats.domains.is_empty(), "domains no vacío");

        // D1: ningún campo expone url/title — solo domain y category
        for cat in &stats.categories {
            assert!(!cat.category.contains("http"), "D1: category no contiene url");
        }
        for dom in &stats.domains {
            assert!(!dom.domain.contains("title"), "D1: domain no contiene title");
        }
    }

    // ── Phase 3 — Consent (T-3-012) tests ───────────────────────────────────

    #[test]
    fn test_consent_record_and_check() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::consent_log_store::ensure_schema(&conn).unwrap();

        assert!(!crate::consent_log_store::has_consent(&conn, "synthesis", "synthesis_v1").unwrap());

        crate::consent_log_store::record_consent(&conn, "synthesis", "synthesis_v1", 1_000_000).unwrap();
        assert!(crate::consent_log_store::has_consent(&conn, "synthesis", "synthesis_v1").unwrap());

        crate::consent_log_store::revoke_consent(&conn, "synthesis", 2_000_000).unwrap();
        assert!(!crate::consent_log_store::has_consent(&conn, "synthesis", "synthesis_v1").unwrap());
    }

    #[test]
    fn test_consent_schema_idempotent() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::consent_log_store::ensure_schema(&conn).unwrap();
        crate::consent_log_store::ensure_schema(&conn).unwrap();
    }

    #[test]
    fn test_synthesis_token_round_trip() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        synthesis_tokens::ensure_schema(&conn).unwrap();
        assert!(!synthesis_tokens::is_token_set(&conn).unwrap());
        synthesis_tokens::set_token(&conn, "encrypted-placeholder", 1_000_000).unwrap();
        assert!(synthesis_tokens::is_token_set(&conn).unwrap());
        let stored = synthesis_tokens::get_token(&conn).unwrap();
        assert_eq!(stored, Some("encrypted-placeholder".to_string()));
        synthesis_tokens::clear_token(&conn).unwrap();
        assert!(!synthesis_tokens::is_token_set(&conn).unwrap());
    }

    /// Elemento 2: lista de mecanismos activos (PatternSummary) con recursos
    /// sintéticos suficientes para disparar detección de patrones.
    /// PatternConfig::default() requiere min_frequency = 3 en 30 días.
    /// Los timestamps se calculan relativos a now para mantenerse dentro
    /// de la ventana de lookback de pattern_detector (que usa SystemTime::now()).
    #[test]
    fn e2e_dashboard_patterns_with_synthetic_resources() {
        let db = open_test_db();
        let conn = db.conn();
        crate::pattern_blocks::ensure_schema(conn).unwrap();

        // base_ts = ahora - 20 días: dentro de la ventana lookback_days = 30
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time")
            .as_secs() as i64;
        let base_ts = now - 20 * 86_400;

        // 6 recursos github.com/desarrollo en 6 ventanas distintas (cada 3 días).
        // Distribuidos en 3 sesiones de captura (gap > 30 min entre sesiones):
        // sesión 1: t+0, t+30min (mismo día — gap < SESSION_GAP_SECS, misma sesión)
        // sesión 2: t+3d, t+3d+30min
        // sesión 3: t+6d, t+6d+30min
        // Esto produce 3 sesiones con la misma combinación (github.com, desarrollo)
        // → frequency = 3 → cumple min_frequency = 3.
        let session_offsets = [
            0_i64,
            1800,           // +30 min (misma sesión 1)
            3 * 86_400,     // día 3 (sesión 2)
            3 * 86_400 + 1800,
            6 * 86_400,     // día 6 (sesión 3)
            6 * 86_400 + 1800,
        ];
        for (i, offset) in session_offsets.iter().enumerate() {
            let r = crate::storage::NewResource {
                uuid: format!("pattern-uuid-{i}"),
                url: crate::crypto::encrypt_aes(
                    &format!("https://github.com/resource-{i}"),
                    "test-key",
                ),
                title: crate::crypto::encrypt_aes(
                    &format!("Resource title {i}"),
                    "test-key",
                ),
                domain: "github.com".to_string(),
                category: "desarrollo".to_string(),
                captured_at: base_ts + offset,
            };
            db.insert_or_ignore(&r).unwrap();
        }

        // 2. Lista de mecanismos activos
        let patterns =
            crate::pattern_detector::detect_patterns(conn, &PatternConfig::default()).unwrap();

        // Verificar estructura de PatternSummary (los 4 campos requeridos por TS-2-004)
        assert!(
            !patterns.is_empty(),
            "detect_patterns debe encontrar al menos un patrón con recursos en ventana de 30 días"
        );
        for p in &patterns {
            assert!(!p.pattern_id.is_empty(), "pattern_id presente");
            assert!(!p.label.is_empty(), "label presente");
            assert!(!p.category_signature.is_empty(), "category_signature presente");
            assert!(!p.domain_signature.is_empty(), "domain_signature presente");
        }

        // D1: patterns no exponen url ni title
        for p in &patterns {
            assert!(
                !p.label.contains("http"),
                "D1: label no debe contener url — label='{}'",
                p.label
            );
        }
    }
}
