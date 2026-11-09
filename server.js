'use strict';
const http = require('http');
const fs = require('fs');
const os = require('os');
const path = require('path');
const zlib = require('zlib');
const { Worker } = require('worker_threads');
const { open } = require('./src/db');
const { parseFile } = require('./src/parse');

const ROOT = process.env.CLAUDE_PROJECTS || path.join(os.homedir(), '.claude', 'projects');
const DBFILE = path.join(__dirname, 'data', 'index.db');
const PORT = Number(process.env.PORT) || 4173;
const PUB = path.join(__dirname, 'public');

const db = open(DBFILE);
const q = {
  projects: db.prepare('SELECT project, MIN(cwd) AS cwd, COUNT(*) AS n, MAX(last_ts) AS last FROM sessions GROUP BY project ORDER BY last DESC'),
  sessions: db.prepare('SELECT id, project, title, cwd, first_prompt, first_ts, last_ts, n_me, n_agent, n_act, n_sub, n_agents FROM sessions ORDER BY last_ts DESC'),
  sessionsP: db.prepare('SELECT id, project, title, cwd, first_prompt, first_ts, last_ts, n_me, n_agent, n_act, n_sub, n_agents FROM sessions WHERE project=? ORDER BY last_ts DESC'),
  session: db.prepare('SELECT * FROM sessions WHERE id=?'),
};

// ---- indexing worker ----
const status = { indexing: false, done: 0, total: 0, error: null, version: 0 };
function runIndex() {
  if (status.indexing) return;
  status.indexing = true; status.done = 0; status.total = 0; status.error = null;
  const w = new Worker(path.join(__dirname, 'src', 'worker.js'), { workerData: { root: ROOT, db: DBFILE } });
  w.on('message', m => {
    if (m.type === 'progress') { status.done = m.done; status.total = m.total; status.version++; }
    else if (m.type === 'done') { if (m.changed) status.version++; }
    else if (m.type === 'error') { status.error = m.message; console.error(m.message); }
  });
  w.on('error', e => { status.error = String(e); console.error(e); });
  w.on('exit', () => { status.indexing = false; status.version++; });
}
runIndex();
setInterval(runIndex, 20000).unref();

// ---- helpers ----
function send(req, res, code, body, type) {
  const buf = Buffer.isBuffer(body) ? body : Buffer.from(body);
  const h = { 'Content-Type': type, 'Cache-Control': 'no-store' };
  if (buf.length > 1024 && /gzip/.test(req.headers['accept-encoding'] || '')) {
    h['Content-Encoding'] = 'gzip';
    res.writeHead(code, h);
    res.end(zlib.gzipSync(buf, { level: 3 }));
  } else {
    res.writeHead(code, h);
    res.end(buf);
  }
}
const json = (req, res, obj, code = 200) => send(req, res, code, JSON.stringify(obj), 'application/json; charset=utf-8');

function ftsQuery(s) {
  const terms = [];
  const re = /"([^"]+)"|(\S+)/g;
  let m;
  while ((m = re.exec(s))) {
    const t = (m[1] || m[2]).replace(/"/g, '').trim();
    if (t.length >= 3) terms.push('"' + t + '"');
  }
  return terms.join(' ');
}

const CATS = new Set(['me', 'agent', 'act', 'sub']);

function search(p) {
  const match = ftsQuery(p.get('q') || '');
  if (!match) return { hits: [], error: 'Type at least 3 characters' };
  const cats = (p.get('cats') || 'me,agent,act,sub').split(',').filter(c => CATS.has(c));
  if (!cats.length) return { hits: [] };
  const project = p.get('project') || '';
  const limit = Math.min(Number(p.get('limit')) || 60, 200);
  const offset = Number(p.get('offset')) || 0;
  const order = p.get('sort') === 'date' ? 'e.ts DESC' : 'rank';
  const args = [match, ...cats];
  let sql = `SELECT e.session, e.cat, e.kind, e.tool, e.agent, e.ts, e.seq,
      snippet(fts, 0, char(1), char(2), '…', 40) AS snip, s.title, s.project, s.cwd
    FROM fts JOIN events e ON e.id = fts.rowid JOIN sessions s ON s.id = e.session
    WHERE fts MATCH ? AND e.cat IN (${cats.map(() => '?').join(',')})`;
  if (project) { sql += ' AND e.project = ?'; args.push(project); }
  sql += ` ORDER BY ${order} LIMIT ? OFFSET ?`;
  args.push(limit, offset);
  const t0 = process.hrtime.bigint();
  const hits = db.prepare(sql).all(...args);
  const ms = Number(process.hrtime.bigint() - t0) / 1e6;
  return { hits, ms: Math.round(ms), more: hits.length === limit };
}

function trunc(s, n) { return s.length > n ? s.slice(0, n) : s; }

function view(file, isSub) {
  const { items } = parseFile(fs.readFileSync(file, 'utf8'), isSub);
  const out = [];
  const byTool = new Map();
  for (const it of items) {
    if (it.kind === 'result') {
      const t = byTool.get(it.toolUseId);
      if (t) {
        t.result = trunc(it.text, 30000);
        t.resultTruncated = it.text.length > 30000;
        t.resultSeq = it.seq;
        if (it.isError) t.isError = true;
        if (it.agentId) t.agentId = it.agentId;
        continue;
      }
      it.text = trunc(it.text, 30000);
      out.push(it);
      continue;
    }
    if (it.kind === 'tool') {
      byTool.set(it.toolUseId, it);
      if (it.input && typeof it.input === 'object') {
        const inp = {};
        for (const [k, v] of Object.entries(it.input)) inp[k] = typeof v === 'string' ? trunc(v, 20000) : v;
        it.input = inp;
      }
    } else {
      it.text = trunc(it.text, 60000);
    }
    out.push(it);
  }
  return out;
}

function api(req, res, url) {
  const p = url.searchParams;
  const parts = url.pathname.split('/').filter(Boolean); // api, ...
  switch (parts[1]) {
    case 'status': return json(req, res, status);
    case 'projects': return json(req, res, q.projects.all());
    case 'sessions': return json(req, res, p.get('project') ? q.sessionsP.all(p.get('project')) : q.sessions.all());
    case 'search': return json(req, res, search(p));
    case 'session': {
      const s = q.session.get(parts[2] || '');
      if (!s) return json(req, res, { error: 'not found' }, 404);
      const agent = p.get('agent');
      if (agent) {
        if (!/^[\w-]+$/.test(agent)) return json(req, res, { error: 'bad agent' }, 400);
        const f = path.join(path.dirname(s.file), s.id, 'subagents', 'agent-' + agent + '.jsonl');
        return json(req, res, { items: view(f, true) });
      }
      const items = view(s.file, false);
      let agents = [];
      try {
        agents = fs.readdirSync(path.join(path.dirname(s.file), s.id, 'subagents'))
          .filter(f => f.endsWith('.jsonl')).map(f => f.replace(/^agent-/, '').slice(0, -6));
      } catch { /* none */ }
      return json(req, res, { session: s, items, agents });
    }
    default: return json(req, res, { error: 'unknown' }, 404);
  }
}

const MIME = { '.html': 'text/html; charset=utf-8', '.js': 'text/javascript; charset=utf-8', '.css': 'text/css; charset=utf-8', '.svg': 'image/svg+xml' };

http.createServer((req, res) => {
  try {
    const url = new URL(req.url, 'http://x');
    if (url.pathname.startsWith('/api/')) return api(req, res, url);
    let f = url.pathname === '/' ? '/index.html' : url.pathname;
    f = path.normalize(path.join(PUB, f));
    if (!f.startsWith(PUB) || !fs.existsSync(f) || !fs.statSync(f).isFile()) return send(req, res, 404, 'not found', 'text/plain');
    return send(req, res, 200, fs.readFileSync(f), MIME[path.extname(f)] || 'application/octet-stream');
  } catch (e) {
    console.error(e);
    json(req, res, { error: String(e.message) }, 500);
  }
}).listen(PORT, '127.0.0.1', () => console.log(`dauclem: http://localhost:${PORT}  (reading ${ROOT})`));
