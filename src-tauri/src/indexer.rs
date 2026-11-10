// indexer.rs — Port of src/indexer.js
// Scans ~/.claude/projects and populates the SQLite database.

use crate::parser::{parse_file, ParsedItem};
use rusqlite::{params, Connection, Result};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::SystemTime;

const MAX_ME: usize = 20000;
const MAX_AGENT: usize = 20000;
const MAX_SUB: usize = 8000;
const MAX_TOOL: usize = 3000;
const MAX_RESULT: usize = 4000;

#[derive(Debug, Clone)]
pub struct SubInfo {
    pub agent: String,
    pub file: PathBuf,
}

#[derive(Debug, Clone)]
pub struct SessionCandidate {
    pub project: String,
    pub id: String,
    pub file: PathBuf,
    pub subs: Vec<SubInfo>,
    pub sig: String,
    pub mtime_ms: u128,
}

fn sig_of(path: &Path) -> (String, u128) {
    if let Ok(meta) = fs::metadata(path) {
        let size = meta.len();
        let mtime_ms = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map(|d| d.as_millis())
            .unwrap_or(0);
        (format!("{}:{}", size, mtime_ms), mtime_ms)
    } else {
        (String::new(), 0)
    }
}

pub fn scan(root: &Path) -> Vec<SessionCandidate> {
    let mut out = Vec::new();
    let dirs = match fs::read_dir(root) {
        Ok(read) => read
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
            .collect::<Vec<_>>(),
        Err(_) => return out,
    };

    for d in dirs {
        let project_name = d.file_name().to_string_lossy().to_string();
        let dir_path = d.path();

        let ents = match fs::read_dir(&dir_path) {
            Ok(read) => read.filter_map(|e| e.ok()).collect::<Vec<_>>(),
            Err(_) => continue,
        };

        for e in ents {
            let fname = e.file_name().to_string_lossy().to_string();
            if !fname.ends_with(".jsonl") {
                continue;
            }
            let is_file = e.file_type().map(|t| t.is_file()).unwrap_or(false);
            if !is_file {
                continue;
            }

            let id = fname.trim_end_matches(".jsonl").to_string();
            let file_path = e.path();
            let (mut sig, mtime) = sig_of(&file_path);

            let mut subs = Vec::new();
            let sub_dir = dir_path.join(&id).join("subagents");
            if let Ok(sub_read) = fs::read_dir(&sub_dir) {
                for s in sub_read.filter_map(|x| x.ok()) {
                    let sname = s.file_name().to_string_lossy().to_string();
                    if !sname.ends_with(".jsonl") {
                        continue;
                    }
                    let sub_path = s.path();
                    let agent = sname
                        .trim_start_matches("agent-")
                        .trim_end_matches(".jsonl")
                        .to_string();
                    let (sub_sig, _) = sig_of(&sub_path);
                    sig.push('|');
                    sig.push_str(&sub_sig);
                    subs.push(SubInfo {
                        agent,
                        file: sub_path,
                    });
                }
            }

            out.push(SessionCandidate {
                project: project_name.clone(),
                id,
                file: file_path,
                subs,
                sig,
                mtime_ms: mtime,
            });
        }
    }

    out
}

struct EventRow {
    cat: String,
    kind: String,
    tool: Option<String>,
    agent: Option<String>,
    ts: i64,
    seq: usize,
    text: String,
}

fn clamp_slice(s: &str, max: usize) -> &str {
    if s.len() <= max {
        s
    } else {
        match s.char_indices().nth(max) {
            Some((idx, _)) => &s[..idx],
            None => s,
        }
    }
}

struct EventCollector {
    n_me: i64,
    n_agent: i64,
    n_act: i64,
    n_sub: i64,
    first_prompt: Option<String>,
    tool_name_map: HashMap<String, String>,
    rows: Vec<EventRow>,
}

impl EventCollector {
    fn new() -> Self {
        Self {
            n_me: 0,
            n_agent: 0,
            n_act: 0,
            n_sub: 0,
            first_prompt: None,
            tool_name_map: HashMap::new(),
            rows: Vec::new(),
        }
    }

    fn add(&mut self, it: &ParsedItem, agent: Option<&str>) {
        let (text, tool): (String, Option<String>) = if it.kind == "tool" {
            let tname = it.tool.clone().unwrap_or_default();
            if let Some(ref tid) = it.tool_use_id {
                self.tool_name_map.insert(tid.clone(), tname.clone());
            }
            let inp_str = it
                .input
                .as_ref()
                .map(|v| serde_json::to_string(v).unwrap_or_default())
                .unwrap_or_default();
            let combined = format!("{} {}", tname, inp_str);
            (clamp_slice(&combined, MAX_TOOL).to_string(), Some(tname))
        } else if it.kind == "result" {
            let tname = it
                .tool_use_id
                .as_ref()
                .and_then(|tid| self.tool_name_map.get(tid))
                .cloned();
            (clamp_slice(&it.text, MAX_RESULT).to_string(), tname)
        } else if it.kind == "thinking" {
            return;
        } else {
            let limit = match it.cat.as_str() {
                "sub" => MAX_SUB,
                "agent" => MAX_AGENT,
                _ => MAX_ME,
            };
            (clamp_slice(&it.text, limit).to_string(), None)
        };

        if text.trim().is_empty() {
            return;
        }

        match it.cat.as_str() {
            "me" => self.n_me += 1,
            "agent" => self.n_agent += 1,
            "act" => self.n_act += 1,
            "sub" => self.n_sub += 1,
            _ => {}
        }

        if self.first_prompt.is_none() && it.cat == "me" {
            self.first_prompt = Some(text.clone());
        }

        self.rows.push(EventRow {
            cat: it.cat.clone(),
            kind: it.kind.clone(),
            tool,
            agent: agent.map(str::to_string),
            ts: it.ts,
            seq: it.seq,
            text,
        });
    }

    fn clear_tools(&mut self) {
        self.tool_name_map.clear();
    }
}

fn index_session(conn: &mut Connection, s: &SessionCandidate) -> Result<bool> {
    let raw = match fs::read_to_string(&s.file) {
        Ok(t) => t,
        Err(_) => return Ok(false),
    };

    let (items, meta) = parse_file(&raw, false);
    let mut collector = EventCollector::new();

    for it in &items {
        collector.add(it, None);
    }

    for sub in &s.subs {
        if let Ok(raw_sub) = fs::read_to_string(&sub.file) {
            let (sub_items, _) = parse_file(&raw_sub, true);
            collector.clear_tools();
            for it in &sub_items {
                collector.add(it, Some(&sub.agent));
            }
        }
    }

    if collector.rows.is_empty() {
        return Ok(false);
    }

    let title = meta
        .custom
        .or(meta.title)
        .or_else(|| {
            collector.first_prompt.as_ref().map(|p| {
                let single_line = p.split_whitespace().collect::<Vec<_>>().join(" ");
                clamp_slice(&single_line, 90).to_string()
            })
        })
        .unwrap_or_else(|| "(untitled)".to_string());

    let prompt_snippet = collector
        .first_prompt
        .as_deref()
        .map(|p| clamp_slice(p, 300))
        .unwrap_or("");

    let tx = conn.transaction()?;

    // Delete old events
    tx.execute("DELETE FROM events WHERE session=?", params![&s.id])?;

    // Insert new events
    {
        let mut ins = tx.prepare_cached(
            "INSERT INTO events(session,project,cat,kind,tool,agent,ts,seq,text) VALUES(?,?,?,?,?,?,?,?,?)"
        )?;
        for r in &collector.rows {
            ins.execute(params![
                &s.id,
                &s.project,
                &r.cat,
                &r.kind,
                &r.tool,
                &r.agent,
                r.ts,
                r.seq as i64,
                &r.text
            ])?;
        }
    }

    // Insert or replace session
    tx.execute(
        "INSERT OR REPLACE INTO sessions(id,project,file,title,cwd,first_prompt,first_ts,last_ts,sig,n_me,n_agent,n_act,n_sub,n_agents)
         VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
        params![
            &s.id,
            &s.project,
            s.file.to_string_lossy(),
            title,
            meta.cwd,
            prompt_snippet,
            meta.first,
            meta.last,
            &s.sig,
            collector.n_me,
            collector.n_agent,
            collector.n_act,
            collector.n_sub,
            s.subs.len() as i64
        ],
    )?;

    tx.commit()?;
    Ok(true)
}

#[derive(Default)]
pub struct IndexerStatus {
    pub indexing: AtomicBool,
    pub done: AtomicUsize,
    pub total: AtomicUsize,
    pub version: AtomicU64,
}

pub fn index_all(
    root: &Path,
    db_path: &Path,
    status: &Arc<IndexerStatus>,
) -> Result<usize> {
    let mut conn = crate::db::open(db_path)?;

    // Read known signatures
    let mut known_sigs: HashMap<String, String> = HashMap::new();
    {
        let mut stmt = conn.prepare("SELECT id, sig FROM sessions")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        for r in rows.flatten() {
            known_sigs.insert(r.0, r.1);
        }
    }

    let found = scan(root);
    let alive_ids: HashSet<String> = found.iter().map(|s| s.id.clone()).collect();
    let mut todo: Vec<SessionCandidate> = found
        .into_iter()
        .filter(|s| match known_sigs.get(&s.id) {
            Some(existing_sig) => existing_sig != &s.sig,
            None => true,
        })
        .collect();

    // Vanished sessions cleanup
    for (id, _) in &known_sigs {
        if !alive_ids.contains(id) {
            conn.execute("DELETE FROM events WHERE session=?", params![id])?;
            conn.execute("DELETE FROM sessions WHERE id=?", params![id])?;
        }
    }

    // Sort newest first
    todo.sort_by(|a, b| b.mtime_ms.cmp(&a.mtime_ms));

    let total = todo.len();
    status.total.store(total, Ordering::Relaxed);
    status.done.store(0, Ordering::Relaxed);

    let mut done = 0;
    for s in &todo {
        if let Err(e) = index_session(&mut conn, s) {
            eprintln!("index fail {} {:?}", s.file.display(), e);
        }
        done += 1;
        status.done.store(done, Ordering::Relaxed);
        status.version.fetch_add(1, Ordering::Relaxed);
    }

    if total > 50 {
        let _ = conn.execute("INSERT INTO fts(fts) VALUES('optimize')", []);
    }

    Ok(total)
}
