// commands.rs — Tauri IPC commands matching server.js API

use crate::indexer::IndexerStatus;
use crate::parser::view_items;
use rusqlite::{params, Connection};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Instant;

pub struct AppState {
    pub db_path: PathBuf,
    pub root_dir: PathBuf,
    pub status: Arc<IndexerStatus>,
    pub db: Mutex<Connection>,
}

#[derive(Serialize)]
pub struct StatusDto {
    pub indexing: bool,
    pub done: usize,
    pub total: usize,
    pub error: Option<String>,
    pub version: u64,
}

#[tauri::command]
pub fn status(state: tauri::State<AppState>) -> StatusDto {
    StatusDto {
        indexing: state.status.indexing.load(Ordering::Relaxed),
        done: state.status.done.load(Ordering::Relaxed),
        total: state.status.total.load(Ordering::Relaxed),
        error: None,
        version: state.status.version.load(Ordering::Relaxed),
    }
}

#[derive(Serialize)]
pub struct ProjectDto {
    pub project: String,
    pub cwd: Option<String>,
    pub n: i64,
    pub last: Option<i64>,
}

#[tauri::command]
pub fn projects(state: tauri::State<AppState>) -> Result<Vec<ProjectDto>, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare("SELECT project, MIN(cwd) AS cwd, COUNT(*) AS n, MAX(last_ts) AS last FROM sessions GROUP BY project ORDER BY last DESC")
        .map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map([], |row| {
            Ok(ProjectDto {
                project: row.get(0)?,
                cwd: row.get(1)?,
                n: row.get(2)?,
                last: row.get(3)?,
            })
        })
        .map_err(|e| e.to_string())?;

    let mut out = Vec::new();
    for r in rows.flatten() {
        out.push(r);
    }
    Ok(out)
}

#[derive(Serialize)]
pub struct SessionSummaryDto {
    pub id: String,
    pub project: String,
    pub title: String,
    pub cwd: Option<String>,
    pub first_prompt: Option<String>,
    pub first_ts: Option<i64>,
    pub last_ts: Option<i64>,
    pub n_me: i64,
    pub n_agent: i64,
    pub n_act: i64,
    pub n_sub: i64,
    pub n_agents: i64,
}

#[tauri::command]
pub fn sessions(
    project: Option<String>,
    state: tauri::State<AppState>,
) -> Result<Vec<SessionSummaryDto>, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let proj = project.unwrap_or_default();

    let sql = if !proj.is_empty() {
        "SELECT id, project, title, cwd, first_prompt, first_ts, last_ts, n_me, n_agent, n_act, n_sub, n_agents FROM sessions WHERE project=? ORDER BY last_ts DESC"
    } else {
        "SELECT id, project, title, cwd, first_prompt, first_ts, last_ts, n_me, n_agent, n_act, n_sub, n_agents FROM sessions ORDER BY last_ts DESC"
    };

    let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;

    let map_row = |row: &rusqlite::Row| {
        Ok(SessionSummaryDto {
            id: row.get(0)?,
            project: row.get(1)?,
            title: row.get(2)?,
            cwd: row.get(3)?,
            first_prompt: row.get(4)?,
            first_ts: row.get(5)?,
            last_ts: row.get(6)?,
            n_me: row.get(7)?,
            n_agent: row.get(8)?,
            n_act: row.get(9)?,
            n_sub: row.get(10)?,
            n_agents: row.get(11)?,
        })
    };

    let mut out = Vec::new();
    if !proj.is_empty() {
        let rows = stmt
            .query_map(params![proj], map_row)
            .map_err(|e| e.to_string())?;
        for r in rows.flatten() {
            out.push(r);
        }
    } else {
        let rows = stmt.query_map([], map_row).map_err(|e| e.to_string())?;
        for r in rows.flatten() {
            out.push(r);
        }
    }

    Ok(out)
}

fn fts_query(s: &str) -> String {
    let mut terms = Vec::new();
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_whitespace() {
            i += 1;
            continue;
        }
        if chars[i] == '"' {
            i += 1;
            let start = i;
            while i < chars.len() && chars[i] != '"' {
                i += 1;
            }
            let quoted: String = chars[start..i].iter().collect();
            let clean = quoted.trim();
            if clean.chars().count() >= 3 {
                terms.push(format!("\"{}\"", clean));
            }
            if i < chars.len() && chars[i] == '"' {
                i += 1;
            }
        } else {
            let start = i;
            while i < chars.len() && !chars[i].is_whitespace() && chars[i] != '"' {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            let clean = word.trim();
            if clean.chars().count() >= 3 {
                terms.push(format!("\"{}\"", clean));
            }
        }
    }
    terms.join(" ")
}

#[derive(Serialize)]
pub struct SearchHitDto {
    pub session: String,
    pub cat: String,
    pub kind: String,
    pub tool: Option<String>,
    pub agent: Option<String>,
    pub ts: i64,
    pub seq: usize,
    pub snip: String,
    pub title: String,
    pub project: String,
    pub cwd: Option<String>,
}

#[derive(Serialize)]
pub struct SearchResponseDto {
    pub hits: Vec<SearchHitDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub more: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[tauri::command]
pub fn search(
    q: String,
    cats: Option<String>,
    project: Option<String>,
    sort: Option<String>,
    limit: Option<usize>,
    offset: Option<usize>,
    state: tauri::State<AppState>,
) -> SearchResponseDto {
    let match_query = fts_query(&q);
    if match_query.is_empty() {
        return SearchResponseDto {
            hits: Vec::new(),
            ms: None,
            more: None,
            error: Some("Type at least 3 characters".to_string()),
        };
    }

    let raw_cats = cats.unwrap_or_else(|| "me,agent,act,sub".to_string());
    let valid_cats = ["me", "agent", "act", "sub"];
    let selected_cats: Vec<String> = raw_cats
        .split(',')
        .filter(|c| valid_cats.contains(c))
        .map(str::to_string)
        .collect();

    if selected_cats.is_empty() {
        return SearchResponseDto {
            hits: Vec::new(),
            ms: None,
            more: None,
            error: None,
        };
    }

    let proj = project.unwrap_or_default();
    let lim = limit.unwrap_or(60).min(200);
    let off = offset.unwrap_or(0);
    let order = if sort.as_deref() == Some("date") {
        "e.ts DESC"
    } else {
        "rank"
    };

    let cat_placeholders = selected_cats.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let mut sql = format!(
        "SELECT e.session, e.cat, e.kind, e.tool, e.agent, e.ts, e.seq,
                snippet(fts, 0, char(1), char(2), '…', 40) AS snip, s.title, s.project, s.cwd
         FROM fts JOIN events e ON e.id = fts.rowid JOIN sessions s ON s.id = e.session
         WHERE fts MATCH ? AND e.cat IN ({})",
        cat_placeholders
    );

    if !proj.is_empty() {
        sql.push_str(" AND e.project = ?");
    }
    sql.push_str(&format!(" ORDER BY {} LIMIT ? OFFSET ?", order));

    let t0 = Instant::now();
    let conn = match state.db.lock() {
        Ok(c) => c,
        Err(e) => {
            return SearchResponseDto {
                hits: Vec::new(),
                ms: None,
                more: None,
                error: Some(e.to_string()),
            };
        }
    };

    let mut stmt = match conn.prepare(&sql) {
        Ok(s) => s,
        Err(e) => {
            return SearchResponseDto {
                hits: Vec::new(),
                ms: None,
                more: None,
                error: Some(e.to_string()),
            };
        }
    };

    let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    params_vec.push(Box::new(match_query));
    for c in &selected_cats {
        params_vec.push(Box::new(c.clone()));
    }
    if !proj.is_empty() {
        params_vec.push(Box::new(proj));
    }
    params_vec.push(Box::new(lim as i64));
    params_vec.push(Box::new(off as i64));

    let slice_params: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|b| b.as_ref()).collect();

    let rows_res = stmt.query_map(slice_params.as_slice(), |row| {
        Ok(SearchHitDto {
            session: row.get(0)?,
            cat: row.get(1)?,
            kind: row.get(2)?,
            tool: row.get(3)?,
            agent: row.get(4)?,
            ts: row.get(5)?,
            seq: row.get::<_, i64>(6)? as usize,
            snip: row.get(7)?,
            title: row.get(8)?,
            project: row.get(9)?,
            cwd: row.get(10)?,
        })
    });

    let mut hits = Vec::new();
    if let Ok(mapped) = rows_res {
        for r in mapped.flatten() {
            hits.push(r);
        }
    }

    let elapsed_ms = t0.elapsed().as_millis() as u64;
    let more = hits.len() == lim;

    SearchResponseDto {
        hits,
        ms: Some(elapsed_ms),
        more: Some(more),
        error: None,
    }
}

#[derive(Serialize)]
pub struct SessionDetailDto {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session: Option<SessionSummaryDto>,
    pub items: Vec<crate::parser::ParsedItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agents: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[tauri::command]
pub fn session_detail(
    id: String,
    agent: Option<String>,
    state: tauri::State<AppState>,
) -> SessionDetailDto {
    let conn = match state.db.lock() {
        Ok(c) => c,
        Err(e) => {
            return SessionDetailDto {
                session: None,
                items: Vec::new(),
                agents: None,
                error: Some(e.to_string()),
            };
        }
    };

    // Look up session row
    let sess_res = conn.query_row(
        "SELECT id, project, file, title, cwd, first_prompt, first_ts, last_ts, n_me, n_agent, n_act, n_sub, n_agents FROM sessions WHERE id=?",
        params![id],
        |row| {
            let file_str: String = row.get(2)?;
            Ok((
                SessionSummaryDto {
                    id: row.get(0)?,
                    project: row.get(1)?,
                    title: row.get(3)?,
                    cwd: row.get(4)?,
                    first_prompt: row.get(5)?,
                    first_ts: row.get(6)?,
                    last_ts: row.get(7)?,
                    n_me: row.get(8)?,
                    n_agent: row.get(9)?,
                    n_act: row.get(10)?,
                    n_sub: row.get(11)?,
                    n_agents: row.get(12)?,
                },
                file_str,
            ))
        },
    );

    let (session_dto, file_path_str) = match sess_res {
        Ok(val) => val,
        Err(_) => {
            return SessionDetailDto {
                session: None,
                items: Vec::new(),
                agents: None,
                error: Some("not found".to_string()),
            };
        }
    };

    let session_file = PathBuf::from(file_path_str);
    let session_dir = session_file.parent().unwrap_or_else(|| Path::new(""));

    // If subagent requested
    if let Some(ag) = agent {
        if !ag.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_') {
            return SessionDetailDto {
                session: None,
                items: Vec::new(),
                agents: None,
                error: Some("bad agent".to_string()),
            };
        }
        let sub_file = session_dir
            .join(&id)
            .join("subagents")
            .join(format!("agent-{}.jsonl", ag));

        let raw = match fs::read_to_string(&sub_file) {
            Ok(r) => r,
            Err(e) => {
                return SessionDetailDto {
                    session: None,
                    items: Vec::new(),
                    agents: None,
                    error: Some(format!("failed to read subagent file: {}", e)),
                };
            }
        };
        let items = view_items(&raw, true);
        return SessionDetailDto {
            session: None,
            items,
            agents: None,
            error: None,
        };
    }

    // Main session view
    let raw = match fs::read_to_string(&session_file) {
        Ok(r) => r,
        Err(e) => {
            return SessionDetailDto {
                session: None,
                items: Vec::new(),
                agents: None,
                error: Some(format!("failed to read session file: {}", e)),
            };
        }
    };
    let items = view_items(&raw, false);

    // List subagents
    let mut agents = Vec::new();
    let sub_dir = session_dir.join(&id).join("subagents");
    if let Ok(read) = fs::read_dir(&sub_dir) {
        for entry in read.flatten() {
            let fname = entry.file_name().to_string_lossy().to_string();
            if fname.starts_with("agent-") && fname.ends_with(".jsonl") {
                let ag_name = fname
                    .trim_start_matches("agent-")
                    .trim_end_matches(".jsonl")
                    .to_string();
                agents.push(ag_name);
            }
        }
    }

    SessionDetailDto {
        session: Some(session_dto),
        items,
        agents: Some(agents),
        error: None,
    }
}
