mod classifier;
mod commands;
mod crypto;
mod grouper;
mod importer;
mod storage;

use commands::DbState;
use storage::Db;

pub fn run() {
    let db = setup_db();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(DbState(std::sync::Mutex::new(db)))
        .invoke_handler(tauri::generate_handler![
            commands::import_resource,
            commands::import_bookmarks,
            commands::set_resource_category,
            commands::get_resources,
            commands::get_clusters,
            commands::resource_count,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn setup_db() -> Db {
    // Resolve the data directory at startup.
    // We can't use AppHandle here (before Builder::run), so we use a known path.
    let data_dir = dirs_next::data_local_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("flowweaver");
    std::fs::create_dir_all(&data_dir).expect("cannot create data dir");
    let db_path = data_dir.join("resources.db");

    // Key is derived from the data dir path — local, never transmitted (invariant 2).
    let key = format!("fw-{}", data_dir.to_string_lossy());
    let db = Db::open(&db_path, &key).expect("cannot open SQLCipher database");
    db.migrate().expect("migration failed");
    db
}
