// FS Watcher: detecta eventos de archivo en sesión activa.
// Distinto de pattern_detector.rs (patrones longitudinales) y de
// episode_detector.rs (episodios de sesión activa sin estado) — R12.
// Opera solo mientras la app está en primer plano (D9).
// Solo registra nombre del archivo (cifrado D1), directorio padre
// (en claro), extensión (en claro) y timestamp. Nunca lee contenido
// (D1 — prohibición permanente).
//
// | Dimensión       | fs_watcher.rs       | pattern_detector.rs   | episode_detector.rs   |
// |-----------------|---------------------|-----------------------|-----------------------|
// | Función         | Eventos archivo     | Patrones longitudinales| Episodio de sesión   |
// | Escala temporal | Tiempo real (sesión)| Días/semanas          | Sesión activa         |
// | Input           | inotify/RDCW/FSE    | SQLCipher resources   | Stream de captures    |
// | Output          | FsWatcherEvent      | DetectedPattern       | Episode (memoria)     |
// | Persistencia    | Solo configuración  | Patrones detectados   | Sin estado            |
// | Foreground-only | Sí (D9 absoluto)    | No aplica             | No aplica             |
// | Decisión clave  | D9                  | D17                   | (heredado Fase 1)     |

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::{Arc, Mutex};

#[cfg(not(target_os = "android"))]
use std::path::PathBuf;

/// Lista blanca de extensiones (TS-2-000 §1 "Extensiones de archivo en scope").
/// Cualquier extensión no listada se ignora silenciosamente.
/// Total: 17 entradas únicas (TS-2-000 lista png en dos grupos —
/// "Imágenes" y "Capturas de pantalla" — pero comparten bucket).
pub(crate) const ALLOWED_EXTENSIONS: &[&str] = &[
    // Documentos
    "pdf", "docx", "doc", "txt", "md", "xlsx", "csv",
    // Imágenes (incluye capturas de pantalla en Desktop/Downloads)
    "png", "jpg", "jpeg", "gif", "webp", "svg",
    // Video
    "mp4", "mov", "webm",
    // Archivos comprimidos
    "zip",
];

/// Directorios candidatos (TS-2-000 §1 "Directorios observables").
/// Ningún directorio se activa por defecto — el usuario los activa
/// individualmente desde el Privacy Dashboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub enum CandidateDirectory {
    Downloads,
    Desktop,
}

impl CandidateDirectory {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            CandidateDirectory::Downloads => "Downloads",
            CandidateDirectory::Desktop => "Desktop",
        }
    }

    pub(crate) fn from_str(s: &str) -> Option<Self> {
        match s {
            "Downloads" => Some(CandidateDirectory::Downloads),
            "Desktop" => Some(CandidateDirectory::Desktop),
            _ => None,
        }
    }

    /// Resuelve la ruta absoluta del directorio candidato sin observar nada.
    /// Devuelve `None` si la plataforma no expone el directorio o si la
    /// resolución falla (e.g. usuario sin home).
    #[cfg(not(target_os = "android"))]
    pub(crate) fn resolve_path(&self) -> Option<PathBuf> {
        match self {
            CandidateDirectory::Downloads => dirs::download_dir(),
            CandidateDirectory::Desktop => dirs::desktop_dir(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FsWatcherDirectory {
    pub directory: CandidateDirectory,
    pub absolute_path: String,
    pub active: bool,
    /// Unix seconds. `None` si nunca se activó (preserva auditoría histórica
    /// si el usuario reactiva tras desactivar).
    pub activated_at: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FsWatcherRuntimeState {
    /// App en primer plano + al menos un directorio activo + watcher corriendo.
    Active,
    /// App en background O sin directorios activos.
    Suspended,
    /// Plataforma no soporta FS Watcher (Android — D19).
    Unsupported,
}

#[derive(Debug, Clone, Serialize)]
pub struct FsWatcherEvent {
    pub event_id: String,
    pub directory: CandidateDirectory,
    /// AES-GCM (D1). Nunca un `String` legible. Excluido del shape
    /// TypeScript expuesto al frontend.
    pub file_name_encrypted: Vec<u8>,
    pub extension: String,
    pub detected_at: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct FsWatcherStatus {
    pub runtime_state: FsWatcherRuntimeState,
    pub directories: Vec<FsWatcherDirectory>,
    pub events_in_current_session: usize,
    pub events_last_24h: usize,
}

#[derive(Debug)]
#[allow(dead_code)] // UnsupportedPlatform/AppInBackground se usan bajo cfg Android
pub enum FsWatcherError {
    UnsupportedPlatform,
    DirectoryResolutionFailed(CandidateDirectory),
    AppInBackground,
    NoActiveDirectories,
    Persistence(rusqlite::Error),
    #[cfg(not(target_os = "android"))]
    NotifyBackend(notify::Error),
}

impl std::fmt::Display for FsWatcherError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FsWatcherError::UnsupportedPlatform => write!(
                f,
                "FS Watcher no soportado en Android — track móvil cubre observación por share intent"
            ),
            FsWatcherError::DirectoryResolutionFailed(d) => {
                write!(f, "no se pudo resolver el directorio {}", d.as_str())
            }
            FsWatcherError::AppInBackground => {
                write!(f, "FS Watcher solo opera con la app en primer plano (D9)")
            }
            FsWatcherError::NoActiveDirectories => {
                write!(f, "no hay directorios activos para observar")
            }
            FsWatcherError::Persistence(e) => write!(f, "persistencia: {e}"),
            #[cfg(not(target_os = "android"))]
            FsWatcherError::NotifyBackend(e) => write!(f, "backend notify: {e}"),
        }
    }
}

impl std::error::Error for FsWatcherError {}

impl From<rusqlite::Error> for FsWatcherError {
    fn from(e: rusqlite::Error) -> Self {
        FsWatcherError::Persistence(e)
    }
}

#[cfg(not(target_os = "android"))]
impl From<notify::Error> for FsWatcherError {
    fn from(e: notify::Error) -> Self {
        FsWatcherError::NotifyBackend(e)
    }
}

// ── Persistencia mínima de configuración ────────────────────────────────────
//
// Los **eventos NO se persisten** entre sesiones (TS-2-000 §2). El runtime
// buffer vive en memoria (FsWatcherState.event_buffer) y se purga al perder
// el foco. Solo la configuración de directorios opt-in se persiste.

/// Crea la tabla `fs_watcher_config` si no existe e inicializa con dos filas
/// inactivas (Downloads, Desktop). Idempotente.
///
/// Schema literal:
/// ```sql
/// CREATE TABLE IF NOT EXISTS fs_watcher_config (
///     directory     TEXT PRIMARY KEY CHECK (directory IN ('Downloads', 'Desktop')),
///     active        INTEGER NOT NULL DEFAULT 0 CHECK (active IN (0, 1)),
///     activated_at  INTEGER,
///     updated_at    INTEGER NOT NULL
/// );
/// ```
pub(crate) fn ensure_schema(conn: &Connection, now_unix: i64) -> Result<(), FsWatcherError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS fs_watcher_config (
            directory     TEXT PRIMARY KEY CHECK (directory IN ('Downloads', 'Desktop')),
            active        INTEGER NOT NULL DEFAULT 0 CHECK (active IN (0, 1)),
            activated_at  INTEGER,
            updated_at    INTEGER NOT NULL
        );",
    )?;
    // Inicialización idempotente: dos filas inactivas si la tabla está vacía
    // (TS-2-000 §1 "Sin activación por defecto").
    conn.execute(
        "INSERT OR IGNORE INTO fs_watcher_config (directory, active, activated_at, updated_at)
         VALUES ('Downloads', 0, NULL, ?1), ('Desktop', 0, NULL, ?1)",
        rusqlite::params![now_unix],
    )?;
    Ok(())
}

/// Lista la configuración actual de los dos directorios candidatos.
/// El `absolute_path` se resuelve desde `dirs::download_dir()` /
/// `dirs::desktop_dir()` — directorio padre, nunca archivo individual (D1).
pub(crate) fn list_directories(
    conn: &Connection,
) -> Result<Vec<FsWatcherDirectory>, FsWatcherError> {
    let mut stmt = conn.prepare(
        "SELECT directory, active, activated_at FROM fs_watcher_config
         ORDER BY directory ASC",
    )?;
    let rows = stmt.query_map([], |r| {
        let dir_str: String = r.get(0)?;
        let active: i64 = r.get(1)?;
        let activated_at: Option<i64> = r.get(2)?;
        Ok((dir_str, active != 0, activated_at))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (dir_str, active, activated_at) = row?;
        let directory = match CandidateDirectory::from_str(&dir_str) {
            Some(d) => d,
            None => continue,
        };
        let absolute_path = resolve_absolute_path(directory);
        out.push(FsWatcherDirectory {
            directory,
            absolute_path,
            active,
            activated_at,
        });
    }
    Ok(out)
}

#[cfg(not(target_os = "android"))]
fn resolve_absolute_path(directory: CandidateDirectory) -> String {
    directory
        .resolve_path()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default()
}

#[cfg(target_os = "android")]
fn resolve_absolute_path(_directory: CandidateDirectory) -> String {
    String::new()
}

/// Activa un directorio. Idempotente: si ya está activo, NO modifica
/// `activated_at` (preserva el momento original de consentimiento).
pub(crate) fn activate(
    conn: &Connection,
    directory: CandidateDirectory,
    now_unix: i64,
) -> Result<(), FsWatcherError> {
    conn.execute(
        "UPDATE fs_watcher_config
         SET active = 1,
             activated_at = COALESCE(activated_at, ?1),
             updated_at = ?1
         WHERE directory = ?2 AND active = 0",
        rusqlite::params![now_unix, directory.as_str()],
    )?;
    // No-op si ya estaba activo (idempotencia explícita).
    Ok(())
}

/// Desactiva un directorio. Preserva `activated_at` para auditoría
/// histórica si el usuario reactiva más tarde.
pub(crate) fn deactivate(
    conn: &Connection,
    directory: CandidateDirectory,
    now_unix: i64,
) -> Result<(), FsWatcherError> {
    conn.execute(
        "UPDATE fs_watcher_config
         SET active = 0,
             updated_at = ?1
         WHERE directory = ?2",
        rusqlite::params![now_unix, directory.as_str()],
    )?;
    Ok(())
}

// ── Filtros (lista blanca de extensiones + lista negra de directorios) ──────

/// Devuelve `true` si la extensión de `path` está en la lista blanca.
pub(crate) fn is_extension_allowed(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .map(|e| ALLOWED_EXTENSIONS.contains(&e.as_str()))
        .unwrap_or(false)
}

/// Verifica que la ruta NO toca un path prohibido (sistema, red, ocultos,
/// otras apps). Defensivo: aunque solo se exponen Downloads/Desktop, esta
/// función blinda contra cualquier escalada de scope futura (TS-2-000
/// "Riesgos De Interpretación" #1).
pub(crate) fn is_directory_allowed(path: &Path) -> bool {
    let s = path.to_string_lossy().to_lowercase();
    let forbidden = [
        "/system/",
        "c:\\windows\\",
        "/users/shared/",
        "/.git/",
        "\\.git\\",
        "/.ssh/",
        "\\.ssh\\",
        "/dropbox/",
        "\\dropbox\\",
        "/onedrive/",
        "\\onedrive\\",
        "/icloud drive/",
        "/google drive/",
        "\\google drive\\",
    ];
    !forbidden.iter().any(|f| s.contains(f))
}

// ── Watcher (notify v6) ─────────────────────────────────────────────────────

#[cfg(not(target_os = "android"))]
pub struct FsWatcherHandle {
    /// El watcher concreto. `Drop` automático detiene el backend (RAII) —
    /// cubre D9 sin paths alternativos.
    _watcher: notify::RecommendedWatcher,
}

#[cfg(target_os = "android")]
pub struct FsWatcherHandle {
    _phantom: std::marker::PhantomData<()>,
}

/// Inicia el watcher de archivos para los directorios actualmente activos.
/// Solo se invoca desde el hook `WindowEvent::Focused(true)` registrado en
/// `lib.rs` (D9 — única vía de entrada). Devuelve un handle cuyo `Drop`
/// detiene el watcher subyacente (RAII).
#[cfg(not(target_os = "android"))]
pub fn start_watching(
    conn: &Connection,
    event_buffer: Arc<Mutex<Vec<FsWatcherEvent>>>,
    crypto_key: &[u8; 32],
) -> Result<FsWatcherHandle, FsWatcherError> {
    use notify::{recommended_watcher, EventKind, RecursiveMode, Watcher};

    let directories = list_directories(conn)?;
    let active: Vec<FsWatcherDirectory> =
        directories.into_iter().filter(|d| d.active).collect();
    if active.is_empty() {
        return Err(FsWatcherError::NoActiveDirectories);
    }

    // Mapeo path → CandidateDirectory para enrutar eventos. El path es el
    // **directorio padre** (nunca el archivo individual — D1).
    let mut mappings: Vec<(PathBuf, CandidateDirectory)> = Vec::with_capacity(active.len());
    for d in &active {
        let resolved = d
            .directory
            .resolve_path()
            .ok_or(FsWatcherError::DirectoryResolutionFailed(d.directory))?;
        mappings.push((resolved, d.directory));
    }

    let key_owned: [u8; 32] = *crypto_key;
    let buffer_handle = event_buffer.clone();
    let mappings_for_closure = mappings.clone();

    let mut watcher = recommended_watcher(move |res: notify::Result<notify::Event>| {
        let event = match res {
            Ok(e) => e,
            Err(e) => { eprintln!("[fs_watcher] notify error: {e}"); return; }
        };
        eprintln!("[fs_watcher] event: kind={:?} paths={:?}", event.kind, event.paths);
        // Solo nos interesan eventos de creación o renombre destino —
        // notify entrega un Event con paths. NO leemos contenido (D1).
        let is_relevant = matches!(
            event.kind,
            EventKind::Create(_) | EventKind::Modify(notify::event::ModifyKind::Name(_))
        );
        if !is_relevant {
            eprintln!("[fs_watcher] skipped (kind not relevant)");
            return;
        }
        for path in &event.paths {
            eprintln!("[fs_watcher] checking path: {:?}", path);
            if !is_directory_allowed(path) {
                eprintln!("[fs_watcher] filtered: directory not allowed");
                continue;
            }
            if !is_extension_allowed(path) {
                eprintln!("[fs_watcher] filtered: extension not allowed ({:?})", path.extension());
                continue;
            }
            let routed_dir: Option<CandidateDirectory> = mappings_for_closure
                .iter()
                .find(|(parent, _)| {
                    // Windows filesystem is case-insensitive but Path::starts_with is not.
                    #[cfg(windows)]
                    {
                        path.to_string_lossy().to_lowercase()
                            .starts_with(parent.to_string_lossy().to_lowercase().as_str())
                    }
                    #[cfg(not(windows))]
                    { path.starts_with(parent) }
                })
                .map(|(_, d)| *d);
            eprintln!("[fs_watcher] routed_dir={:?} (mappings={:?})", routed_dir, mappings_for_closure.iter().map(|(p,_)| p).collect::<Vec<_>>());
            let directory = match routed_dir {
                Some(d) => d,
                None => continue,
            };
            let file_name = match path.file_name().and_then(|n| n.to_str()) {
                Some(n) => n.to_string(),
                None => continue,
            };
            let extension = match path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.to_ascii_lowercase())
            {
                Some(e) => e,
                None => continue,
            };
            let detected_at = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            let evt = FsWatcherEvent {
                event_id: uuid::Uuid::new_v4().to_string(),
                directory,
                file_name_encrypted: encrypt_filename(&file_name, &key_owned),
                extension,
                detected_at,
            };
            if let Ok(mut buf) = buffer_handle.lock() {
                buf.push(evt);
            }
        }
    })?;

    for (path, _) in &mappings {
        eprintln!("[fs_watcher] watching path: {:?} (exists={})", path, path.exists());
        watcher.watch(path, RecursiveMode::NonRecursive)?;
    }

    Ok(FsWatcherHandle { _watcher: watcher })
}

#[cfg(target_os = "android")]
pub fn start_watching(
    _conn: &Connection,
    _event_buffer: Arc<Mutex<Vec<FsWatcherEvent>>>,
    _crypto_key: &[u8; 32],
) -> Result<FsWatcherHandle, FsWatcherError> {
    Err(FsWatcherError::UnsupportedPlatform)
}

/// AES-256-GCM in-place — devuelve `Vec<u8>` (nunca String legible).
/// Layout: 12-byte nonce || ciphertext+tag. Coherente con `crypto::encrypt_aes`
/// pero sin prefijo de magic (los eventos no se persisten — TS-2-000 §2).
#[cfg(not(target_os = "android"))]
fn encrypt_filename(name: &str, key: &[u8; 32]) -> Vec<u8> {
    use aes_gcm::{
        aead::{Aead, AeadCore, KeyInit, OsRng},
        Aes256Gcm,
    };
    let cipher = Aes256Gcm::new(key.into());
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let ct = cipher
        .encrypt(&nonce, name.as_bytes())
        .expect("AES-256-GCM encrypt failed");
    let mut out = Vec::with_capacity(12 + ct.len());
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ct);
    out
}

/// Deriva la clave de 32 bytes desde la passphrase. Coherente con
/// `crypto::derive_key_aes` (no se reusa para evitar exponer ese helper como
/// API pública). SHA-256 de la passphrase → 32 bytes.
pub fn derive_filename_key(passphrase: &str) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(passphrase.as_bytes());
    h.finalize().into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn open_mem() -> Connection {
        Connection::open_in_memory().expect("open in-memory failed")
    }

    /// Test #1 — TS-2-000 §1 "Extensiones de archivo en scope".
    /// La lista hardcoded en este test detecta drift entre código y spec.
    #[test]
    fn test_extension_whitelist_exact_set() {
        let expected: std::collections::HashSet<&str> = [
            "pdf", "docx", "doc", "txt", "md", "xlsx", "csv", "png", "jpg", "jpeg", "gif",
            "webp", "svg", "mp4", "mov", "webm", "zip",
        ]
        .into_iter()
        .collect();
        let actual: std::collections::HashSet<&str> =
            ALLOWED_EXTENSIONS.iter().copied().collect();
        assert_eq!(actual.len(), 17, "deben ser exactamente 17 extensiones únicas");
        assert_eq!(actual, expected, "lista blanca no coincide con TS-2-000 §1");
    }

    /// Test #2 — TS-2-000 §1 "Extensiones explícitamente fuera de scope".
    #[test]
    fn test_extension_filter_rejects_executables() {
        let forbidden = [
            // Ejecutables
            "exe", "app", "dmg", "msi", "sh", "bat",
            // Archivos de sistema
            "dll", "sys", "plist", "dylib",
            // Archivos de código
            "py", "js", "rs", "swift", "java",
            // Credenciales
            "pem", "key", "p12", "env",
        ];
        for ext in forbidden {
            let path_str = format!("/tmp/file.{ext}");
            let path = Path::new(&path_str);
            assert!(
                !is_extension_allowed(path),
                "extensión '{ext}' NO debe pasar el filtro"
            );
        }
        // Sanity: una extensión permitida sí pasa.
        assert!(is_extension_allowed(Path::new("/tmp/x.pdf")));
        assert!(is_extension_allowed(Path::new("/tmp/x.PDF")));
    }

    /// Test #3 — TS-2-000 §1 "Directorios prohibidos".
    #[test]
    fn test_directory_filter_rejects_forbidden() {
        let forbidden = [
            "/System/Library/file.pdf",
            "C:\\Windows\\system32\\note.txt",
            "/Users/Shared/file.pdf",
            "/home/u/.git/config.md",
            "C:\\Users\\u\\.git\\HEAD.md",
            "/home/u/.ssh/key.txt",
            "/Users/u/Dropbox/notes.txt",
            "C:\\Users\\u\\OneDrive\\doc.docx",
            "/Users/u/iCloud Drive/file.pdf",
            "C:\\Users\\u\\Google Drive\\doc.docx",
        ];
        for p in forbidden {
            assert!(
                !is_directory_allowed(Path::new(p)),
                "ruta '{p}' debe ser rechazada"
            );
        }
        // Sanity: rutas permitidas pasan.
        assert!(is_directory_allowed(Path::new("/Users/u/Downloads/x.pdf")));
        assert!(is_directory_allowed(Path::new("C:\\Users\\u\\Desktop\\x.pdf")));
    }

    /// Test #4 — round-trip activate / deactivate preservando `activated_at`.
    #[test]
    fn test_activate_deactivate_round_trip() {
        let conn = open_mem();
        ensure_schema(&conn, 1000).expect("ensure_schema");
        let dirs = list_directories(&conn).expect("list");
        assert_eq!(dirs.len(), 2, "deben existir Downloads y Desktop");
        assert!(dirs.iter().all(|d| !d.active), "ningún dir activo por defecto");

        activate(&conn, CandidateDirectory::Downloads, 1500).expect("activate");
        let dirs = list_directories(&conn).expect("list");
        let downloads = dirs
            .iter()
            .find(|d| d.directory == CandidateDirectory::Downloads)
            .unwrap();
        assert!(downloads.active);
        assert_eq!(downloads.activated_at, Some(1500));

        deactivate(&conn, CandidateDirectory::Downloads, 2000).expect("deactivate");
        let dirs = list_directories(&conn).expect("list");
        let downloads = dirs
            .iter()
            .find(|d| d.directory == CandidateDirectory::Downloads)
            .unwrap();
        assert!(!downloads.active);
        // Preservación de auditoría histórica.
        assert_eq!(downloads.activated_at, Some(1500));
    }

    /// Test #5 — `activate` idempotente: no modifica `activated_at` si ya está activo.
    #[test]
    fn test_activate_idempotent() {
        let conn = open_mem();
        ensure_schema(&conn, 1000).expect("ensure_schema");
        activate(&conn, CandidateDirectory::Desktop, 1500).expect("activate 1");
        activate(&conn, CandidateDirectory::Desktop, 9999).expect("activate 2");
        let dirs = list_directories(&conn).expect("list");
        let desktop = dirs
            .iter()
            .find(|d| d.directory == CandidateDirectory::Desktop)
            .unwrap();
        assert!(desktop.active);
        assert_eq!(
            desktop.activated_at,
            Some(1500),
            "second activate must not bump activated_at"
        );
    }

    /// Test #6 — eventos NO persisten entre sesiones (D9 transitivo).
    /// Verifica que `fs_watcher_config` tiene exactamente 4 columnas.
    #[test]
    fn test_no_events_persisted_across_sessions() {
        let conn = open_mem();
        ensure_schema(&conn, 1000).expect("ensure_schema");
        let mut stmt = conn
            .prepare("PRAGMA table_info(fs_watcher_config)")
            .expect("pragma");
        let cols: Vec<String> = stmt
            .query_map([], |r| r.get::<_, String>(1))
            .expect("query")
            .collect::<Result<Vec<_>, _>>()
            .expect("collect");
        assert_eq!(
            cols.len(),
            4,
            "fs_watcher_config debe tener 4 columnas, no {} ({:?})",
            cols.len(),
            cols
        );
        let expected: std::collections::HashSet<&str> =
            ["directory", "active", "activated_at", "updated_at"]
                .into_iter()
                .collect();
        let actual: std::collections::HashSet<&str> = cols.iter().map(|s| s.as_str()).collect();
        assert_eq!(actual, expected, "columnas deben ser exactamente las declaradas");
    }

    /// Test #7 — D1 estructural: ningún token prohibido en el módulo.
    /// Auditable por `include_str!` + grep negativo.
    #[test]
    fn test_no_url_or_title_in_event_struct() {
        let src = include_str!("./fs_watcher.rs");
        // Excluir el bloque de tests del análisis (puede contener literales
        // como urls de ejemplo en el futuro). El módulo tiene UN solo
        // bloque `#[cfg(test)]` — coherente con state_machine.rs.
        let prod_src = src
            .split("#[cfg(test)]")
            .next()
            .expect("debe haber código antes del bloque de tests");
        let forbidden = [
            "url:",
            "title:",
            "link:",
            "href:",
            "page_title:",
            "bookmark_url:",
            "full_path:",
            "content:",
            "body:",
        ];
        for token in forbidden {
            assert!(
                !prod_src.contains(token),
                "D1 violation: token '{token}' presente en fs_watcher.rs (sección de producción)"
            );
        }
    }

    /// Test #8 — R12 estructural: sin imports de pattern_detector ni
    /// episode_detector, y comentario de cabecera obligatorio presente.
    #[test]
    fn test_no_pattern_detector_or_episode_detector_imports() {
        let src = include_str!("./fs_watcher.rs");
        let prod_src = src
            .split("#[cfg(test)]")
            .next()
            .expect("debe haber código antes del bloque de tests");
        assert!(
            prod_src.contains("Distinto de pattern_detector.rs (patrones longitudinales) y de"),
            "comentario de cabecera R12 ausente"
        );
        assert!(
            !prod_src.contains("use crate::pattern_detector"),
            "import prohibido: use crate::pattern_detector"
        );
        assert!(
            !prod_src.contains("use crate::episode_detector"),
            "import prohibido: use crate::episode_detector"
        );
    }
}
