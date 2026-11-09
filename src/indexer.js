'use strict';
const fs = require('fs');
const path = require('path');
const { parseFile } = require('./parse');
const { open } = require('./db');

const MAX = { me: 20000, agent: 20000, sub: 8000, tool: 3000, result: 4000 };

function sigOf(st) { return st.size + ':' + Math.floor(st.mtimeMs); }

function scan(root) {
  const out = [];
  let dirs = [];
  try { dirs = fs.readdirSync(root, { withFileTypes: true }).filter(d => d.isDirectory()); } catch { return out; }
  for (const d of dirs) {
    const dir = path.join(root, d.name);
    let ents;
    try { ents = fs.readdirSync(dir, { withFileTypes: true }); } catch { continue; }
    for (const e of ents) {
      if (!e.isFile() || !e.name.endsWith('.jsonl')) continue;
      const id = e.name.slice(0, -6);
      const file = path.join(dir, e.name);
      let sig;
      try { sig = sigOf(fs.statSync(file)); } catch { continue; }
      const subs = [];
      const sd = path.join(dir, id, 'subagents');
      try {
        for (const s of fs.readdirSync(sd)) {
          if (!s.endsWith('.jsonl')) continue;
          const f = path.join(sd, s);
          subs.push({ agent: s.replace(/^agent-/, '').slice(0, -6), file: f });
          sig += '|' + sigOf(fs.statSync(f));
        }
      } catch { /* no subagents */ }
      out.push({ project: d.name, id, file, subs, sig });
    }
  }
  return out;
}

function indexSession(db, st, s) {
  const raw = fs.readFileSync(s.file, 'utf8');
  const { items, meta } = parseFile(raw, false);
  const counts = { me: 0, agent: 0, act: 0, sub: 0 };
  const toolName = new Map();
  let firstPrompt = null;
  const rows = [];
  const add = (it, agent) => {
    let text, tool = it.tool || null;
    if (it.kind === 'tool') {
      toolName.set(it.toolUseId, it.tool);
      let inp = '';
      try { inp = JSON.stringify(it.input); } catch { /* ignore */ }
      text = (it.tool + ' ' + inp).slice(0, MAX.tool);
    } else if (it.kind === 'result') {
      tool = toolName.get(it.toolUseId) || null;
      text = it.text.slice(0, MAX.result);
    } else if (it.kind === 'thinking') {
      return;
    } else {
      text = it.text.slice(0, it.cat === 'sub' ? MAX.sub : MAX.me);
    }
    if (!text) return;
    counts[it.cat]++;
    if (!firstPrompt && it.cat === 'me') firstPrompt = text;
    rows.push([s.id, s.project, it.cat, it.kind, tool, agent, it.ts, it.seq, text]);
  };
  for (const it of items) add(it, null);
  for (const sub of s.subs) {
    let r;
    try { r = parseFile(fs.readFileSync(sub.file, 'utf8'), true); } catch { continue; }
    toolName.clear();
    for (const it of r.items) add(it, sub.agent);
  }
  if (!rows.length) return false;
  db.exec('BEGIN');
  try {
    st.delEv.run(s.id);
    for (const r of rows) st.insEv.run(...r);
    const title = meta.custom || meta.title || (firstPrompt || '(untitled)').replace(/\s+/g, ' ').slice(0, 90);
    st.upSess.run(
      s.id, s.project, s.file, title, meta.cwd, (firstPrompt || '').slice(0, 300),
      meta.first, meta.last, s.sig, counts.me, counts.agent, counts.act, counts.sub, s.subs.length
    );
    db.exec('COMMIT');
  } catch (e) {
    db.exec('ROLLBACK');
    throw e;
  }
  return true;
}

function indexAll(root, dbFile, progress) {
  const db = open(dbFile);
  const st = {
    delEv: db.prepare('DELETE FROM events WHERE session=?'),
    insEv: db.prepare('INSERT INTO events(session,project,cat,kind,tool,agent,ts,seq,text) VALUES(?,?,?,?,?,?,?,?,?)'),
    upSess: db.prepare('INSERT OR REPLACE INTO sessions(id,project,file,title,cwd,first_prompt,first_ts,last_ts,sig,n_me,n_agent,n_act,n_sub,n_agents) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?)'),
  };
  const known = new Map(db.prepare('SELECT id, sig FROM sessions').all().map(r => [r.id, r.sig]));
  const found = scan(root);
  const todo = found.filter(s => known.get(s.id) !== s.sig);
  // vanished sessions
  const alive = new Set(found.map(s => s.id));
  for (const id of known.keys()) {
    if (!alive.has(id)) {
      st.delEv.run(id);
      db.prepare('DELETE FROM sessions WHERE id=?').run(id);
    }
  }
  // newest first so recent chats are searchable ASAP
  todo.sort((a, b) => (fs.statSync(b.file).mtimeMs - fs.statSync(a.file).mtimeMs));
  let done = 0;
  for (const s of todo) {
    try { indexSession(db, st, s); } catch (e) { console.error('index fail', s.file, e.message); }
    progress && progress(++done, todo.length);
  }
  if (todo.length > 50) {
    try { db.exec("INSERT INTO fts(fts) VALUES('optimize')"); } catch { /* ignore */ }
  }
  db.close();
  return todo.length;
}

module.exports = { indexAll };
