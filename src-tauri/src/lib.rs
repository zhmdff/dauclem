pub mod commands;
pub mod db;
pub mod indexer;
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
                // Determine database path
                let local_data = PathBuf::from("data").join("index.db");
                let db_path = if local_data.exists() {
                    local_data
                } else if let Ok(app_dir) = app.path().app_data_dir() {
                    std::fs::create_dir_all(&app_dir).ok();
                    app_dir.join("index.db")
                } else {
                    std::fs::create_dir_all("data").ok();
                    local_data
                };

                // Open shared connection for IPC queries
                let conn = db::open(&db_path).expect("Failed to open SQLite database");

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
                            if let Err(e) = indexer::index_all(&root_bg, &db_bg, &status_bg) {
                                eprintln!("Background indexer error: {:?}", e);
                            }
                            status_bg.indexing.store(false, Ordering::SeqCst);
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
            commands::session_detail
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
