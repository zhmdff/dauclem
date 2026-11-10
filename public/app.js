'use strict';
const $ = s => document.querySelector(s);
const el = (tag, cls, html) => { const e = document.createElement(tag); if (cls) e.className = cls; if (html != null) e.innerHTML = html; return e; };
const esc = s => s.replace(/[&<>"]/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));
const isTauri = () => typeof window.__TAURI__ !== 'undefined' && !!window.__TAURI__.core;
const api = async (path, params) => {
  if (isTauri()) {
    const parts = path.split('/');
    const endpoint = parts[0];
    if (endpoint === 'status') {
      return await window.__TAURI__.core.invoke('status');
    }
    if (endpoint === 'projects') {
      return await window.__TAURI__.core.invoke('projects');
    }
    if (endpoint === 'sessions') {
      const project = params && params.project ? params.project : null;
      return await window.__TAURI__.core.invoke('sessions', { project });
    }
    if (endpoint === 'search') {
      return await window.__TAURI__.core.invoke('search', {
        q: params?.q || '',
        cats: params?.cats || null,
        project: params?.project || null,
        sort: params?.sort || null,
        limit: params?.limit ? Number(params.limit) : null,
        offset: params?.offset ? Number(params.offset) : null,
      });
    }
    if (endpoint === 'session') {
      const id = parts[1] || '';
      const agent = params && params.agent ? params.agent : null;
      return await window.__TAURI__.core.invoke('session_detail', { id, agent });
    }
    throw new Error('Unknown command: ' + path);
  }
  throw new Error('dauclem must be run within the Tauri desktop application.');
};

const S = { projects: [], project: '', sessions: [], cats: { me: 1, agent: 1, act: 1, sub: 1 }, q: '', fq: '', items: [], agentItems: {}, fmatches: [], fi: -1, sort: 'rank', current: null, offset: 0, version: -1 };
try { Object.assign(S.cats, JSON.parse(localStorage.getItem('cats') || '{}')); S.sort = localStorage.getItem('sort') || 'rank'; S.project = localStorage.getItem('project') || ''; } catch { /* ignore */ }

const pLabel = (cwd, dir) => cwd ? cwd.replace(/^[A-Za-z]:\\(Projects\\)?/, '').replace(/\\/g, '/') : dir;
const fmtDate = ts => ts ? new Date(ts).toLocaleString([], { year: '2-digit', month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' }) : '';
const projName = new Map();

// ---------- markdown ----------
function inline(s) {
  s = esc(s);
  s = s.replace(/`([^`\n]+)`/g, '<code>$1</code>');
  s = s.replace(/\*\*([^*\n]+)\*\*/g, '<strong>$1</strong>');
  s = s.replace(/\[([^\]\n]+)\]\((https?:[^)\s]+)\)/g, '<a href="$2" target="_blank" rel="noopener">$1</a>');
  return s;
}
function md(src) {
  const out = [];
  const parts = src.split(/```/);
  parts.forEach((part, i) => {
    if (i % 2) { out.push('<pre><code>' + esc(part.replace(/^[^\n]*\n/, '')) + '</code></pre>'); return; }
    let list = null;
    const flush = () => { if (list) { out.push('</' + list + '>'); list = null; } };
    let para = [];
    const fp = () => { if (para.length) { out.push('<p>' + para.join('<br>') + '</p>'); para = []; } };
    for (const line of part.split('\n')) {
      let m;
      if (!line.trim()) { fp(); flush(); }
      else if ((m = line.match(/^(#{1,4})\s+(.*)/))) { fp(); flush(); out.push('<h3>' + inline(m[2]) + '</h3>'); }
      else if ((m = line.match(/^\s*[-*]\s+(.*)/))) { fp(); if (list !== 'ul') { flush(); out.push('<ul>'); list = 'ul'; } out.push('<li>' + inline(m[1]) + '</li>'); }
      else if ((m = line.match(/^\s*\d+[.)]\s+(.*)/))) { fp(); if (list !== 'ol') { flush(); out.push('<ol>'); list = 'ol'; } out.push('<li>' + inline(m[1]) + '</li>'); }
      else { flush(); para.push(inline(line)); }
    }
    fp(); flush();
  });
  return out.join('');
}

// ---------- highlighting ----------
function terms(q) {
  const out = [], re = /"([^"]+)"|(\S+)/g; let m;
  while ((m = re.exec(q))) { const t = (m[1] || m[2]).trim(); if (t.length >= 2) out.push(t.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')); }
  return out;
}
// in-session query wins over the global one
const hlTerms = () => terms(S.fq || S.q);
function markDom(root) {
  const t = hlTerms(); if (!t.length) return;
  const re = new RegExp('(' + t.join('|') + ')', 'gi');
  const w = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  const nodes = []; let n;
  while ((n = w.nextNode())) if (re.test(n.nodeValue) && !(n.parentElement && n.parentElement.closest('.who,.agentbtn'))) nodes.push(n), re.lastIndex = 0;
  for (const node of nodes) {
    const f = document.createDocumentFragment();
    node.nodeValue.split(re).forEach((p, i) => { if (i % 2) { const m = el('mark'); m.textContent = p; f.append(m); } else if (p) f.append(p); });
    node.replaceWith(f);
  }
}
function snippetHtml(s) { return esc(s).replace(/\u0001/g, '<mark>').replace(/\u0002/g, '</mark>'); }

// ---------- session view ----------
const WHO = { me: 'You', agent: 'Claude', sub: 'Subagent' };
function pre(text) { const p = el('pre'); p.textContent = text; return p; }
function lbl(t) { return el('div', 'lbl', t); }

// ---------- diff ----------
function diffLines(a, b) {
  const A = a === '' ? [] : a.split('\n'), B = b === '' ? [] : b.split('\n');
  let s = 0;
  while (s < A.length && s < B.length && A[s] === B[s]) s++;
  let ea = A.length, eb = B.length;
  while (ea > s && eb > s && A[ea - 1] === B[eb - 1]) { ea--; eb--; }
  const ops = [];
  for (let k = 0; k < s; k++) ops.push([' ', A[k]]);
  const n = ea - s, m = eb - s;
  if (n * m > 400000) {
    for (let k = s; k < ea; k++) ops.push(['-', A[k]]);
    for (let k = s; k < eb; k++) ops.push(['+', B[k]]);
  } else if (n || m) {
    const w = m + 1, L = new Uint32Array((n + 1) * w);
    for (let x = n - 1; x >= 0; x--) for (let y = m - 1; y >= 0; y--)
      L[x * w + y] = A[s + x] === B[s + y] ? L[(x + 1) * w + y + 1] + 1 : Math.max(L[(x + 1) * w + y], L[x * w + y + 1]);
    let x = 0, y = 0;
    while (x < n && y < m) {
      if (A[s + x] === B[s + y]) { ops.push([' ', A[s + x]]); x++; y++; }
      else if (L[(x + 1) * w + y] >= L[x * w + y + 1]) ops.push(['-', A[s + x++]]);
      else ops.push(['+', B[s + y++]]);
    }
    while (x < n) ops.push(['-', A[s + x++]]);
    while (y < m) ops.push(['+', B[s + y++]]);
  }
  for (let k = ea; k < A.length; k++) ops.push([' ', A[k]]);
  return ops;
}
function diffStats(a, b) {
  let add = 0, del = 0;
  for (const o of diffLines(a, b)) { if (o[0] === '+') add++; else if (o[0] === '-') del++; }
  return { add, del };
}
function editStats(it) {
  if (it._stats !== undefined) return it._stats;
  const i = it.input || {}; let st = null;
  try {
    if (typeof i.old_string === 'string') st = diffStats(i.old_string, i.new_string || '');
    else if (Array.isArray(i.edits)) { st = { add: 0, del: 0 }; for (const e of i.edits) { const d = diffStats(e.old_string || '', e.new_string || ''); st.add += d.add; st.del += d.del; } }
    else if (it.tool === 'Write' && typeof i.content === 'string') st = { add: i.content === '' ? 0 : i.content.split('\n').length, del: 0, isNew: true };
  } catch { st = null; }
  return (it._stats = st);
}
function diffEl(a, b) {
  const ops = diffLines(a, b), CTX = 3;
  const keep = new Array(ops.length).fill(false);
  ops.forEach((o, k) => { if (o[0] !== ' ') for (let j = Math.max(0, k - CTX); j <= Math.min(ops.length - 1, k + CTX); j++) keep[j] = true; });
  const box = el('div', 'diff');
  const row = (t, s) => { const r = el('div', 'dl ' + (t === '+' ? 'add' : t === '-' ? 'del' : 'ctx')); const g = el('span', 'sg'); g.textContent = t === ' ' ? '' : t; const x = el('span', 'tx'); x.textContent = s; r.append(g, x); return r; };
  let k = 0;
  while (k < ops.length) {
    if (keep[k]) { box.append(row(ops[k][0], ops[k][1])); k++; continue; }
    let j = k; while (j < ops.length && !keep[j]) j++;
    const sk = el('div', 'dl skip'); sk.textContent = '⋯ ' + (j - k) + ' unchanged line' + (j - k > 1 ? 's' : '');
    box.append(sk); k = j;
  }
  if (!ops.length) box.append(el('div', 'dl skip', '(empty)'));
  return box;
}

function toolBody(it) {
  const f = document.createDocumentFragment();
  const i = it.input || {};
  if (typeof i.command === 'string') { f.append(lbl('Command'), pre(i.command)); }
  else if (typeof i.old_string === 'string') {
    f.append(lbl(i.file_path || 'Edit'), diffEl(i.old_string, i.new_string || ''));
    if (i.replace_all) f.append(el('div', 'lbl', 'replace all occurrences'));
  } else if (Array.isArray(i.edits) && i.file_path) {
    f.append(lbl(i.file_path));
    i.edits.forEach((e, n) => { f.append(lbl('Edit ' + (n + 1) + ' of ' + i.edits.length), diffEl(e.old_string || '', e.new_string || '')); });
  } else if (typeof i.content === 'string' && i.file_path) { f.append(lbl(i.file_path + ' (new content)'), diffEl('', i.content)); }
  else if (typeof i.prompt === 'string') {
    const rest = Object.assign({}, i); delete rest.prompt;
    f.append(lbl('Prompt'), pre(i.prompt));
    if (Object.keys(rest).length) f.append(lbl('Input'), pre(JSON.stringify(rest, null, 2)));
  } else f.append(lbl('Input'), pre(JSON.stringify(i, null, 2)));
  if (it.result != null) {
    f.append(lbl(it.isError ? 'Error' : 'Result'), pre(it.result || '(empty)'));
    if (it.resultTruncated) f.append(el('div', 'lbl', '… truncated'));
  }
  return f;
}
function toolSummaryText(it) {
  const i = it.input || {};
  const v = i.command || i.file_path || i.path || i.pattern || i.description || i.url || i.query || i.skill || (typeof i.prompt === 'string' ? i.prompt : '') || '';
  return String(v).split('\n')[0].slice(0, 200);
}

function renderItem(it, sessionId, agent) {
  const d = el('div', 'it cat-' + it.cat);
  d.dataset.seq = it.seq; d.dataset.agent = agent || '';
  if (it.resultSeq != null) d.dataset.seq2 = it.resultSeq;
  const when = it.ts ? new Date(it.ts).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' }) : '';
  if (it.kind === 'tool' || (it.kind === 'result')) {
    const det = el('details', 'tool' + (it.isError ? ' err' : ''));
    const sm = el('summary');
    const name = el('span', 'name'); name.textContent = it.tool || 'result';
    const sum = el('span', 'sum'); sum.textContent = it.kind === 'tool' ? toolSummaryText(it) : (it.text || '').slice(0, 160);
    sm.append(name, sum);
    const st = it.kind === 'tool' ? editStats(it) : null;
    if (st) {
      const b = el('span', 'dstat');
      if (st.add) b.append(el('span', 'a', '+' + st.add));
      if (st.del) b.append(el('span', 'd', '−' + st.del));
      if (!st.add && !st.del) b.append(el('span', 'n', 'no change'));
      if (st.isNew) b.append(el('span', 'n', 'write'));
      sm.append(b);
    }
    det.append(sm);
    let built = false;
    const build = () => {
      if (built) return; built = true;
      const b = el('div', 'tb');
      if (it.kind === 'tool') b.append(toolBody(it)); else b.append(pre(it.text));
      det.append(b); markDom(b);
    };
    det.addEventListener('toggle', () => det.open && build());
    d._open = () => { det.open = true; build(); };
    markDom(sm);
    d.append(det);
    if (it.agentId) {
      const btn = el('button', 'agentbtn cat-sub', '▸ Subagent log');
      let box = null;
      const toggle = async () => {
        if (box) { box.hidden = !box.hidden; btn.textContent = (box.hidden ? '▸' : '▾') + ' Subagent log'; return; }
        btn.textContent = '… loading';
        box = el('div', 'agentbox');
        d.append(box);
        const r = await api('session/' + sessionId, { agent: it.agentId });
        S.agentItems[it.agentId] = r.items;
        for (const x of r.items) box.append(renderItem(x, sessionId, it.agentId));
        btn.textContent = '▾ Subagent log';
        if (S.fq) findRun(true);
      };
      btn.onclick = toggle;
      d._agent = toggle; d.dataset.agentId = it.agentId;
      d.append(btn);
    }
    return d;
  }
  const who = el('div', 'who', '<span>' + (WHO[it.cat] || '') + (it.kind === 'thinking' ? ' · thinking' : '') + '</span><time>' + when + '</time>');
  const body = el('div', 'body');
  if (it.kind === 'thinking') {
    const det = el('details', 'tool think'); det.append(el('summary', null, 'thinking…'));
    const b = el('div', 'tb'); b.textContent = it.text; det.append(b);
    d.append(det); return d;
  }
  if (it.cat === 'me') body.textContent = it.text; else body.innerHTML = md(it.text);
  if (it.cat === 'me') body.style.whiteSpace = 'pre-wrap';
  markDom(body);
  d.append(who, body);
  return d;
}

async function openSession(id, target) {
  const main = $('#main');
  if (!target || S.current !== id) {
    main.innerHTML = '<div class="empty">Loading…</div>';
    const r = await api('session/' + id);
    if (r.error) { main.innerHTML = '<div class="empty">' + esc(r.error) + '</div>'; return; }
    S.current = id;
    document.querySelectorAll('.row.active').forEach(e => e.classList.remove('active'));
    const act = document.querySelector('.row[data-id="' + id + '"]'); if (act) act.classList.add('active');
    main.innerHTML = '';
    const s = r.session;
    S.items = r.items; S.agentItems = {}; S.fq = ''; S.fmatches = []; S.fi = -1;
    const head = el('div', 'sesshead');
    head.innerHTML = '<div class="hrow"><div class="htext"><h1></h1><div class="m"></div></div>' +
      '<div class="find"><input id="fq" type="search" placeholder="Find in chat…  (Ctrl+F)" autocomplete="off" spellcheck="false">' +
      '<span id="fcount"></span><button id="fprev" title="Previous (Shift+Enter)">↑</button><button id="fnext" title="Next (Enter)">↓</button></div></div>';
    wireFind(head);
    head.querySelector('h1').textContent = s.title;
    head.querySelector('.m').textContent = pLabel(s.cwd, s.project) + ' · ' + fmtDate(s.first_ts) + ' → ' + fmtDate(s.last_ts) + ' · ' + s.n_me + ' you · ' + s.n_agent + ' claude · ' + s.n_act + ' actions · ' + s.n_agents + ' subagents';
    const th = el('div', 'thread');
    const linked = new Set();
    for (const it of r.items) if (it.agentId) linked.add(it.agentId);
    // newest first: render only the last chunk, older ones load on scroll-up
    S.older = el('div', 'older');
    S.older.onclick = () => loadOlder();
    S.start = Math.max(0, r.items.length - CHUNK);
    th.append(S.older, renderRange(S.start, r.items.length));
    updateOlder();
    const orphans = r.agents.filter(a => !linked.has(a));
    if (orphans.length) {
      const o = el('div', 'orphans cat-sub');
      o.append(el('div', 'who', 'Other subagents'));
      for (const a of orphans) {
        const d = el('div', 'it'); const btn = el('button', 'agentbtn', '▸ ' + a); let box = null; d.dataset.agentId = a;
        const t = async () => { if (box) { box.hidden = !box.hidden; return; } box = el('div', 'agentbox'); d.append(box); const x = await api('session/' + id, { agent: a }); S.agentItems[a] = x.items; for (const y of x.items) box.append(renderItem(y, id, a)); if (S.fq) findRun(true); };
        btn.onclick = t; d._agent = t; d.append(btn); o.append(d);
      }
      th.append(o);
    }
    main.append(head, th);
    applyCats();
    buildOutline();
  }
  if (target && target.seq != null) await jumpTo(target);
  else { main.scrollTop = main.scrollHeight; requestAnimationFrame(() => { main.scrollTop = main.scrollHeight; updateOutlineActive(); }); }
  updateOutlineActive();
}

// ---------- right panel: your messages ----------
function buildOutline() {
  const box = $('#outline'); box.innerHTML = '';
  const me = S.items.filter(i => i.cat === 'me');
  box.hidden = !me.length;
  if (!me.length) return;
  box.append(el('div', 'ohead', 'Your messages · ' + me.length));
  const list = el('div', 'olist');
  me.forEach((it, n) => {
    const r = el('div', 'orow');
    r.dataset.seq = it.seq;
    const num = el('span', 'on'); num.textContent = n + 1;
    const txt = el('span', 'ot'); txt.textContent = it.text.replace(/\s+/g, ' ').slice(0, 160);
    const tm = el('span', 'otime'); tm.textContent = it.ts ? new Date(it.ts).toLocaleString([], { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' }) : '';
    const col = el('div', 'oc'); col.append(txt, tm);
    r.append(num, col);
    r.onclick = () => jumpTo({ seq: it.seq, agent: '' });
    list.append(r);
  });
  box.append(list);
}
let outlineRaf = 0, outlineCur = null;
function updateOutlineActive() {
  const main = $('#main');
  const line = main.getBoundingClientRect().top + 90;
  let cur = null;
  for (const d of document.querySelectorAll('.thread > .it.cat-me')) {
    if (d.getBoundingClientRect().top <= line) cur = d; else break;
  }
  if (!cur) cur = document.querySelector('.thread > .it.cat-me');
  const seq = cur ? cur.dataset.seq : null;
  if (seq === outlineCur) return;
  outlineCur = seq;
  document.querySelectorAll('.orow.active').forEach(e => e.classList.remove('active'));
  const row = seq != null && document.querySelector('.orow[data-seq="' + seq + '"]');
  if (row) { row.classList.add('active'); row.scrollIntoView({ block: 'nearest' }); }
}
$('#main').addEventListener('scroll', () => {
  if (outlineRaf) return;
  outlineRaf = requestAnimationFrame(() => { outlineRaf = 0; updateOutlineActive(); });
}, { passive: true });

// ---------- chunked, bottom-up rendering ----------
const CHUNK = 40;
function renderRange(from, to) {
  const f = document.createDocumentFragment();
  for (let i = from; i < to; i++) f.append(renderItem(S.items[i], S.current, ''));
  return f;
}
function updateOlder() {
  S.older.hidden = S.start <= 0;
  S.older.textContent = '↑ ' + S.start + ' earlier messages · scroll up to load';
}
function loadOlder(n = CHUNK) {
  if (S.start <= 0) return false;
  const main = $('#main');
  const from = Math.max(0, S.start - n);
  const h0 = main.scrollHeight, t0 = main.scrollTop;
  S.older.after(renderRange(from, S.start));
  S.start = from;
  updateOlder();
  main.scrollTop = t0 + (main.scrollHeight - h0); // keep viewport anchored
  return true;
}
function ensureRendered(idx) { if (idx >= 0 && S.start > idx) loadOlder(S.start - idx); }
(() => {
  const main = $('#main'); let busy = false;
  main.addEventListener('scroll', () => {
    if (busy || S.start <= 0 || main.scrollTop > 700) return;
    busy = true;
    let guard = 0;
    do { loadOlder(); } while (main.scrollTop < 700 && S.start > 0 && ++guard < 6);
    busy = false;
  }, { passive: true });
})();

// ---------- in-session find ----------
function rehighlight() {
  const th = document.querySelector('.thread'); if (!th) return;
  th.querySelectorAll('mark').forEach(m => m.replaceWith(document.createTextNode(m.textContent)));
  th.normalize();
  markDom(th);
}
function itemMatcher() {
  const t = terms(S.fq); if (!t.length) return null;
  const re = new RegExp(t.join('|'), 'i');
  return it => {
    if (!S.cats[it.cat]) return false;
    let s = it.text || '';
    if (it.kind === 'tool' && it.input) for (const v of Object.values(it.input)) if (typeof v === 'string') s += '\n' + v;
    if (it.result) s += '\n' + it.result;
    return re.test(s);
  };
}
function findRun(keep) {
  const cur = keep && S.fmatches[S.fi];
  S.fmatches = []; S.fi = -1;
  rehighlight();
  const test = itemMatcher();
  const cnt = $('#fcount');
  if (!test) { if (cnt) cnt.textContent = ''; return; }
  for (const it of S.items) if (test(it)) S.fmatches.push({ seq: it.seq, agent: '' });
  for (const [a, items] of Object.entries(S.agentItems)) for (const it of items) if (test(it)) S.fmatches.push({ seq: it.seq, agent: a });
  if (!S.fmatches.length) { cnt.textContent = '0 results'; return; }
  let start = 0;
  if (cur) { const k = S.fmatches.findIndex(m => m.seq === cur.seq && m.agent === cur.agent); if (k >= 0) { S.fi = k; cnt.textContent = (k + 1) + ' / ' + S.fmatches.length; return; } }
  findGo(start);
}
async function findGo(i) {
  const n = S.fmatches.length; if (!n) return;
  S.fi = ((i % n) + n) % n;
  $('#fcount').textContent = (S.fi + 1) + ' / ' + n;
  document.querySelectorAll('.it.cur').forEach(e => e.classList.remove('cur'));
  const node = await jumpTo(S.fmatches[S.fi], true);
  if (node) node.classList.add('cur');
}
function wireFind(head) {
  const fq = head.querySelector('#fq');
  let t;
  fq.addEventListener('input', () => { S.fq = fq.value; clearTimeout(t); t = setTimeout(() => findRun(false), 150); });
  fq.addEventListener('keydown', e => {
    if (e.key === 'Enter') { e.preventDefault(); findGo(S.fi + (e.shiftKey ? -1 : 1)); }
    if (e.key === 'Escape') { fq.value = ''; S.fq = ''; findRun(false); fq.blur(); }
  });
  head.querySelector('#fnext').onclick = () => findGo(S.fi + 1);
  head.querySelector('#fprev').onclick = () => findGo(S.fi - 1);
}

async function jumpTo({ seq, agent }, quiet) {
  let node;
  // make sure the target's chunk is in the DOM
  ensureRendered(agent ? S.items.findIndex(x => x.agentId === agent) : S.items.findIndex(x => x.seq === seq || x.resultSeq === seq));
  if (agent) {
    const host = document.querySelector('.it[data-agent-id="' + agent + '"]');
    if (host && host._agent) {
      if (!host.querySelector('.agentbox')) await host._agent();
      else { const b = host.querySelector('.agentbox'); b.hidden = false; }
    }
    node = document.querySelector('.it[data-agent="' + agent + '"][data-seq="' + seq + '"]');
  } else {
    node = document.querySelector('.it[data-agent=""][data-seq="' + seq + '"],.it[data-agent=""][data-seq2="' + seq + '"]');
  }
  if (!node) return null;
  if (node._open) node._open();
  node.scrollIntoView({ block: 'center' });
  if (!quiet) {
    node.classList.add('flash');
    setTimeout(() => node.classList.remove('flash'), 2500);
  }
  return node;
}

function applyCats() {
  const th = document.querySelector('.thread');
  for (const c of ['me', 'agent', 'act', 'sub']) {
    if (th) th.classList.toggle('hide-' + c, !S.cats[c]);
    document.querySelector('.chip[data-cat="' + c + '"]').classList.toggle('on', !!S.cats[c]);
  }
}

// ---------- sidebar ----------
function renderSessions() {
  const list = $('#list'); list.innerHTML = '';
  const f = $('#filter').value.trim().toLowerCase();
  const frag = document.createDocumentFragment();
  for (const s of S.sessions) {
    if (f && !(s.title + ' ' + s.first_prompt).toLowerCase().includes(f)) continue;
    const r = el('div', 'row' + (s.id === S.current ? ' active' : ''));
    r.dataset.id = s.id;
    const t = el('div', 't'); t.textContent = s.title;
    const m = el('div', 'm');
    m.innerHTML = '<span>' + fmtDate(s.last_ts) + '</span><span>' + s.n_me + ' msgs</span>' + (s.n_agents ? '<span>' + s.n_agents + ' agents</span>' : '') + (!S.project ? '<span>' + esc(pLabel(s.cwd, s.project)) + '</span>' : '');
    r.append(t, m);
    r.onclick = () => openSession(s.id);
    frag.append(r);
  }
  list.append(frag);
}

async function loadSessions() {
  S.sessions = await api('sessions', { project: S.project });
  if (!S.q) renderSessions();
}
async function loadProjects() {
  S.projects = await api('projects');
  const sel = $('#proj'); const keep = S.project;
  sel.innerHTML = '<option value="">All projects</option>';
  for (const p of S.projects) {
    const o = el('option'); o.value = p.project; o.textContent = pLabel(p.cwd, p.project) + ' (' + p.n + ')'; sel.append(o);
    projName.set(p.project, pLabel(p.cwd, p.project));
  }
  sel.value = keep;
  if (sel.value !== keep) { S.project = ''; sel.value = ''; }
}

// ---------- search ----------
let searchSeq = 0;
async function runSearch(more) {
  const my = ++searchSeq;
  if (!S.q.trim()) { $('#qinfo').textContent = ''; renderSessions(); return; }
  if (!more) S.offset = 0;
  const cats = Object.keys(S.cats).filter(c => S.cats[c]).join(',');
  const r = await api('search', { q: S.q, cats, project: S.project, sort: S.sort, offset: S.offset });
  if (my !== searchSeq) return;
  const list = $('#list');
  if (!more) { list.innerHTML = ''; list.scrollTop = 0; }
  list.querySelector('.more')?.remove();
  $('#qinfo').textContent = r.error ? r.error : (r.hits.length || S.offset ? '' : 'no results') + (r.ms != null && !more ? ' ' + r.ms + 'ms' : '');
  const frag = document.createDocumentFragment();
  for (const h of r.hits) {
    const row = el('div', 'row');
    const t = el('div', 't'); t.textContent = h.title;
    const sn = el('div', 'snip', snippetHtml(h.snip));
    const m = el('div', 'm');
    m.innerHTML = '<span class="badge ' + h.cat + '">' + ({ me: 'you', agent: 'claude', act: h.tool || 'action', sub: 'subagent' }[h.cat]) + '</span><span>' + fmtDate(h.ts) + '</span>' + (!S.project ? '<span>' + esc(pLabel(h.cwd, h.project)) + '</span>' : '');
    row.append(t, m, sn);
    row.onclick = () => { openSession(h.session, { seq: h.seq, agent: h.agent }); };
    frag.append(row);
  }
  list.append(frag);
  if (r.more) {
    const b = el('button', 'more', 'Load more'); b.onclick = () => { S.offset += 60; runSearch(true); }; list.append(b);
  }
}

// ---------- wiring ----------
let deb;
$('#q').addEventListener('input', e => { S.q = e.target.value; clearTimeout(deb); deb = setTimeout(() => runSearch(), 160); });
$('#filter').addEventListener('input', () => { if (!S.q) renderSessions(); });
$('#proj').addEventListener('change', async e => {
  S.project = e.target.value; try { localStorage.setItem('project', S.project); } catch { /* ignore */ }
  await loadSessions(); if (S.q) runSearch();
});
$('#sort').value = S.sort;
$('#sort').addEventListener('change', e => { S.sort = e.target.value; try { localStorage.setItem('sort', S.sort); } catch { /* ignore */ } if (S.q) runSearch(); });
$('#chips').addEventListener('click', e => {
  const c = e.target.dataset.cat; if (!c) return;
  S.cats[c] = S.cats[c] ? 0 : 1;
  try { localStorage.setItem('cats', JSON.stringify(S.cats)); } catch { /* ignore */ }
  applyCats(); if (S.q) runSearch(); if (S.fq) findRun(true);
});
document.addEventListener('keydown', e => {
  if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'f' && $('#fq')) { e.preventDefault(); $('#fq').focus(); $('#fq').select(); return; }
  if (e.key === '/' && document.activeElement.tagName !== 'INPUT') { e.preventDefault(); $('#q').focus(); $('#q').select(); }
  if (e.key === 'Escape' && document.activeElement.id === 'q') { $('#q').value = ''; S.q = ''; runSearch(); }
});

async function poll() {
  try {
    const st = await api('status');
    const p = $('#progress');
    p.hidden = !(st.indexing && st.total);
    if (st.indexing && st.total) { p.querySelector('.bar').style.width = (100 * st.done / st.total) + '%'; p.querySelector('span').textContent = 'Indexing ' + st.done + ' / ' + st.total; }
    if (st.version !== S.version) {
      S.version = st.version;
      if (!st.indexing || st.done % 10 === 0) { await loadProjects(); await loadSessions(); }
    }
  } catch { /* server down */ }
  setTimeout(poll, st_delay());
}
let lastIdx = true;
function st_delay() { return lastIdx ? 1000 : 4000; }

(async () => {
  applyCats();
  await loadProjects(); await loadSessions();
  poll();
})();
