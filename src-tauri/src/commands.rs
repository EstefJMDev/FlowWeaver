use serde::{Deserialize, Serialize};
use tauri::{Manager, State};
use uuid::Uuid;

use crate::{
    classifier,
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
    trust_scorer::{self, TrustConfig},
};

pub struct DbState(pub std::sync::Mutex<Db>);

/// Estado runtime del FS Watcher (T-2-000). El `handle` mantiene vivo el
/// watcher de `notify` mientras la app está en primer plano (D9). El
/// `event_buffer` acumula eventos de la sesión actual y se purga al perder
/// el foco. Registrado vía `.manage(FsWatcherState::default())` en `lib.rs`.
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

        // Si ya hay un handle vivo (app en foreground), reiniciamos el watcher
        // para incluir el nuevo directorio. Si no hay handle (app suspendida),
        // se lanzará al próximo Focused(true) — D9.
        let mut handle_guard = fs_state.handle.lock().map_err(|e| e.to_string())?;
        if handle_guard.is_some() {
            *handle_guard = None; // RAII drop del watcher anterior
            let key = derive_fs_key(&app);
            match fs_watcher::start_watching(conn, fs_state.event_buffer.clone(), &key) {
                Ok(h) => *handle_guard = Some(h),
                Err(fs_watcher::FsWatcherError::NoActiveDirectories) => { /* nada que observar */ }
                Err(e) => return Err(e.to_string()),
            }
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

        // Reinicio del watcher: si quedan directorios activos, observa el
        // resto. Si fue el último, drop del handle (sin tocar buffer hasta
        // perder el foco — TS-2-000 §3 "Desactivación inmediata").
        let mut handle_guard = fs_state.handle.lock().map_err(|e| e.to_string())?;
        if handle_guard.is_some() {
            *handle_guard = None;
            let key = derive_fs_key(&app);
            match fs_watcher::start_watching(conn, fs_state.event_buffer.clone(), &key) {
                Ok(h) => *handle_guard = Some(h),
                Err(fs_watcher::FsWatcherError::NoActiveDirectories) => { /* OK: era el último */ }
                Err(e) => return Err(e.to_string()),
            }
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
}
