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
        .on_page_load(|w, payload| {
            logf!("page_load {:?} url={}", payload.event(), payload.url());
            if matches!(payload.event(), tauri::webview::PageLoadEvent::Finished) {
                let js = r#"
(async () => {
  const L = m => { try { window.__TAURI_INTERNALS__.invoke('log_frontend', { msg: 'diag: ' + m }); } catch (e) {} };
  const opts = () => { const p = document.querySelector('#proj'); return p ? p.options.length : -1; };
  const safe = async (name, fn) => { try { await fn(); } catch (e) { L(name + ' THREW: ' + e + ' | ' + (e && e.stack)); } };
  window.addEventListener('error', e => L('window error: ' + e.message + ' @' + e.filename + ':' + e.lineno + ':' + e.colno));
  window.addEventListener('unhandledrejection', e => L('unhandled rejection: ' + (e.reason && e.reason.stack || e.reason)));
  window.addEventListener('securitypolicyviolation', e => L('CSP violation: ' + e.violatedDirective + ' ' + e.blockedURI));

  await safe('env', () => L('UA=' + navigator.userAgent + ' readyState=' + document.readyState + ' href=' + location.href + ' __TAURI__=' + typeof window.__TAURI__ + ' INTERNALS=' + typeof window.__TAURI_INTERNALS__ + ' #proj options=' + opts()));
  await safe('scripts', () => { for (const s of document.scripts) L('script tag src=' + s.src + ' type=' + JSON.stringify(s.type) + ' async=' + s.async + ' defer=' + s.defer + ' noModule=' + s.noModule); });
  await safe('perf', () => { for (const e of performance.getEntriesByType('resource')) L('resource ' + e.name + ' type=' + e.initiatorType + ' status=' + e.responseStatus + ' size=' + e.transferSize + ' dur=' + Math.round(e.duration)); });
  await safe('inline-exec', () => { const s = document.createElement('script'); s.textContent = 'window.__inl = 1'; document.body.appendChild(s); L('inline script executed=' + (window.__inl === 1)); });

  let t = '';
  await safe('fetch', async () => {
    const r = await fetch('/app.js'); t = await r.text();
    L('fetch /app.js status=' + r.status + ' len=' + t.length + ' content-type=' + r.headers.get('content-type') + ' nosniff=' + r.headers.get('x-content-type-options') + ' csp=' + r.headers.get('content-security-policy') + ' hasUlog=' + t.includes('ulog') + ' hasTinvoke=' + t.includes('tinvoke'));
    L('fetch / (index.html) csp meta=' + (document.querySelector('meta[http-equiv]') ? document.querySelector('meta[http-equiv]').outerHTML : 'none'));
  });
  await safe('compile', () => { try { new Function(t); L('app.js compiles OK'); } catch (e) { L('app.js COMPILE ERROR: ' + e + ' | ' + (e && e.stack)); } });

  await safe('ipc-status', async () => L('invoke status -> ' + JSON.stringify(await window.__TAURI_INTERNALS__.invoke('status'))));
  await safe('ipc-projects', async () => { const p = await window.__TAURI_INTERNALS__.invoke('projects'); L('invoke projects -> ' + (Array.isArray(p) ? p.length + ' first=' + JSON.stringify(p[0]) : typeof p)); });
  await safe('ipc-sessions', async () => { const p = await window.__TAURI_INTERNALS__.invoke('sessions', { project: null }); L('invoke sessions -> ' + (Array.isArray(p) ? p.length : typeof p)); });

  L('window.isTauri=' + typeof window.isTauri + ' desc=' + JSON.stringify(Object.getOwnPropertyDescriptor(window, 'isTauri')));
  setTimeout(() => L('after 3s: loadProjects=' + typeof window.loadProjects + ' poll=' + typeof window.poll + ' options=' + opts() + ' listChildren=' + (document.querySelector('#list') ? document.querySelector('#list').children.length : -1) + ' mainText=' + JSON.stringify((document.querySelector('#main') || {}).textContent)), 3000);
  setTimeout(() => L('after 10s: loadProjects=' + typeof window.loadProjects + ' options=' + opts()), 10000);
})();
"#;
                let _ = w.eval(js);
            }
        })
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
