pub mod commands;
pub mod db;
pub mod indexer;
pub mod logger;
pub mod parser;

use commands::AppState;
use indexer::IndexerStatus;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use tauri::Manager;

pub fn run() {
    let status = Arc::new(IndexerStatus::default());

    let root_dir = std::env::var("CLAUDE_PROJECTS")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".claude")
                .join("projects")
        });

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .setup({
            let status = Arc::clone(&status);
            let root_dir = root_dir.clone();
            move |app| {
                // Determine database path: prioritize standard app_data_dir for both dev and installed app
                let db_path = if let Ok(app_dir) = app.path().app_data_dir() {
                    std::fs::create_dir_all(&app_dir).ok();
                    let app_db = app_dir.join("index.db");
                    // If a local data/index.db exists and app_data_dir index.db does not, seed it from local
                    let local_data = PathBuf::from("data").join("index.db");
                    if !app_db.exists() && local_data.exists() {
                        let _ = std::fs::copy(&local_data, &app_db);
                    }
                    app_db
                } else {
                    std::fs::create_dir_all("data").ok();
                    PathBuf::from("data").join("index.db")
                };

                if let Ok(app_dir) = app.path().app_data_dir() {
                    logger::init(&app_dir.join("dauclem.log"));
                }
                logf!("startup v{} cwd={:?}", env!("CARGO_PKG_VERSION"), std::env::current_dir().ok());
                logf!("root_dir={} exists={}", root_dir.display(), root_dir.exists());
                logf!("db_path={} exists={}", db_path.display(), db_path.exists());

                // Open shared connection for IPC queries
                let conn = match db::open(&db_path) {
                    Ok(c) => c,
                    Err(e) => {
                        logf!("FATAL db::open failed: {:?}", e);
                        panic!("Failed to open SQLite database: {:?}", e);
                    }
                };

                app.manage(AppState {
                    db_path: db_path.clone(),
                    root_dir: root_dir.clone(),
                    status: Arc::clone(&status),
                    db: Mutex::new(conn),
                });

                // Spawn background indexing thread
                let status_bg = Arc::clone(&status);
                let root_bg = root_dir.clone();
                let db_bg = db_path.clone();

                std::thread::spawn(move || {
                    loop {
                        if !status_bg.indexing.swap(true, Ordering::SeqCst) {
                            match indexer::index_all(&root_bg, &db_bg, &status_bg) {
                                Ok(n) if n > 0 => logf!("index_all ok: {} sessions (re)indexed", n),
                                Ok(_) => {}
                                Err(e) => logf!("index_all ERROR: {:?}", e),
                            }
                            status_bg.indexing.store(false, Ordering::SeqCst);
                            status_bg.version.fetch_add(1, Ordering::SeqCst);
                        }
                        std::thread::sleep(std::time::Duration::from_secs(20));
                    }
                });

                Ok(())
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::status,
            commands::projects,
            commands::sessions,
            commands::search,
            commands::session_detail,
            commands::log_frontend
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
