mod classifier;
mod commands;
mod crypto;
mod episode_detector;
mod grouper;
mod importer;
mod raw_event;
mod session_builder;
mod storage;

#[cfg(not(target_os = "android"))]
mod drive_relay;

use commands::DbState;
use storage::Db;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let db = setup_db();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(DbState(std::sync::Mutex::new(db)))
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
