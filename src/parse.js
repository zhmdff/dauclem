'use strict';
// Normalizes Claude Code JSONL transcripts into flat items.
// cat: me | agent | act | sub     kind: user | assistant | thinking | tool | result

const SKIP_PREFIX = /^<(local-command|system-reminder|task-notification|bash-input|bash-stdout|bash-stderr|user-prompt|command-message|command-args)/;

function clean(s) {
  if (typeof s !== 'string') return null;
  const t = s.trim();
  if (!t) return null;
  if (t.startsWith('<command-name>')) {
    const name = (t.match(/<command-name>([\s\S]*?)<\/command-name>/) || [])[1] || '';
    const args = (t.match(/<command-args>([\s\S]*?)<\/command-args>/) || [])[1] || '';
    return (name + ' ' + args).trim() || null;
  }
  if (SKIP_PREFIX.test(t)) return null;
  return t;
}

function blocksText(c) {
  if (typeof c === 'string') return c;
  if (!Array.isArray(c)) return '';
  const out = [];
  for (const b of c) {
    if (b.type === 'text' && b.text) out.push(b.text);
    else if (b.type === 'image') out.push('[image]');
  }
  return out.join('\n');
}

function toolSummary(name, i) {
  i = i || {};
  const v = i.command || i.file_path || i.path || i.pattern || i.description || i.url || i.query || i.skill || i.subject || (typeof i.prompt === 'string' ? i.prompt.slice(0, 140) : '') || '';
  return String(v).split('\n')[0].slice(0, 200);
}

function parseFile(raw, isSub) {
  const items = [];
  const meta = { title: null, custom: null, cwd: null, first: 0, last: 0 };
  const seen = new Set();
  let seq = 0;
  const push = (base, o, ts, extra) => {
    items.push(Object.assign({ seq: seq++, cat: isSub ? 'sub' : base, kind: 'user', ts, text: '' }, extra));
  };
  let start = 0;
  const len = raw.length;
  while (start < len) {
    let end = raw.indexOf('\n', start);
    if (end < 0) end = len;
    const line = raw.slice(start, end);
    start = end + 1;
    if (!line) continue;
    let o;
    try { o = JSON.parse(line); } catch { continue; }
    const t = o.type;
    if (t === 'ai-title') { meta.title = o.aiTitle; continue; }
    if (t === 'custom-title') { meta.custom = o.customTitle; continue; }
    if (o.cwd && !meta.cwd) meta.cwd = o.cwd;
    if (t !== 'user' && t !== 'assistant') continue;
    if (o.isMeta || !o.message) continue;
    if (o.uuid) { if (seen.has(o.uuid)) continue; seen.add(o.uuid); }
    const ts = o.timestamp ? Date.parse(o.timestamp) : 0;
    if (ts) { if (!meta.first) meta.first = ts; meta.last = ts; }
    const c = o.message.content;
    if (t === 'user') {
      if (typeof c === 'string') {
        const x = clean(c);
        if (x) push('me', o, ts, { kind: 'user', text: x });
      } else if (Array.isArray(c)) {
        for (const b of c) {
          if (b.type === 'text') {
            const x = clean(b.text);
            if (x) push('me', o, ts, { kind: 'user', text: x });
          } else if (b.type === 'tool_result') {
            const tur = o.toolUseResult;
            push('act', o, ts, {
              kind: 'result',
              toolUseId: b.tool_use_id,
              text: blocksText(b.content),
              isError: !!b.is_error,
              agentId: tur && typeof tur === 'object' && tur.agentId ? tur.agentId : undefined,
            });
          }
        }
      }
    } else {
      if (!Array.isArray(c)) continue;
      for (const b of c) {
        if (b.type === 'text') {
          if (b.text && b.text.trim()) push('agent', o, ts, { kind: 'assistant', text: b.text });
        } else if (b.type === 'thinking') {
          if (b.thinking && b.thinking.trim()) push('agent', o, ts, { kind: 'thinking', text: b.thinking });
        } else if (b.type === 'tool_use') {
          push('act', o, ts, { kind: 'tool', tool: b.name, toolUseId: b.id, input: b.input, text: '' });
        }
      }
    }
  }
  return { items, meta };
}

module.exports = { parseFile, toolSummary };
