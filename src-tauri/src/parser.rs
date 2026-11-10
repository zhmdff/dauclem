// parser.rs — Port of src/parse.js and server.js view()
// Normalises Claude Code JSONL transcripts into flat ParsedItem values.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

/// A single parsed event from a session transcript.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedItem {
    pub seq: usize,
    /// "me" | "agent" | "act" | "sub"
    pub cat: String,
    /// "user" | "assistant" | "thinking" | "tool" | "result"
    pub kind: String,
    pub ts: i64,
    pub text: String,
    // tool-specific fields
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "toolUseId")]
    pub tool_use_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "resultTruncated")]
    pub result_truncated: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "resultSeq")]
    pub result_seq: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "isError")]
    pub is_error: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "agentId")]
    pub agent_id: Option<String>,
}

/// Session-level metadata extracted from the transcript.
#[derive(Debug, Default, Clone)]
pub struct TranscriptMeta {
    pub title: Option<String>,
    pub custom: Option<String>,
    pub cwd: Option<String>,
    pub first: i64,
    pub last: i64,
}

static SKIP_PREFIXES: &[&str] = &[
    "<local-command",
    "<system-reminder",
    "<task-notification",
    "<bash-input",
    "<bash-stdout",
    "<bash-stderr",
    "<user-prompt",
    "<command-message",
    "<command-args",
];

fn clean(s: &str) -> Option<String> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    if t.starts_with("<command-name>") {
        let name = extract_tag(t, "command-name").unwrap_or_default();
        let args = extract_tag(t, "command-args").unwrap_or_default();
        let combined = format!("{} {}", name, args);
        let combined = combined.trim().to_string();
        return if combined.is_empty() { None } else { Some(combined) };
    }
    for prefix in SKIP_PREFIXES {
        if t.starts_with(prefix) {
            return None;
        }
    }
    Some(t.to_string())
}

fn extract_tag<'a>(s: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{}>", tag);
    let close = format!("</{}>", tag);
    let start = s.find(&open)? + open.len();
    let end = s.find(&close)?;
    Some(&s[start..end])
}

fn blocks_text(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(arr) => {
            let mut parts = Vec::new();
            for b in arr {
                if b.get("type").and_then(Value::as_str) == Some("text") {
                    if let Some(t) = b.get("text").and_then(Value::as_str) {
                        parts.push(t.to_string());
                    }
                } else if b.get("type").and_then(Value::as_str) == Some("image") {
                    parts.push("[image]".to_string());
                }
            }
            parts.join("\n")
        }
        _ => String::new(),
    }
}

pub fn parse_file(raw: &str, is_sub: bool) -> (Vec<ParsedItem>, TranscriptMeta) {
    let mut items: Vec<ParsedItem> = Vec::new();
    let mut meta = TranscriptMeta::default();
    let mut seen_uuids: HashSet<String> = HashSet::new();
    let mut seq: usize = 0;

    for line in raw.lines() {
        if line.is_empty() {
            continue;
        }
        let o: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let t = match o.get("type").and_then(Value::as_str) {
            Some(v) => v,
            None => continue,
        };

        if t == "ai-title" {
            meta.title = o.get("aiTitle").and_then(Value::as_str).map(str::to_string);
            continue;
        }
        if t == "custom-title" {
            meta.custom = o
                .get("customTitle")
                .and_then(Value::as_str)
                .map(str::to_string);
            continue;
        }
        if meta.cwd.is_none() {
            if let Some(cwd) = o.get("cwd").and_then(Value::as_str) {
                meta.cwd = Some(cwd.to_string());
            }
        }
        if t != "user" && t != "assistant" {
            continue;
        }
        if o.get("isMeta").and_then(Value::as_bool).unwrap_or(false) {
            continue;
        }
        if o.get("message").is_none() {
            continue;
        }
        if let Some(uuid) = o.get("uuid").and_then(Value::as_str) {
            if !seen_uuids.insert(uuid.to_string()) {
                continue;
            }
        }

        let ts = o
            .get("timestamp")
            .and_then(Value::as_str)
            .and_then(parse_iso_timestamp)
            .unwrap_or(0);

        if ts > 0 {
            if meta.first == 0 {
                meta.first = ts;
            }
            meta.last = ts;
        }

        let content = &o["message"]["content"];
        let base_cat = if is_sub { "sub" } else if t == "user" { "me" } else { "agent" };

        if t == "user" {
            match content {
                Value::String(s) => {
                    if let Some(cleaned) = clean(s) {
                        items.push(make_item(seq, base_cat, "user", ts, cleaned));
                        seq += 1;
                    }
                }
                Value::Array(blocks) => {
                    for b in blocks {
                        let btype = b.get("type").and_then(Value::as_str).unwrap_or("");
                        if btype == "text" {
                            if let Some(txt) = b.get("text").and_then(Value::as_str) {
                                if let Some(cleaned) = clean(txt) {
                                    items.push(make_item(seq, base_cat, "user", ts, cleaned));
                                    seq += 1;
                                }
                            }
                        } else if btype == "tool_result" {
                            let tool_use_id = b
                                .get("tool_use_id")
                                .and_then(Value::as_str)
                                .map(str::to_string);
                            let text = blocks_text(&b["content"]);
                            let is_error = b
                                .get("is_error")
                                .and_then(Value::as_bool)
                                .unwrap_or(false);
                            let agent_id = o
                                .get("toolUseResult")
                                .and_then(|r| r.get("agentId"))
                                .and_then(Value::as_str)
                                .map(str::to_string);
                            items.push(ParsedItem {
                                seq,
                                cat: "act".to_string(),
                                kind: "result".to_string(),
                                ts,
                                text,
                                tool: None,
                                tool_use_id,
                                input: None,
                                result: None,
                                result_truncated: None,
                                result_seq: None,
                                is_error: if is_error { Some(true) } else { None },
                                agent_id,
                            });
                            seq += 1;
                        }
                    }
                }
                _ => {}
            }
        } else {
            let blocks = match content.as_array() {
                Some(a) => a,
                None => continue,
            };
            for b in blocks {
                let btype = b.get("type").and_then(Value::as_str).unwrap_or("");
                match btype {
                    "text" => {
                        let txt = b.get("text").and_then(Value::as_str).unwrap_or("");
                        if !txt.trim().is_empty() {
                            items.push(make_item(seq, "agent", "assistant", ts, txt.to_string()));
                            seq += 1;
                        }
                    }
                    "thinking" => {
                        let txt = b.get("thinking").and_then(Value::as_str).unwrap_or("");
                        if !txt.trim().is_empty() {
                            items.push(make_item(seq, "agent", "thinking", ts, txt.to_string()));
                            seq += 1;
                        }
                    }
                    "tool_use" => {
                        let tool = b
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string();
                        let tool_use_id = b
                            .get("id")
                            .and_then(Value::as_str)
                            .map(str::to_string);
                        let input = b.get("input").cloned();
                        items.push(ParsedItem {
                            seq,
                            cat: "act".to_string(),
                            kind: "tool".to_string(),
                            ts,
                            text: String::new(),
                            tool: Some(tool),
                            tool_use_id,
                            input,
                            result: None,
                            result_truncated: None,
                            result_seq: None,
                            is_error: None,
                            agent_id: None,
                        });
                        seq += 1;
                    }
                    _ => {}
                }
            }
        }
    }

    (items, meta)
}

fn trunc(s: &str, n: usize) -> String {
    if s.len() > n {
        s.chars().take(n).collect()
    } else {
        s.to_string()
    }
}

/// Corresponds to server.js `view(file, isSub)`:
/// stitches tool results into tool objects, truncates values for UI display.
pub fn view_items(raw: &str, is_sub: bool) -> Vec<ParsedItem> {
    let (items, _) = parse_file(raw, is_sub);
    let mut out: Vec<ParsedItem> = Vec::new();
    let mut by_tool: HashMap<String, usize> = HashMap::new(); // toolUseId -> index in `out`

    for mut it in items {
        if it.kind == "result" {
            if let Some(ref tid) = it.tool_use_id {
                if let Some(&idx) = by_tool.get(tid) {
                    let t = &mut out[idx];
                    t.result = Some(trunc(&it.text, 30000));
                    t.result_truncated = Some(it.text.len() > 30000);
                    t.result_seq = Some(it.seq);
                    if it.is_error.unwrap_or(false) {
                        t.is_error = Some(true);
                    }
                    if it.agent_id.is_some() {
                        t.agent_id = it.agent_id;
                    }
                    continue;
                }
            }
            it.text = trunc(&it.text, 30000);
            out.push(it);
            continue;
        }

        if it.kind == "tool" {
            if let Some(ref input_val) = it.input {
                if let Value::Object(map) = input_val {
                    let mut new_map = serde_json::Map::new();
                    for (k, v) in map {
                        if let Value::String(s) = v {
                            new_map.insert(k.clone(), Value::String(trunc(s, 20000)));
                        } else {
                            new_map.insert(k.clone(), v.clone());
                        }
                    }
                    it.input = Some(Value::Object(new_map));
                }
            }
            let idx = out.len();
            if let Some(ref tid) = it.tool_use_id {
                by_tool.insert(tid.clone(), idx);
            }
            out.push(it);
        } else {
            it.text = trunc(&it.text, 60000);
            out.push(it);
        }
    }

    out
}

fn make_item(seq: usize, cat: &str, kind: &str, ts: i64, text: String) -> ParsedItem {
    ParsedItem {
        seq,
        cat: cat.to_string(),
        kind: kind.to_string(),
        ts,
        text,
        tool: None,
        tool_use_id: None,
        input: None,
        result: None,
        result_truncated: None,
        result_seq: None,
        is_error: None,
        agent_id: None,
    }
}

pub fn parse_iso_timestamp(s: &str) -> Option<i64> {
    let s = s.trim_end_matches('Z');
    let (date, time) = s.split_once('T')?;
    let mut dp = date.split('-');
    let year: i64 = dp.next()?.parse().ok()?;
    let month: i64 = dp.next()?.parse().ok()?;
    let day: i64 = dp.next()?.parse().ok()?;

    let (time_main, frac) = time.split_once('.').unwrap_or((time, "0"));
    let mut tp = time_main.split(':');
    let hour: i64 = tp.next()?.parse().ok()?;
    let minute: i64 = tp.next()?.parse().ok()?;
    let second: i64 = tp.next()?.parse().ok()?;
    let millis: i64 = {
        let f = &frac[..frac.len().min(3)];
        let ms: i64 = f.parse().ok()?;
        let pad = 3i32 - f.len() as i32;
        if pad > 0 { ms * 10i64.pow(pad as u32) } else { ms }
    };

    let days = days_since_epoch(year, month, day)?;
    Some(days * 86_400_000 + hour * 3_600_000 + minute * 60_000 + second * 1000 + millis)
}

fn days_since_epoch(year: i64, month: i64, day: i64) -> Option<i64> {
    let a = (14 - month) / 12;
    let y = year + 4800 - a;
    let m = month + 12 * a - 3;
    let jdn = day + (153 * m + 2) / 5 + 365 * y + y / 4 - y / 100 + y / 400 - 32045;
    Some(jdn - 2440588)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_iso_timestamp() {
        let ts = parse_iso_timestamp("2026-11-10T15:30:00.000Z");
        assert!(ts.is_some());
        assert!(ts.unwrap() > 0);
    }

    #[test]
    fn test_parse_jsonl_and_view() {
        let sample = r#"{"type":"ai-title","aiTitle":"Test Conversation"}
{"type":"user","message":{"content":"hello claude"},"timestamp":"2026-11-10T15:30:00.000Z","uuid":"u1"}
{"type":"assistant","message":{"content":[{"type":"text","text":"hello human"}]},"timestamp":"2026-11-10T15:30:01.000Z","uuid":"u2"}
{"type":"assistant","message":{"content":[{"type":"tool_use","id":"call_1","name":"Bash","input":{"command":"dir"}}]},"timestamp":"2026-11-10T15:30:02.000Z","uuid":"u3"}
{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"call_1","content":"file.txt"}]},"timestamp":"2026-11-10T15:30:03.000Z","uuid":"u4"}
"#;

        let (items, meta) = parse_file(sample, false);
        assert_eq!(meta.title.as_deref(), Some("Test Conversation"));
        assert_eq!(items.len(), 4); // user, assistant, tool, result

        let viewed = view_items(sample, false);
        assert_eq!(viewed.len(), 3); // tool and result stitched into 1 tool item
        assert_eq!(viewed[2].kind, "tool");
        assert_eq!(viewed[2].result.as_deref(), Some("file.txt"));
    }
}
