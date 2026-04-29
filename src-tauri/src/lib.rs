mod classifier;
mod commands;
pub mod crypto;
mod episode_detector;
mod fs_watcher;
mod grouper;
mod importer;
mod pattern_blocks;
mod pattern_detector;
pub mod raw_event;
mod session_builder;
mod state_machine;
pub mod storage;
mod trust_scorer;

#[cfg(not(target_os = "android"))]
pub mod drive_relay;

use commands::{DbState, FsWatcherState};
use storage::Db;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let db = setup_db();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(DbState(std::sync::Mutex::new(db)))
        .manage(FsWatcherState::default())
        .on_window_event(|window, event| {
            // Arranca el watcher la primera vez que la ventana gana foco.
            // No se detiene al perder el foco: corre en segundo plano mientras
            // haya directorios activos (D9 revisado — background-persistent).
            #[cfg(not(target_os = "android"))]
            if let tauri::WindowEvent::Focused(focused) = event {
                if *focused {
                    use tauri::Manager;
                    let fs_state = window.state::<FsWatcherState>();
                    let mut guard = match fs_state.handle.lock() {
                        Ok(g) => g,
                        Err(_) => return,
                    };
                    if guard.is_none() {
                        let db_state = window.state::<DbState>();
                        let db = match db_state.0.lock() {
                            Ok(d) => d,
                            Err(_) => return,
                        };
                        let conn = db.conn();
                        let now_unix = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_secs() as i64)
                            .unwrap_or(0);
                        if fs_watcher::ensure_schema(conn, now_unix).is_err() {
                            return;
                        }
                        let app_handle = window.app_handle();
                        let app_data_dir = match app_handle.path().app_data_dir() {
                            Ok(p) => p,
                            Err(_) => return,
                        };
                        let passphrase = format!("fw-{}", app_data_dir.to_string_lossy());
                        let key = fs_watcher::derive_filename_key(&passphrase);
                        match fs_watcher::start_watching(
                            conn,
                            fs_state.event_buffer.clone(),
                            &key,
                        ) {
                            Ok(h) => *guard = Some(h),
                            Err(fs_watcher::FsWatcherError::NoActiveDirectories) => {}
                            Err(e) => eprintln!("[fs_watcher] start_watching error: {e}"),
                        }
                    }
                }
            }
            #[cfg(target_os = "android")]
            {
                let _ = (window, event); // Android: stub, sin watcher
            }
        })
        .setup(|app| {
            // Desktop relay background loop — not needed on Android (WorkManager handles it).
            #[cfg(not(target_os = "android"))]
            {
                use std::time::Duration;
                use tauri::Manager;

                let handle = app.handle().clone();
                let app_data_dir = handle
                    .path()
                    .app_data_dir()
                    .expect("cannot resolve app_data_dir");
                let config_path = drive_relay::config_path(&app_data_dir);
                let local_key = format!("fw-{}", app_data_dir.to_string_lossy());

                tauri::async_runtime::spawn(async move {
                    loop {
                        let state = handle.state::<DbState>();
                        if let Err(e) = drive_relay::run_relay_cycle(
                            &config_path,
                            &state.0,
                            &local_key,
                            Some(&handle),
                        )
                        .await
                        {
                            if e != "Drive not configured" {
                                eprintln!("[relay] cycle error: {e}");
                            }
                        }
                        tokio::time::sleep(Duration::from_secs(30)).await;
                    }
                });
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::import_resource,
            commands::import_bookmarks,
            commands::import_bookmarks_html,
            commands::set_resource_category,
            commands::get_resources,
            commands::get_clusters,
            commands::resource_count,
            commands::get_sessions,
            commands::get_episodes,
            commands::add_capture,
            commands::get_privacy_stats,
            commands::clear_all_resources,
            commands::fs_watcher_get_status,
            commands::fs_watcher_list_directories,
            commands::fs_watcher_activate_directory,
            commands::fs_watcher_deactivate_directory,
            commands::fs_watcher_get_session_events,
            commands::fs_watcher_clear_directory_history,
            commands::fs_watcher_get_24h_event_count,
            commands::get_trust_state,
            commands::reset_trust_state,
            commands::enable_autonomous_mode,
            commands::get_detected_patterns,
            commands::block_pattern,
            commands::unblock_pattern,
            commands::get_mobile_resources,
            commands::get_platform,
            commands::open_resource_url,
            commands::configure_drive,
            commands::get_relay_device_id,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn setup_db() -> Db {
    // On Android, temp_dir() resolves to the app's cache dir
    // (/data/data/{pkg}/cache). SQLiteOpenHelper stores databases one level up
    // at /data/data/{pkg}/databases/ — we derive that same path so Kotlin and
    // Rust share a single SQLite file.
    #[cfg(target_os = "android")]
    let data_dir = std::env::temp_dir()    // /data/data/{pkg}/cache
        .parent()
        .map(|p| p.join("databases"))
        .unwrap_or_else(|| std::env::temp_dir().join("flowweaver"));

    #[cfg(not(target_os = "android"))]
    let data_dir = dirs_next::data_local_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("flowweaver");

    std::fs::create_dir_all(&data_dir).expect("cannot create data dir");
    let db_path = data_dir.join("resources.db");
    let key = format!("fw-{}", data_dir.to_string_lossy());
    let db = Db::open(&db_path, &key).expect("cannot open SQLCipher database");
    db.migrate().expect("migration failed");
    db
}
