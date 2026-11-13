// Maestro web UI — vanilla JS, no build step. Live updates via SSE.
'use strict';

const S = { tickets: new Map(), agents: [], skills: [], settings: {}, project: null, stats: {}, sm: { states: [], transitions: [] },
  runs: [], busy: [], lastSync: null, claudeVersion: null, view: 'board', boardMode: 'state', filter: { q: '', repo: '', agent: '' },
  selRun: null, drawerTicket: null, feed: [], plan: null, planning: false, hideDone: false, graphView: null,
  source: { mode: 'repo', repos: [] }, assistant: {}, chats: null, chatId: null, chatMsgs: [], chatBusy: {}, chatLive: [] };

const STATE_LABEL = { backlog: 'Backlog', triage: 'Triage', split: 'Split', blocked: 'Blocked', ready: 'Ready', running: 'Running',
  reviewing: 'Reviewing', awaiting_merge: 'Awaiting merge', done: 'Done', needs_human: 'Needs human', paused: 'Paused' };
const BOARD_COLS = [
  ['Backlog', ['backlog', 'paused']], ['Triage', ['triage', 'split']], ['Queued', ['ready', 'blocked']],
  ['In progress', ['running']], ['Review', ['reviewing', 'awaiting_merge']], ['Needs human', ['needs_human']], ['Done', ['done']],
];
const sc = s => `var(--s-${s})`;

// ───────── icons (stroke icons, 24×24 grid)
const ICONS = {
  board: '<rect x="3" y="4" width="18" height="16" rx="2.5"/><path d="M9 4v16M15 4v16"/>',
  chat: '<path d="M20 12.5a7.5 7.5 0 0 1-11.2 6.5L4 20l1.1-4.3A7.5 7.5 0 1 1 20 12.5z"/><path d="M8.5 11h7M8.5 14.5h4.5"/>',
  send: '<path d="M4.5 12L20 4.5l-4 15.5-4.2-6.1z"/><path d="M11.8 13.9L20 4.5"/>',
  repo: '<path d="M6 3.5h11.5a1 1 0 0 1 1 1v13H6.8a2.3 2.3 0 0 0 0 4.6h11.7"/><path d="M6 3.5a2.3 2.3 0 0 0-2.3 2.3v13.9M9 8h6"/>',
  graph: '<circle cx="5.5" cy="6" r="2.5"/><circle cx="5.5" cy="18" r="2.5"/><circle cx="18.5" cy="12" r="2.5"/><path d="M8 6h2.5a3 3 0 0 1 3 3v0a3 3 0 0 0 3 3M8 18h2.5a3 3 0 0 0 3-3v0a3 3 0 0 1 3-3"/>',
  machine: '<rect x="3" y="3" width="7" height="7" rx="2"/><rect x="14" y="14" width="7" height="7" rx="2"/><path d="M6.5 10v3.5a3 3 0 0 0 3 3H14M17.5 14v-3.5a3 3 0 0 0-3-3H10"/>',
  planner: '<path d="M12 3l1.8 4.7L18.5 9.5l-4.7 1.8L12 16l-1.8-4.7L5.5 9.5l4.7-1.8z"/><path d="M18.5 15.5l.8 1.9 1.9.8-1.9.8-.8 1.9-.8-1.9-1.9-.8 1.9-.8z"/>',
  agents: '<rect x="4" y="8" width="16" height="12" rx="3.5"/><path d="M12 4.5V8M9 13.5v1.5M15 13.5v1.5M2 14v2M22 14v2"/><circle cx="12" cy="3.5" r="1"/>',
  runs: '<rect x="3" y="4" width="18" height="16" rx="2.5"/><path d="M7 9l3 3-3 3M13 15h4"/>',
  settings: '<path d="M4 7h9M17 7h3M4 17h3M11 17h9"/><circle cx="15" cy="7" r="2"/><circle cx="9" cy="17" r="2"/>',
  sync: '<path d="M20.5 12a8.5 8.5 0 0 1-14.8 5.7L3.5 15.5M3.5 12a8.5 8.5 0 0 1 14.8-5.7l2.2 2.2"/><path d="M20.5 3.5v5h-5M3.5 20.5v-5h5"/>',
  play: '<path d="M8 5.5v13a.8.8 0 0 0 1.2.7l10.3-6.5a.8.8 0 0 0 0-1.4L9.2 4.8A.8.8 0 0 0 8 5.5z"/>',
  pause: '<rect x="6.5" y="5" width="3.5" height="14" rx="1"/><rect x="14" y="5" width="3.5" height="14" rx="1"/>',
  stop: '<rect x="6" y="6" width="12" height="12" rx="2"/>',
  plus: '<path d="M12 5v14M5 12h14"/>',
  x: '<path d="M6 6l12 12M18 6L6 18"/>',
  search: '<circle cx="11" cy="11" r="6.5"/><path d="M20 20l-4.2-4.2"/>',
  external: '<path d="M14 4h6v6M20 4l-8.5 8.5M18 14v4.5a1.5 1.5 0 0 1-1.5 1.5h-11A1.5 1.5 0 0 1 4 18.5v-11A1.5 1.5 0 0 1 5.5 6H10"/>',
  updown: '<path d="M8 9.5l4-4 4 4M8 14.5l4 4 4-4"/>',
  check: '<path d="M5 12.5l4.5 4.5L19 7.5"/>',
  sun: '<circle cx="12" cy="12" r="4"/><path d="M12 2.5v2M12 19.5v2M4.6 4.6l1.4 1.4M18 18l1.4 1.4M2.5 12h2M19.5 12h2M4.6 19.4L6 18M18 6l1.4-1.4"/>',
  moon: '<path d="M20 14.5A8.5 8.5 0 0 1 9.5 4a7 7 0 1 0 10.5 10.5z"/>',
  menu: '<path d="M4 7h16M4 12h16M4 17h16"/>',
  link: '<path d="M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1 1M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1-1"/>',
  pr: '<circle cx="6" cy="6" r="2.3"/><circle cx="6" cy="18" r="2.3"/><circle cx="18" cy="18" r="2.3"/><path d="M6 8.3v7.4M18 15.7V10a3 3 0 0 0-3-3h-4.5M12.5 4.5L10 7l2.5 2.5"/>',
  branch: '<circle cx="6" cy="5.5" r="2.3"/><circle cx="6" cy="18.5" r="2.3"/><circle cx="18" cy="7" r="2.3"/><path d="M6 7.8v8.4M18 9.3c0 4-3 5.7-6.5 6.2L8 16"/>',
  merge: '<circle cx="6" cy="5.5" r="2.3"/><circle cx="6" cy="18.5" r="2.3"/><circle cx="18" cy="14" r="2.3"/><path d="M6 7.8v8.4M6 7.8c1 3.7 4.5 6.2 9.7 6.2"/>',
  retry: '<path d="M3.5 12a8.5 8.5 0 1 0 2.6-6.1L3.5 8.5"/><path d="M3.5 3.5v5h5"/>',
  reset: '<path d="M4 4v6h6"/><path d="M5.3 15a8 8 0 1 0 1.6-8.4L4 10"/>',
  alert: '<path d="M10.3 4.2L2.6 17.5A2 2 0 0 0 4.3 20.5h15.4a2 2 0 0 0 1.7-3L13.7 4.2a2 2 0 0 0-3.4 0z"/><path d="M12 9.5v4M12 17v.01"/>',
  info: '<circle cx="12" cy="12" r="9"/><path d="M12 11v5.5M12 7.5v.01"/>',
  ok: '<circle cx="12" cy="12" r="9"/><path d="M8.2 12.3l2.6 2.6 5-5.4"/>',
  trash: '<path d="M4 7h16M10 11v6M14 11v6M5.5 7l1 12.5a1.5 1.5 0 0 0 1.5 1.5h8a1.5 1.5 0 0 0 1.5-1.5L18.5 7M9 7V4.5h6V7"/>',
  cell: '<rect x="3" y="3" width="7.5" height="7.5" rx="2"/><rect x="13.5" y="3" width="7.5" height="7.5" rx="2"/><rect x="3" y="13.5" width="7.5" height="7.5" rx="2"/><rect x="13.5" y="13.5" width="7.5" height="7.5" rx="2"/>',
  skill: '<path d="M13 2.5L4.5 13.5H11l-1 8 8.5-11H12z"/>',
  tag: '<path d="M3 12.2V4.5A1.5 1.5 0 0 1 4.5 3h7.7l8.8 8.8-9.2 9.2z"/><circle cx="7.8" cy="7.8" r="1.4"/>',
  cpu: '<rect x="6" y="6" width="12" height="12" rx="2"/><path d="M9.5 2.5V6M14.5 2.5V6M9.5 18v3.5M14.5 18v3.5M2.5 9.5H6M2.5 14.5H6M18 9.5h3.5M18 14.5h3.5"/>',
  copy: '<rect x="9" y="9" width="11.5" height="11.5" rx="2"/><path d="M5.5 15H5a1.5 1.5 0 0 1-1.5-1.5V5A1.5 1.5 0 0 1 5 3.5h8.5A1.5 1.5 0 0 1 15 5v.5"/>',
  edit: '<path d="M4 20h4L19 9a2.8 2.8 0 0 0-4-4L4 16z"/><path d="M13.5 6.5l4 4"/>',
  folder: '<path d="M3 7.5A2.5 2.5 0 0 1 5.5 5H9l2 2.5h7.5A2.5 2.5 0 0 1 21 10v7.5a2.5 2.5 0 0 1-2.5 2.5h-13A2.5 2.5 0 0 1 3 17.5z"/>',
  user: '<circle cx="12" cy="8" r="4"/><path d="M4.5 20.5a7.5 7.5 0 0 1 15 0"/>',
  dollar: '<path d="M12 3v18M16.5 7.5C16.5 5.8 14.5 4.5 12 4.5S7.5 5.8 7.5 7.5 9.5 10.3 12 11s4.5 1.8 4.5 3.5-2 3.5-4.5 3.5-4.5-1.3-4.5-3"/>',
  issue: '<circle cx="12" cy="12" r="8.5"/><circle cx="12" cy="12" r="1.5"/>',
  zoomin: '<circle cx="11" cy="11" r="6.5"/><path d="M20 20l-4.2-4.2M8.5 11h5M11 8.5v5"/>',
  zoomout: '<circle cx="11" cy="11" r="6.5"/><path d="M20 20l-4.2-4.2M8.5 11h5"/>',
  fit: '<path d="M4 9V5.5A1.5 1.5 0 0 1 5.5 4H9M15 4h3.5A1.5 1.5 0 0 1 20 5.5V9M20 15v3.5a1.5 1.5 0 0 1-1.5 1.5H15M9 20H5.5A1.5 1.5 0 0 1 4 18.5V15"/>',
  key: '<circle cx="8" cy="15" r="4"/><path d="M11 12l8.5-8.5M16.5 6.5l2.5 2.5M14 9l2 2"/>',
  github: '<path d="M9 19c-4.3 1.4-4.3-2.5-6-3m12 5v-3.5c0-1 .1-1.4-.5-2 2.8-.3 5.5-1.4 5.5-6a4.6 4.6 0 0 0-1.3-3.2 4.2 4.2 0 0 0-.1-3.2s-1.1-.3-3.5 1.3a12.3 12.3 0 0 0-6.2 0C6.5 2.8 5.4 3.1 5.4 3.1a4.2 4.2 0 0 0-.1 3.2A4.6 4.6 0 0 0 4 9.5c0 4.6 2.7 5.7 5.5 6-.6.6-.6 1.2-.5 2V21"/>',
  git: '<circle cx="12" cy="12" r="3"/><path d="M3 12h6M15 12h6"/>',
  bolt: '<path d="M13 2.5L4.5 13.5H11l-1 8 8.5-11H12z"/>',
  close: '<circle cx="12" cy="12" r="8.5"/><path d="M9 9l6 6M15 9l-6 6"/>',
  reopen: '<circle cx="12" cy="12" r="8.5"/><path d="M12 8v4l2.5 2.5"/>',
  inbox: '<path d="M3 13.5l2.6-7.7A2 2 0 0 1 7.5 4.5h9a2 2 0 0 1 1.9 1.3l2.6 7.7V18a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/><path d="M3 13.5h5l1.5 2.5h5l1.5-2.5h5"/>',
};
const SOLID = new Set(['play']);
function icon(name, cls = '') {
  const s = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
  s.setAttribute('viewBox', '0 0 24 24');
  s.setAttribute('class', `ic ${SOLID.has(name) ? 'solid' : ''} ${cls}`);
  s.setAttribute('aria-hidden', 'true');
  s.innerHTML = ICONS[name] || '';
  return s;
}

// ───────── tiny DOM helper
function h(tag, attrs = {}, ...kids) {
  const el = document.createElementNS(tag.startsWith('svg:') ? 'http://www.w3.org/2000/svg' : 'http://www.w3.org/1999/xhtml', tag.replace('svg:', ''));
  for (const [k, v] of Object.entries(attrs || {})) {
    if (v == null || v === false) continue;
    if (k.startsWith('on')) el.addEventListener(k.slice(2), v);
    else if (k === 'style' && typeof v === 'object') Object.assign(el.style, v);
    else if (k === 'html') el.innerHTML = v;
    else el.setAttribute(k, v === true ? '' : v);
  }
  for (const k of kids.flat(Infinity)) if (k != null && k !== false) el.append(k.nodeType ? k : document.createTextNode(String(k)));
  return el;
}
const $ = s => document.querySelector(s);
const put = (el, ...kids) => el.replaceChildren(...kids.flat(Infinity).filter(k => k != null && k !== false));
const money = v => '$' + (v || 0).toFixed(v >= 10 ? 1 : 3);
const ago = ts => { if (!ts) return '—'; const s = (Date.now() - new Date(ts)) / 1000; return s < 60 ? `${s | 0}s ago` : s < 3600 ? `${(s / 60) | 0}m ago` : s < 86400 ? `${(s / 3600) | 0}h ago` : `${(s / 86400) | 0}d ago`; };
const time = ts => ts ? new Date(ts).toLocaleTimeString() : '';
const lbl = text => h('span', { class: 'btn-label' }, text);
// button with an icon: btn('plus', 'New ticket', { class: 'btn primary', onclick })
const btn = (ic, text, attrs = {}) => h('button', { type: 'button', class: 'btn', ...attrs }, ic ? icon(ic, 'sm') : null, text);

async function api(path, opts = {}) {
  const r = await fetch(path, { headers: { 'content-type': 'application/json' }, ...opts, body: opts.body ? JSON.stringify(opts.body) : undefined });
  const j = await r.json().catch(() => ({}));
  if (!r.ok) throw new Error(j.error || r.statusText);
  return j;
}
function toast(msg, level = 'info') {
  const ic = { error: 'alert', warn: 'alert', ok: 'ok' }[level] || 'info';
  const t = h('div', { class: `toast ${level}`, role: level === 'error' ? 'alert' : 'status' }, icon(ic), h('div', {}, msg));
  $('#toasts').append(t);
  setTimeout(() => { t.classList.add('out'); setTimeout(() => t.remove(), 220); }, level === 'error' ? 9000 : 4500);
}
async function act(fn, okMsg) {
  try { const r = await fn(); if (okMsg) toast(okMsg, 'ok'); return r; } catch (e) { toast(e.message, 'error'); }
}

// ───────── modal dialogs (replace prompt/confirm)
function openModal({ title, desc, body, foot, width, onClose }) {
  let closed = false;
  const close = () => { if (closed) return; closed = true; ov.remove(); onClose?.(); };
  const ov = h('div', { class: 'overlay', onmousedown: e => { if (e.target === ov) close(); } },
    h('div', { class: 'modal', role: 'dialog', 'aria-modal': 'true', style: width ? `width:min(${width}px,100%)` : null },
      h('div', { class: 'modal-h' }, h('div', {}, h('h3', {}, title), desc ? h('p', {}, desc) : null),
        h('button', { class: 'icon-btn sm', type: 'button', 'aria-label': 'Close', onclick: close }, icon('x'))),
      h('div', { class: 'modal-b' }, body),
      foot ? h('div', { class: 'modal-f' }, foot) : null));
  ov._close = close;
  document.body.append(ov);
  setTimeout(() => ov.querySelector('[autofocus], input, textarea, select, .modal-f .btn.primary, .modal-f .btn.danger-solid')?.focus(), 30);
  return close;
}
function confirmDialog(message, { title = 'Are you sure?', ok = 'Confirm', danger = false } = {}) {
  return new Promise(res => {
    let result = false;
    const close = openModal({ title, onClose: () => res(result), body: h('p', { style: 'margin:0;color:var(--tx-2)' }, message),
      foot: [h('button', { class: 'btn ghost', type: 'button', onclick: () => close() }, 'Cancel'),
        h('button', { class: `btn ${danger ? 'danger-solid' : 'primary'}`, type: 'button', onclick: () => { result = true; close(); } }, ok)] });
  });
}
function promptDialog({ title, label, value = '', placeholder = '', ok = 'Save', desc }) {
  return new Promise(res => {
    let result = null;
    const submit = () => { result = input.value; close(); };
    const input = h('input', { value, placeholder, autofocus: true, onkeydown: e => { if (e.key === 'Enter') submit(); } });
    const close = openModal({ title, desc, onClose: () => res(result), body: h('label', { class: 'f', style: 'margin:0' }, label ? h('span', {}, label) : null, input),
      foot: [h('button', { class: 'btn ghost', type: 'button', onclick: () => close() }, 'Cancel'),
        h('button', { class: 'btn primary', type: 'button', onclick: submit }, ok)] });
    setTimeout(() => { input.focus(); input.select(); }, 40);
  });
}

// ───────── agents helpers
const agentOf = n => S.agents.find(a => a.name === n);
const cellOf = n => (S.cells || []).find(c => c.name === n);
const agentColor = n => agentOf(n)?.color || cellOf(n)?.color || '#7c8799';
const workersList = () => S.agents.filter(a => a.role !== 'orchestrator' && a.role !== 'reviewer');
const initial = n => (String(n || '?').replace(/[^a-z0-9]/i, '')[0] || '?').toUpperCase();
function agentChip(n) {
  if (!n) return null;
  const c = cellOf(n);
  return h('span', { class: `agent-chip ${c ? 'cell' : ''}`, style: `--ac:${agentColor(n)}`, title: c ? `cell: ${[c.lead, ...c.members].filter(Boolean).join(', ')}` : 'agent' },
    h('span', { class: 'av' }, initial(n)), n);
}
function assignees(t) {
  return [agentChip(t.agent), (t.helpers || []).map(x => h('span', { class: 'agent-chip helper', style: `--ac:${agentColor(x)}`, title: 'helper' }, icon('plus', 'xs'), x))];
}
function assigneeOptions(selected, emptyLabel) {
  return [emptyLabel ? h('option', { value: '' }, emptyLabel) : null,
    h('optgroup', { label: 'Agents' }, workersList().map(a => h('option', { value: a.name, selected: a.name === selected }, a.name))),
    (S.cells || []).length ? h('optgroup', { label: 'Cells' }, S.cells.map(c => h('option', { value: c.name, selected: c.name === selected }, `${c.name} (${c.mode || 'delegate'})`))) : null];
}
function stateBadge(s) { return h('span', { class: 'badge', style: `--c:${sc(s)}` }, STATE_LABEL[s] || s); }

// ───────── data
async function load() {
  const d = await api('/api/state');
  S.tickets = new Map(d.tickets.map(t => [t.id, t]));
  S.agents = d.agents; S.cells = d.cells || []; S.skills = d.skills; S.settings = d.settings; S.project = d.project; S.projects = d.projects || []; S.activeProject = d.active_project || ''; S.stats = d.stats;
  S.source = d.source || S.source; S.assistant = d.assistant || {}; S.chatBusy = (d.assistant && d.assistant.busy) || S.chatBusy;
  S.envSecrets = d.env_secrets || []; S.mcp = d.mcp; S.sm = d.statemachine; S.runs = d.runs; S.busy = d.busy; S.lastSync = d.last_sync; S.claudeVersion = d.claude_version;
  renderChrome(); render();
}

function connect() {
  const es = new EventSource('/api/events');
  es.onmessage = m => {
    const e = JSON.parse(m.data);
    switch (e.type) {
      case 'ticket': S.tickets.set(e.ticket.id, e.ticket); if (S.drawerTicket === e.ticket.id) openTicket(e.ticket.id, true); break;
      case 'ticket_removed': S.tickets.delete(e.id); break;
      case 'transition': S.feed.unshift(e); S.feed = S.feed.slice(0, 80); flashTransition(e); break;
      case 'run': { const i = S.runs.findIndex(r => r.id === e.run.id); if (i >= 0) S.runs[i] = e.run; else S.runs.unshift(e.run); break; }
      case 'log': appendLog(e.line); chatLog(e.line); return;
      case 'chat': onChatMessage(e.message); return;
      case 'chat_state': onChatState(e.chat_id, e.run_id); return;
      case 'chats': loadChats(); return;
      case 'notice':
        if (e.level === 'plan') { S.planning = false; S.plan = JSON.parse(e.message).plan; S.planRun = JSON.parse(e.message).run_id; if (S.view === 'planner') render(); toast('Plan ready', 'ok'); return; }
        toast(e.message, e.level); if (e.message.startsWith('planner')) { S.planning = false; if (S.view === 'planner') render(); } break;
    }
    scheduleRender();
  };
  es.onerror = () => { const si = $('#sync-info'); si.className = 'sync-info err'; put(si, h('span', { class: 'spin' }), 'reconnecting…'); };
  es.onopen = () => load().catch(() => {});
}
let rt = null;
function scheduleRender() { if (rt) return; rt = setTimeout(() => { rt = null; renderChrome(); if (!['settings', 'agents', 'planner', 'assistant'].includes(S.view) || S.view === 'agents' && !S.editing) render(); }, 120); }

// ───────── chrome (sidebar, top bar)
const NAV = [
  ['Workspace', [['assistant', 'chat', 'Assistant'], ['board', 'board', 'Board'], ['graph', 'graph', 'Graph'], ['machine', 'machine', 'State machine'], ['planner', 'planner', 'Planner']]],
  ['Manage', [['agents', 'agents', 'Agents, cells, skills'], ['runs', 'runs', 'Runs & logs'], ['settings', 'settings', 'Settings']]],
];
const VIEW_TITLE = Object.fromEntries(NAV.flatMap(([, items]) => items.map(([v, , l]) => [v, l])));

function renderNav() {
  const ts = [...S.tickets.values()];
  const needs = ts.filter(t => t.state === 'needs_human').length;
  const live = S.runs.filter(r => r.status === 'running').length;
  put($('#nav'), NAV.map(([sec, items]) => [h('div', { class: 'nav-sec' }, sec),
    items.map(([v, ic, l]) => h('a', { 'data-view': v, class: S.view === v ? 'active' : '', href: `#${v}` }, icon(ic), l,
      v === 'board' && needs ? h('span', { class: 'nav-badge', title: 'need you' }, needs) : null,
      v === 'runs' && live ? h('span', { class: 'nav-badge live', title: 'running' }, live) : null,
      v === 'assistant' && Object.keys(S.chatBusy || {}).length ? h('span', { class: 'nav-badge live', title: 'answering' }, h('span', { class: 'spin' })) : null))]));
}

function renderChrome() {
  renderProjectSwitcher();
  renderNav();
  const ts = [...S.tickets.values()];
  const count = s => ts.filter(t => t.state === s).length;
  $('#page-title').textContent = VIEW_TITLE[S.view] || '';
  document.title = `${VIEW_TITLE[S.view] || 'Maestro'} · Maestro`;
  const repos = S.source?.mode === 'repo' ? (S.source.repos || []) : [];
  put($('#proj'), repos.length
    ? [repos.slice(0, 3).map(r => h('a', { href: `https://github.com/${r}/issues`, target: '_blank', rel: 'noopener', title: 'Open the issues on GitHub' }, icon('repo', 'xs'), r)),
      repos.length > 3 ? h('span', {}, `+${repos.length - 3}`) : null,
      S.project ? h('a', { href: S.project.url, target: '_blank', rel: 'noopener', title: 'Mirror board' }, icon('board', 'xs'), S.project.title) : null]
    : S.project
      ? [h('span', {}, S.project.title), h('a', { href: S.project.url, target: '_blank', rel: 'noopener', title: 'Open on GitHub' }, icon('external', 'xs'), 'GitHub')]
      : h('span', {}, 'No project — add one from the switcher or ask the Assistant'));
  const needs = count('needs_human');
  put($('#kpis'),
    h('span', { class: 'kpi opt' }, h('b', {}, ts.length), 'tickets'),
    h('span', { class: 'kpi' }, h('span', { class: 'dot', style: `--c:${sc('running')}` }), h('b', {}, count('running') + count('reviewing')), 'active'),
    h('span', { class: 'kpi' }, h('b', {}, count('ready') + count('triage')), 'queued'),
    needs ? h('span', { class: 'kpi alert', title: 'Show on board', onclick: () => go('board') }, icon('alert', 'xs'), h('b', {}, needs), 'need you') : null,
    h('span', { class: 'kpi opt' }, h('b', {}, count('done')), 'done'),
    h('span', { class: 'kpi' }, icon('dollar', 'xs'), h('b', {}, money(S.stats.total_cost_usd)), 'spent'),
  );
  const on = S.settings.engine_enabled;
  const b = $('#btn-engine');
  put(b, h('span', { class: 'led' }), lbl(on ? 'Engine running' : 'Engine paused'), icon(on ? 'pause' : 'play', 'sm'));
  b.title = on ? 'Pause the engine' : 'Start the engine';
  b.classList.toggle('on', !!on);
  const ls = S.lastSync, si = $('#sync-info');
  si.className = 'sync-info' + (ls?.error ? ' err' : '');
  si.title = ls?.error || '';
  put(si, ls ? (ls.error ? [icon('alert', 'xs'), 'sync error'] : `synced ${ago(ls.ts)}`) : null);
  const workers = S.busy.length, max = S.settings.max_parallel || 0;
  put($('#side-foot'), h('div', { class: 'sys' },
    h('div', { class: 'row' }, h('span', {}, 'Claude Code'), h('span', { style: S.claudeVersion ? null : 'color:var(--warn)' }, S.claudeVersion || 'not found')),
    h('div', { class: 'row' }, h('span', {}, 'Auth'), h('span', {}, { account: 'Account', oauth_token: 'Token', api_key: 'API key' }[S.settings.claude_auth] || '—')),
    h('div', { class: 'row' }, h('span', {}, 'Workers'), h('span', {}, `${workers} / ${max || '?'}`)),
    h('div', { class: 'meter' }, h('i', { style: `width:${max ? Math.min(100, workers / max * 100) : 0}%` }))));
}

// theme
function currentTheme() { return document.documentElement.getAttribute('data-theme') === 'light' ? 'light' : 'dark'; }
function renderThemeBtn() { const t = currentTheme(); put($('#btn-theme'), icon(t === 'dark' ? 'moon' : 'sun', 'sm'), t === 'dark' ? 'Dark theme' : 'Light theme', h('span', { class: 'kbd' }, 'T')); }
function toggleTheme() {
  const t = currentTheme() === 'dark' ? 'light' : 'dark';
  document.documentElement.setAttribute('data-theme', t);
  try { localStorage.setItem('maestro-theme', t); } catch (e) { /* storage unavailable */ }
  renderThemeBtn();
  if (S.view === 'graph' || S.view === 'machine') render();
}
$('#btn-theme').onclick = toggleTheme;
put($('#btn-menu'), icon('menu'));
$('#btn-menu').onclick = () => document.body.classList.toggle('nav-open');
$('#side-scrim').onclick = () => document.body.classList.remove('nav-open');
put($('#btn-sync'), icon('sync', 'sm'), lbl('Sync'));
renderThemeBtn();

$('#btn-engine').onclick = () => act(async () => { const r = await api('/api/engine', { method: 'POST', body: { enabled: !S.settings.engine_enabled } }); S.settings.engine_enabled = r.enabled; renderChrome(); });
$('#btn-sync').onclick = () => act(async () => {
  const b = $('#btn-sync'); b.disabled = true; put(b, h('span', { class: 'spin' }), lbl('Syncing'));
  try { const r = await api('/api/sync', { method: 'POST' }); await load(); toast(`Synced ${r.items} items`, 'ok'); } finally { b.disabled = false; put(b, icon('sync', 'sm'), lbl('Sync')); }
});
function go(view) { S.view = view; S.editing = null; document.body.classList.remove('nav-open'); renderChrome(); render(); $('#view').scrollTop = 0; }
$('#nav').onclick = e => { const a = e.target.closest('a'); if (!a) return; e.preventDefault(); go(a.dataset.view); };

function render() {
  const v = $('#view');
  const fn = { assistant: viewAssistant, board: viewBoard, graph: viewGraph, machine: viewMachine, planner: viewPlanner, agents: viewAgents, runs: viewRuns, settings: viewSettings }[S.view];
  const keepScroll = v.scrollTop;
  const active = document.activeElement, keepId = active && v.contains(active) ? active.id : null;
  put(v, fn());
  v.scrollTop = keepScroll;
  if (keepId) { const el = document.getElementById(keepId); if (el) { el.focus(); if (el.setSelectionRange && el.value != null) el.setSelectionRange(el.value.length, el.value.length); } }
  if (S.view === 'runs' && S.selRun) loadLog(S.selRun, $('#log-main'));
}

// ───────── filters
function filtered() {
  const { q, repo, agent } = S.filter;
  return [...S.tickets.values()].filter(t =>
    (!q || (`#${t.number} ${t.title}`).toLowerCase().includes(q.toLowerCase())) && (!repo || t.repo === repo) && (!agent || t.agent === agent || (t.helpers || []).includes(agent)));
}
function filterBar(extra = []) {
  const repos = [...new Set([...S.tickets.values()].map(t => t.repo))].sort();
  return h('div', { class: 'toolbar' },
    h('div', { class: 'search' }, icon('search', 'sm'),
      h('input', { id: 'f-search', placeholder: 'Search tickets…', value: S.filter.q, oninput: e => { S.filter.q = e.target.value; clearTimeout(S._ft); S._ft = setTimeout(render, 200); } }),
      h('span', { class: 'kbd' }, '/')),
    h('select', { onchange: e => { S.filter.repo = e.target.value; render(); } }, h('option', { value: '' }, 'All repos'), repos.map(r => h('option', { value: r, selected: r === S.filter.repo }, r))),
    h('select', { onchange: e => { S.filter.agent = e.target.value; render(); } }, h('option', { value: '' }, 'All agents & cells'), [...S.agents, ...(S.cells || [])].map(a => h('option', { value: a.name, selected: a.name === S.filter.agent }, a.name))),
    ...extra);
}

// ───────── BOARD
function ticketCard(t) {
  const running = S.busy.includes(t.id) && ['running', 'reviewing', 'triage'].includes(t.state);
  const openDeps = t.deps.filter(d => S.tickets.get(d)?.state !== 'done').length;
  return h('div', { class: `tcard ${running ? 'running' : ''}`, tabindex: 0, onclick: () => openTicket(t.id), onkeydown: e => { if (e.key === 'Enter') openTicket(t.id); } },
    h('div', { class: 'top-row' }, h('span', { class: 'num' }, `${t.repo.split('/')[1] || ''}#${t.number}`),
      running ? h('span', { class: 'spin', style: `color:${sc('running')}` }) : null, stateBadge(t.state)),
    h('div', { class: 't' }, t.title),
    h('div', { class: 'meta' }, assignees(t),
      t.deps.length ? h('span', { class: 'pill', title: 'open / total dependencies', style: openDeps ? null : 'color:var(--ok)' }, icon('link'), openDeps, '/', t.deps.length) : null,
      t.pr_url ? h('a', { class: 'pill', href: t.pr_url, target: '_blank', rel: 'noopener', onclick: e => e.stopPropagation() }, icon('pr'), '#', t.pr_number) : null,
      t.cost_usd ? h('span', { class: 'pill mono' }, money(t.cost_usd)) : null,
      t.gh_status ? h('span', { class: 'pill', title: 'GitHub column' }, icon('board'), t.gh_status) : null),
    t.note && t.state === 'needs_human' ? h('div', { class: 'note' }, t.note.slice(0, 220)) : null);
}
function viewBoard() {
  const ts = filtered();
  if (!activeProject() && !ts.length) return onboarding();
  let cols;
  if (S.boardMode === 'github') {
    const st = S.settings, repoCols = [st.col_trigger, st.col_in_progress, st.col_review, st.col_done].filter(Boolean);
    const names = S.project?.options?.map(o => o[1]) || (S.source?.mode === 'repo' ? repoCols : [...new Set(ts.map(t => t.gh_status))]);
    cols = [...names, ''].map(n => [n || 'No status', ts.filter(t => (t.gh_status || '') === n), null]);
  } else cols = BOARD_COLS.map(([n, st]) => [n, ts.filter(t => st.includes(t.state)), st[0]]);
  return h('div', { class: 'fill' },
    filterBar([h('div', { class: 'seg' },
      h('button', { type: 'button', class: S.boardMode === 'state' ? 'on' : '', onclick: () => { S.boardMode = 'state'; render(); } }, 'Maestro states'),
      h('button', { type: 'button', class: S.boardMode === 'github' ? 'on' : '', onclick: () => { S.boardMode = 'github'; render(); } }, icon('github', 'sm'), S.source?.mode === 'repo' && !S.project ? 'GitHub labels' : 'GitHub columns')),
    h('span', { class: 'spacer' }),
    btn('plus', 'New ticket', { class: 'btn primary', onclick: () => newTicket() })]),
    h('div', { class: 'board' }, cols.map(([name, list, st]) => h('div', { class: 'col' },
      h('div', { class: 'col-h' }, h('span', { class: 'dot', style: st ? `--c:${sc(st)}` : null }), name, h('span', { class: 'count' }, list.length)),
      h('div', { class: 'col-b' }, list.length ? list.sort((a, b) => b.updated_at.localeCompare(a.updated_at)).map(ticketCard) : h('div', { class: 'col-empty' }, 'No tickets'))))));
}
function onboarding() {
  return h('div', { class: 'card onboard hero' },
    h('span', { class: 'logo', style: 'width:44px;height:44px;border-radius:12px' }, h('svg:svg', { viewBox: '0 0 24 24', style: 'width:26px;height:26px' }, h('svg:path', { d: 'M5 17V7l7 7 7-7v10' }))),
    h('h2', {}, 'Welcome to Maestro'),
    h('p', {}, 'Orchestrate a team of Claude Code agents on your GitHub repositories. Four steps to get going — or just ask the Assistant to set it up for you.'),
    h('ol', { class: 'steps' },
      h('li', {}, h('div', {}, h('b', {}, 'GitHub access. '), 'In Settings, add a token (scope ', h('code', {}, 'repo'), ', plus ', h('code', {}, 'project'), ' for GitHub boards) or use the ', h('code', {}, 'gh'), ' CLI. Then add your repositories with the switcher (top left).')),
      h('li', {}, h('div', {}, h('b', {}, 'Claude Code. '), '“Account” uses your local ', h('code', {}, 'claude'), ' login (Pro/Max). Or paste a token from ', h('code', {}, 'claude setup-token'), '.')),
      h('li', {}, h('div', {}, h('b', {}, 'Your team. '), 'Tune the agents in Agents, cells, skills (orchestrator, engine, gamedev, gamedesign…).')),
      h('li', {}, h('div', {}, h('b', {}, 'Go. '), 'Label an issue ', h('code', {}, S.settings.trigger_label || 'maestro'), ' (or move a card to the trigger column of a board), use the Planner or the Assistant, then ', h('b', {}, 'start the engine'), '.'))),
    h('div', { class: 'row-btns' },
      btn('chat', 'Ask the Assistant', { class: 'btn primary', onclick: () => go('assistant') }),
      btn('plus', 'Add repositories', { onclick: () => openProjectPicker('repo') }),
      btn('settings', 'Open settings', { onclick: () => go('settings') })));
}

// ───────── TICKET DRAWER
$('#drawer').onmousedown = e => { if (e.target.id === 'drawer') closeDrawer(); };
document.addEventListener('keydown', e => {
  if (e.key === 'Escape') {
    const ov = [...document.querySelectorAll('.overlay')].pop();
    if (ov) return ov._close();
    if (document.querySelector('.proj-menu')) return closeProjectMenu();
    if (document.body.classList.contains('nav-open')) return document.body.classList.remove('nav-open');
    return closeDrawer();
  }
  const typing = /^(INPUT|TEXTAREA|SELECT)$/.test(document.activeElement?.tagName) || document.activeElement?.isContentEditable;
  if (typing || e.metaKey || e.ctrlKey || e.altKey) return;
  if (e.key === '/' && $('#f-search')) { e.preventDefault(); $('#f-search').focus(); }
  else if ((e.key === 't' || e.key === 'T') && !document.querySelector('.overlay')) toggleTheme();
});
function closeDrawer() { $('#drawer').classList.add('hidden'); S.drawerTicket = null; S.drawerRun = null; }
function closeIssueDialog(id) {
  const f = { comment: '', reason: 'completed' };
  const radio = (v, title, sub) => h('label', { class: 'radio-card' },
    h('input', { type: 'radio', name: 'close-reason', value: v, checked: f.reason === v, onchange: () => f.reason = v }), h('div', {}, h('b', {}, title), h('small', {}, sub)));
  const close = openModal({ title: 'Close issue', desc: 'Closes the GitHub issue. You can reopen it later.',
    body: [h('div', { class: 'radio-cards', style: 'margin-bottom:14px' }, radio('completed', 'Completed', 'Done, fixed or resolved'), radio('not_planned', 'Not planned', 'Won’t do, duplicate, stale')),
      h('label', { class: 'f', style: 'margin:0' }, h('span', {}, 'Comment (optional)'), h('textarea', { class: 'prose', rows: 4, oninput: e => f.comment = e.target.value }))],
    foot: [h('button', { class: 'btn ghost', type: 'button', onclick: () => close() }, 'Cancel'),
      btn('close', 'Close issue', { class: 'btn danger-solid', onclick: () => { close(); act(() => api(`/api/tickets/${id}/close`, { method: 'POST', body: { comment: f.comment, reason: f.reason } }), 'Issue closed'); } })] });
}
async function openTicket(id, refresh = false) {
  const d = await api(`/api/tickets/${id}`).catch(e => toast(e.message, 'error'));
  if (!d) return;
  S.drawerTicket = id;
  const t = d.ticket;
  const others = [...S.tickets.values()].filter(x => x.id !== id).sort((a, b) => a.number - b.number);
  const A = (ic, label, action, body, cls = '') => btn(ic, label, { class: `btn sm ${cls}`, onclick: () => act(() => api(`/api/tickets/${id}/${action}`, { method: 'POST', body }), `${label} ✓`) });
  const actions = [];
  if (t.state === 'backlog') actions.push(A('play', 'Start', 'start', null, 'primary'));
  if (['needs_human', 'paused', 'backlog', 'blocked', 'ready'].includes(t.state)) actions.push(A('retry', 'Retriage', 'retriage'));
  if (['needs_human', 'paused', 'awaiting_merge'].includes(t.state) && t.agent) actions.push(A('retry', 'Retry agent', 'retry'));
  if (t.state === 'paused') actions.push(A('play', 'Resume', 'resume'));
  if (['running', 'reviewing', 'triage'].includes(t.state)) actions.push(A('stop', 'Cancel run', 'pause', null, 'danger'));
  else if (!['done', 'paused'].includes(t.state)) actions.push(A('pause', 'Pause', 'pause'));
  if (t.pr_number && t.state !== 'done') actions.push(A('merge', 'Merge PR', 'merge', null, 'primary'));
  if (t.state !== 'backlog' && t.state !== 'done') actions.push(A('reset', 'Reset', 'reset', null, 'danger'));
  if (t.gh_state === 'CLOSED' || t.state === 'done') actions.push(A('reopen', 'Reopen', 'reopen'));
  else actions.push(btn('close', 'Close issue', { class: 'btn sm danger', onclick: () => closeIssueDialog(id) }));

  const drawerRun = refresh && S.drawerRun ? S.drawerRun : (d.runs[0]?.id || null);
  S.drawerRun = drawerRun;
  const logBox = h('div', { class: 'log log-inline', id: 'log-drawer' });
  const inner = $('#drawer-inner'), keep = refresh ? inner.scrollTop : 0;
  put(inner,
    h('div', { class: 'dw-head' }, icon('issue', 'sm'), h('span', { class: 'crumb' }, `${t.repo}#${t.number}`), stateBadge(t.state), h('span', { class: 'grow' }),
      h('a', { class: 'btn sm', href: t.url, target: '_blank', rel: 'noopener' }, icon('github', 'sm'), lbl('Issue')),
      t.pr_url ? h('a', { class: 'btn sm', href: t.pr_url, target: '_blank', rel: 'noopener' }, icon('pr', 'sm'), `#${t.pr_number}`) : null,
      h('button', { class: 'icon-btn sm', type: 'button', title: 'Close (Esc)', onclick: closeDrawer }, icon('x'))),
    h('div', { class: 'dw-body' },
      h('h2', {}, t.title),
      h('div', { class: 'props' }, assignees(t), t.labels.map(l => h('span', { class: 'pill' }, icon('tag'), l)),
        h('span', { class: 'pill', title: 'attempts' }, icon('retry'), `${t.attempts} attempts`), h('span', { class: 'pill', title: 'review rounds' }, icon('pr'), `${t.review_rounds} reviews`),
        h('span', { class: 'pill mono' }, money(t.cost_usd)),
        t.branch ? h('span', { class: 'pill mono' }, icon('branch'), t.branch) : null),
      t.note ? h('div', { class: 'note lg' }, icon('alert', 'sm'), h('div', {}, t.note)) : null,
      h('div', { class: 'actions' }, actions),
      h('div', { class: 'form-card' }, h('div', { class: 'grid2' },
        h('div', {},
          h('label', { class: 'f' }, h('span', {}, 'Assigned to (agent or cell)'), h('select', { onchange: e => act(() => api(`/api/tickets/${id}/assign`, { method: 'POST', body: { agent: e.target.value } }), 'Assigned') },
            assigneeOptions(t.agent, '— orchestrator decides —'))),
          h('label', { class: 'f' }, h('span', {}, 'Helpers'), h('select', { multiple: true, size: 3, onchange: e => act(() => api(`/api/tickets/${id}/helpers`, { method: 'POST', body: { helpers: [...e.target.selectedOptions].map(o => o.value) } }), 'Helpers saved') },
            workersList().map(a => h('option', { value: a.name, selected: (t.helpers || []).includes(a.name) }, a.name))), h('small', {}, 'Extra agents. Ctrl/Cmd-click to select several.'))),
        h('label', { class: 'f' }, h('span', {}, 'Depends on'), h('select', { multiple: true, size: 6, onchange: e => act(() => api(`/api/tickets/${id}/deps`, { method: 'POST', body: { deps: [...e.target.selectedOptions].map(o => +o.value) } }), 'Dependencies saved') },
          others.map(o => h('option', { value: o.id, selected: t.deps.includes(o.id) }, `#${o.number} ${o.title.slice(0, 50)}`))), h('small', {}, 'Ctrl/Cmd-click to select several.')))),
      h('div', { class: 'section' }, h('h4', {}, 'Description'), h('div', { class: 'body-md' }, t.body || '(empty)')),
      h('div', { class: 'section' }, h('h4', {}, 'State history'), d.transitions.length ? h('div', { class: 'timeline' },
        d.transitions.slice().reverse().map(x => h('div', { class: 'ti', style: `--c:${sc(x.to)}` }, h('b', {}, STATE_LABEL[x.to]), h('span', { class: 'muted' }, `from ${STATE_LABEL[x.from]} · ${x.reason} · ${time(x.ts)}`)))) : h('div', { class: 'muted small' }, 'No transitions yet')),
      h('div', { class: 'section' }, h('h4', {}, 'Runs', d.runs.length ? h('span', { class: 'muted', style: 'text-transform:none;letter-spacing:0;font-weight:500' }, `(${d.runs.length})`) : null),
        h('div', { class: 'run-tabs' }, d.runs.map(r => h('button', { type: 'button', class: `run-tab ${r.id === drawerRun ? 'on' : ''}`, onclick: () => { S.drawerRun = r.id; openTicket(id, true); } },
          r.status === 'running' ? h('span', { class: 'spin st-running' }) : h('span', { class: `st st-${r.status}` }), `${r.kind} · ${r.agent}`, h('span', { class: 'muted mono' }, money(r.cost_usd))))),
        drawerRun ? logBox : h('div', { class: 'muted small' }, 'No runs yet'))));
  $('#drawer').classList.remove('hidden');
  inner.scrollTop = keep;
  if (drawerRun) loadLog(drawerRun, logBox);
}

function newTicket() {
  const repos = repoChoices();
  const f = { repo: S.planRepo || repos[0] || '', title: '', body: '', agent: '', helpers: [], deps: [], start: false };
  const others = [...S.tickets.values()].filter(t => t.state !== 'done').sort((a, b) => a.number - b.number);
  S.drawerTicket = null;
  put($('#drawer-inner'),
    h('div', { class: 'dw-head' }, icon('plus', 'sm'), h('b', {}, 'New ticket'), h('span', { class: 'grow' }), h('button', { class: 'icon-btn sm', type: 'button', title: 'Close (Esc)', onclick: closeDrawer }, icon('x'))),
    h('div', { class: 'dw-body' },
      h('p', { class: 'muted', style: 'margin:0 0 18px' }, S.source?.mode === 'repo' ? 'Creates a GitHub issue in the repository (queued = labelled for Maestro).' : 'Creates a GitHub issue and adds it to the project.'),
      h('label', { class: 'f' }, h('span', {}, 'Repository'), h('input', { list: 'nt-repos', value: f.repo, placeholder: 'owner/repo', oninput: e => f.repo = e.target.value }), h('datalist', { id: 'nt-repos' }, repos.map(r => h('option', { value: r })))),
      h('label', { class: 'f' }, h('span', {}, 'Title'), h('input', { placeholder: 'Short, imperative summary', autofocus: true, oninput: e => f.title = e.target.value })),
      h('label', { class: 'f' }, h('span', {}, 'Description'), h('textarea', { class: 'prose', rows: 9, placeholder: 'Markdown. Context, acceptance criteria…', oninput: e => f.body = e.target.value })),
      h('div', { class: 'grid2' },
        h('div', {},
          h('label', { class: 'f' }, h('span', {}, 'Assigned to'), h('select', { onchange: e => f.agent = e.target.value }, assigneeOptions('', '— orchestrator decides —'))),
          h('label', { class: 'f' }, h('span', {}, 'Helpers'), h('select', { multiple: true, size: 3, onchange: e => f.helpers = [...e.target.selectedOptions].map(o => o.value) }, workersList().map(a => h('option', { value: a.name }, a.name))))),
        h('label', { class: 'f' }, h('span', {}, 'Depends on'), h('select', { multiple: true, size: 7, onchange: e => f.deps = [...e.target.selectedOptions].map(o => +o.value) }, others.map(o => h('option', { value: o.id }, `#${o.number} ${o.title.slice(0, 50)}`))))),
      h('label', { class: 'switch' }, h('input', { type: 'checkbox', onchange: e => f.start = e.target.checked }), h('span', { class: 'track' }), S.source?.mode === 'repo' ? `Queue now (label it ${S.settings.trigger_label || 'maestro'})` : 'Queue now (put it in the trigger column)'),
      h('div', { class: 'actions' },
        btn('plus', 'Create issue', { class: 'btn primary', onclick: () => act(async () => { const t = await api('/api/tickets', { method: 'POST', body: f }); S.tickets.set(t.id, t); openTicket(t.id); render(); }, 'Issue created') }),
        h('button', { class: 'btn ghost', type: 'button', onclick: closeDrawer }, 'Cancel'))));
  $('#drawer').classList.remove('hidden');
  $('#drawer-inner').scrollTop = 0;
  setTimeout(() => $('#drawer-inner [autofocus]')?.focus(), 50);
}

// ───────── LOGS
async function loadLog(runId, box) {
  if (!box) return;
  const lines = await api(`/api/runs/${runId}/logs`).catch(() => []);
  box.dataset.run = runId;
  put(box, lines.map(logLine));
  box.scrollTop = box.scrollHeight;
}
function logLine(l) { return h('div', { class: `l k-${l.kind}` }, h('span', { class: 'ts' }, time(l.ts)), l.kind === 'tool' ? '⚒ ' : l.kind === 'result' ? '✔ ' : '', l.text); }
function appendLog(l) {
  for (const box of document.querySelectorAll('.log')) {
    if (+box.dataset.run !== l.run_id) continue;
    const stick = box.scrollHeight - box.scrollTop - box.clientHeight < 40;
    box.append(logLine(l));
    if (stick) box.scrollTop = box.scrollHeight;
  }
}

function viewRuns() {
  const tk = id => S.tickets.get(id);
  if (!S.selRun && S.runs[0]) S.selRun = S.runs[0].id;
  const sel = S.runs.find(r => r.id === S.selRun);
  return h('div', { class: 'fill' }, h('div', { class: 'runs' },
    h('div', { class: 'runlist' }, S.runs.length ? S.runs.map(r => h('div', { class: `run ${r.id === S.selRun ? 'sel' : ''}`, onclick: () => { S.selRun = r.id; render(); } },
      h('div', { class: 'r1' }, r.status === 'running' ? h('span', { class: 'spin st-running' }) : h('span', { class: `st st-${r.status}`, title: r.status }), h('b', {}, r.kind), agentChip(r.agent), h('span', { class: 'cost' }, money(r.cost_usd))),
      h('div', { class: 'r2' }, tk(r.ticket_id) ? `#${tk(r.ticket_id).number} ${tk(r.ticket_id).title}` : r.kind === 'plan' ? 'planner' : '—', ' · ', ago(r.started_at), r.turns ? ` · ${r.turns} turns` : '')))
      : h('div', { class: 'empty' }, icon('runs'), h('b', {}, 'No runs yet'), 'Agent sessions show up here as soon as the engine picks up a ticket.')),
    h('div', { class: 'log-pane' },
      sel ? h('div', { class: 'h' }, h('div', { style: 'display:flex;align-items:center;gap:8px;min-width:0' },
        h('span', { class: `st st-${sel.status}` }), h('h3', {}, `Run #${sel.id}`), h('span', { class: 'muted' }, `${sel.kind} by`), agentChip(sel.agent)),
        h('div', { class: 'row-btns' }, sel.ticket_id ? btn('issue', 'Open ticket', { class: 'btn sm', onclick: () => openTicket(sel.ticket_id) }) : null,
          sel.status === 'running' ? btn('stop', 'Cancel', { class: 'btn sm danger', onclick: () => act(() => api(`/api/runs/${sel.id}/cancel`, { method: 'POST' }), 'Cancel sent') }) : null)) : null,
      h('div', { class: 'log', id: 'log-main', style: 'flex:1' }))));
}

// ───────── GRAPH (ticket DAG)
function viewGraph() {
  const ts = filtered().filter(t => !S.hideDone || t.state !== 'done');
  const ids = new Set(ts.map(t => t.id));
  const preds = new Map(ts.map(t => [t.id, [...t.deps.filter(d => ids.has(d)), ...(t.parent && ids.has(t.parent) ? [t.parent] : [])]]));
  const layer = new Map(); const visiting = new Set();
  const L = id => { if (layer.has(id)) return layer.get(id); if (visiting.has(id)) return 0; visiting.add(id); const v = Math.max(-1, ...preds.get(id).map(L)) + 1; visiting.delete(id); layer.set(id, v); return v; };
  ts.forEach(t => L(t.id));
  const cols = []; ts.forEach(t => { (cols[layer.get(t.id)] ||= []).push(t); });
  const W = 236, H = 64, GX = 300, GY = 86, pos = new Map();
  cols.forEach((c, i) => c.sort((a, b) => (a.parent || a.id) - (b.parent || b.id) || a.number - b.number).forEach((t, j) => pos.set(t.id, { x: 40 + i * GX, y: 40 + j * GY })));
  const g = h('svg:g', { id: 'gview' });
  for (const t of ts) {
    const b = pos.get(t.id);
    for (const p of preds.get(t.id)) {
      const a = pos.get(p); if (!a) continue;
      const x1 = a.x + W, y1 = a.y + H / 2, x2 = b.x, y2 = b.y + H / 2, mx = (x1 + x2) / 2;
      const live = t.state === 'running' || t.state === 'reviewing';
      g.append(h('svg:path', { class: `edge ${t.parent === p && !t.deps.includes(p) ? 'parent' : ''} ${live ? 'live' : ''}`, d: `M${x1},${y1} C${mx},${y1} ${mx},${y2} ${x2},${y2}`, 'marker-end': 'url(#arr)' }));
    }
  }
  for (const t of ts) {
    const p = pos.get(t.id), running = S.busy.includes(t.id);
    g.append(h('svg:g', { class: `gnode ${running ? 'running' : ''}`, transform: `translate(${p.x},${p.y})`, onclick: () => openTicket(t.id) },
      h('svg:rect', { class: 'box', width: W, height: H, rx: 10, style: `stroke:${sc(t.state)}`, 'stroke-width': 1.5 }),
      h('svg:rect', { x: 10, y: 12, width: 4, height: H - 24, rx: 2, fill: agentColor(t.agent) }),
      h('svg:circle', { cx: W - 14, cy: 15, r: 4, style: `fill:${sc(t.state)}` }),
      h('svg:text', { class: 'g-num', x: 24, y: 20 }, `#${t.number} · ${STATE_LABEL[t.state]}`),
      h('svg:text', { class: 'g-title', x: 24, y: 39 }, t.title.length > 30 ? t.title.slice(0, 29) + '…' : t.title),
      h('svg:text', { class: 'g-agent', x: 24, y: 55, fill: agentColor(t.agent) }, t.agent ? `${cellOf(t.agent) ? '▣' : '●'} ${t.agent}${(t.helpers || []).length ? ` +${t.helpers.length}` : ''}` : '○ unassigned'),
      h('svg:title', {}, `#${t.number} ${t.title}`)));
  }
  const svg = h('svg:svg', {}, h('svg:defs', {}, h('svg:marker', { id: 'arr', viewBox: '0 0 10 10', refX: 9, refY: 5, markerWidth: 7, markerHeight: 7, orient: 'auto-start-reverse' }, h('svg:path', { class: 'arrow-head', d: 'M0,0 L10,5 L0,10 z' }))), g);
  const pz = panZoom(svg, g, 'graphView');
  return h('div', { class: 'fill' },
    filterBar([h('label', { class: 'switch', style: 'margin:0' }, h('input', { type: 'checkbox', checked: S.hideDone, onchange: e => { S.hideDone = e.target.checked; render(); } }), h('span', { class: 'track' }), 'Hide done'),
      h('span', { class: 'spacer' }),
      h('span', { class: 'hint' }, 'Solid = dependency · dashed = parent → sub-ticket')]),
    h('div', { class: 'canvas' }, ts.length ? svg : h('div', { class: 'empty' }, icon('graph'), h('b', {}, 'No tickets'), 'Nothing matches the current filters.'),
      h('div', { class: 'legend' }, Object.keys(STATE_LABEL).map(s => stateBadge(s))),
      ts.length ? canvasTools(pz) : null));
}
function canvasTools(pz) {
  const b = (ic, title, fn) => h('button', { class: 'icon-btn sm', type: 'button', title, onclick: fn }, icon(ic, 'sm'));
  return h('div', { class: 'canvas-tools' }, b('zoomout', 'Zoom out', () => pz.zoom(0.8)), b('zoomin', 'Zoom in', () => pz.zoom(1.25)), b('fit', 'Reset view', pz.reset));
}
function panZoom(svg, g, key) {
  const v = S[key] ||= { x: 0, y: 0, k: 1 };
  const apply = () => g.setAttribute('transform', `translate(${v.x},${v.y}) scale(${v.k})`);
  apply();
  svg.style.touchAction = 'none';
  let drag = null;
  svg.addEventListener('pointerdown', e => { if (e.target.closest('.gnode, .sm-e')) return; drag = { x: e.clientX - v.x, y: e.clientY - v.y }; svg.setPointerCapture(e.pointerId); });
  svg.addEventListener('pointermove', e => { if (drag) { v.x = e.clientX - drag.x; v.y = e.clientY - drag.y; apply(); } });
  svg.addEventListener('pointerup', () => drag = null);
  svg.addEventListener('pointercancel', () => drag = null);
  const zoomAt = (mx, my, f) => { const k2 = Math.min(2.5, Math.max(0.25, v.k * f)); v.x = mx - (mx - v.x) * k2 / v.k; v.y = my - (my - v.y) * k2 / v.k; v.k = k2; apply(); };
  svg.addEventListener('wheel', e => {
    e.preventDefault(); const r = svg.getBoundingClientRect();
    zoomAt(e.clientX - r.left, e.clientY - r.top, e.deltaY < 0 ? 1.1 : 0.9);
  }, { passive: false });
  return {
    zoom: f => { const r = svg.getBoundingClientRect(); zoomAt(r.width / 2, r.height / 2, f); },
    reset: () => { v.x = 0; v.y = 0; v.k = 1; apply(); },
  };
}

// ───────── STATE MACHINE
const SM_POS = { backlog: [74, 300], paused: [74, 520], triage: [262, 300], split: [459, 80], blocked: [459, 190], needs_human: [623, 530],
  ready: [640, 300], running: [828, 300], reviewing: [1017, 150], awaiting_merge: [1017, 450], done: [1197, 300] };
const NW = 140, NH = 54;
function border([cx, cy], dx, dy) { const t = Math.min((NW / 2 + 6) / Math.abs(dx || 1e-9), (NH / 2 + 6) / Math.abs(dy || 1e-9)); return [cx + dx * t, cy + dy * t]; }
function viewMachine() {
  const ts = [...S.tickets.values()];
  const g = h('svg:g', { id: 'smview' });
  const pairs = new Set(S.sm.transitions.map(t => `${t.from}>${t.to}`));
  const edges = h('svg:g', {}), labels = h('svg:g', {});
  for (const tr of S.sm.transitions) {
    const A = SM_POS[tr.from], B = SM_POS[tr.to];
    const dx = B[0] - A[0], dy = B[1] - A[1], len = Math.hypot(dx, dy) || 1, nx = -dy / len, ny = dx / len;
    const off = pairs.has(`${tr.to}>${tr.from}`) ? 22 : 0;
    const [sx, sy] = border(A, dx + nx * off * 2, dy + ny * off * 2), [ex, ey] = border(B, -dx + nx * off * 2, -dy + ny * off * 2);
    const cx = (sx + ex) / 2 + nx * off, cy = (sy + ey) / 2 + ny * off;
    const d = `M${sx},${sy} Q${cx},${cy} ${ex},${ey}`;
    const lab = h('svg:text', { class: 'sm-label', x: cx, y: cy - 5, 'text-anchor': 'middle' }, tr.label);
    const grp = h('svg:g', { class: 'sm-e', onmouseenter: () => lab.classList.add('on'), onmouseleave: () => lab.classList.remove('on') },
      h('svg:path', { d, stroke: 'transparent', 'stroke-width': 14, fill: 'none' }),
      h('svg:path', { class: 'sm-edge', id: `sme-${tr.from}-${tr.to}`, d, 'marker-end': 'url(#arr2)' }), h('svg:title', {}, `${STATE_LABEL[tr.from]} → ${STATE_LABEL[tr.to]} : ${tr.label}`));
    edges.append(grp); labels.append(lab);
  }
  g.append(edges);
  for (const st of S.sm.states) {
    const [x, y] = SM_POS[st] || [0, 0]; const n = ts.filter(t => t.state === st).length;
    g.append(h('svg:g', { class: 'sm-node', id: `smn-${st}`, transform: `translate(${x - NW / 2},${y - NH / 2})` },
      h('svg:rect', { width: NW, height: NH, rx: 12, 'stroke-width': n ? 2 : 1.3,
        style: `stroke:${sc(st)};fill:${n ? `color-mix(in srgb, ${sc(st)} 14%, var(--node))` : 'var(--node)'}` }),
      h('svg:text', { class: 'sm-name', x: NW / 2, y: 24, 'text-anchor': 'middle' }, STATE_LABEL[st]),
      h('svg:text', { class: 'sm-count', x: NW / 2, y: 42, 'text-anchor': 'middle', style: `fill:${n ? sc(st) : 'var(--node-tx-2)'}` }, `${n} ticket${n === 1 ? '' : 's'}`)));
  }
  g.append(labels);
  if (S.smLabels) labels.classList.add('all');
  const svg = h('svg:svg', { viewBox: '0 0 1290 610' }, h('svg:defs', {}, h('svg:marker', { id: 'arr2', viewBox: '0 0 10 10', refX: 9, refY: 5, markerWidth: 7, markerHeight: 7, orient: 'auto-start-reverse' }, h('svg:path', { class: 'arrow-head', d: 'M0,0 L10,5 L0,10 z' }))), g);
  const pz = panZoom(svg, g, 'smView');
  return h('div', { class: 'fill' },
    h('div', { class: 'toolbar' }, h('label', { class: 'switch', style: 'margin:0' }, h('input', { type: 'checkbox', checked: !!S.smLabels, onchange: e => { S.smLabels = e.target.checked; render(); } }), h('span', { class: 'track' }), 'Show all transition labels'),
      h('span', { class: 'spacer' }), h('span', { class: 'hint' }, 'Hover an arrow to see its trigger. Transitions flash live as tickets move.')),
    h('div', { class: 'split' },
      h('div', { class: 'canvas' }, svg, canvasTools(pz)),
      h('div', { class: 'card' }, h('div', { class: 'panel-h' }, icon('bolt', 'sm'), 'Live transitions', h('span', { class: 'live-dot', title: 'live' })),
        h('div', { class: 'feed' }, S.feed.length ? S.feed.map(e => { const t = S.tickets.get(e.ticket_id); return h('div', { class: 'ev', onclick: () => openTicket(e.ticket_id) },
          h('div', { class: 'r' }, h('b', { class: 'mono' }, t ? `#${t.number}` : '?'), stateBadge(e.from), h('span', { class: 'arrow' }, '→'), stateBadge(e.to)), h('div', { class: 'muted' }, e.reason, ' · ', time(e.ts))); })
          : h('div', { class: 'empty', style: 'padding:32px 12px' }, icon('bolt'), 'Waiting for activity…')))));
}
function flashTransition(e) {
  if (S.view !== 'machine') return;
  setTimeout(() => {
    for (const id of [`sme-${e.from}-${e.to}`, `smn-${e.to}`]) { const el = document.getElementById(id); if (el) { el.classList.add('flash'); setTimeout(() => el.classList.remove('flash'), 1400); } }
  }, 150);
}

// ───────── PLANNER
function viewPlanner() {
  const repos = repoChoices();
  S.planRepo ||= repos[0] || '';
  const box = h('div', { style: 'max-width:1100px' },
    h('div', { class: 'card hero', style: 'padding:22px' },
      h('div', { class: 'page-h', style: 'margin-bottom:16px' }, h('div', {}, h('h2', {}, 'Plan a goal'), h('p', {}, 'The orchestrator turns a goal into a dependency graph of GitHub issues. Review and edit before anything is created.'))),
      h('label', { class: 'f' }, h('span', {}, 'Goal'), h('textarea', { id: 'goal', class: 'prose', rows: 5, placeholder: 'e.g. Add a dash ability to the player: design doc with tuning, engine support for i-frames, gameplay implementation, VFX hook.', oninput: e => S.goal = e.target.value }, S.goal || '')),
      h('div', { style: 'display:flex;gap:12px;align-items:flex-end;flex-wrap:wrap' },
        h('label', { class: 'f', style: 'flex:1;min-width:220px;margin:0' }, h('span', {}, 'Repository'), h('input', { id: 'plan-repo', list: 'repos', value: S.planRepo, placeholder: 'owner/repo', oninput: e => S.planRepo = e.target.value }), h('datalist', { id: 'repos' }, repos.map(r => h('option', { value: r })))),
        h('button', { class: 'btn primary', type: 'button', style: 'height:34px', disabled: S.planning, onclick: () => { S.goal = $('#goal').value; S.planning = true; S.plan = null; render(); act(() => api('/api/plan', { method: 'POST', body: { goal: S.goal, repo: S.planRepo } })).then(r => { if (!r) { S.planning = false; render(); } }); } },
          S.planning ? [h('span', { class: 'spin' }), 'Orchestrator is thinking…'] : [icon('planner', 'sm'), 'Generate plan']))));
  if (S.planning) box.append(h('div', { class: 'card', style: 'margin-top:16px;display:flex;align-items:center;gap:10px;color:var(--tx-2)' }, h('span', { class: 'spin', style: 'color:var(--acc)' }), 'Planning… you can follow it live in ', h('a', { href: '#runs', onclick: e => { e.preventDefault(); go('runs'); } }, 'Runs & logs'), '.'));
  if (S.plan) {
    const tk = S.plan.tickets || [];
    box.append(h('div', { class: 'card', style: 'margin-top:16px' },
      h('div', { class: 'h' }, h('h3', {}, `${tk.length} proposed tickets`), S.plan.summary ? h('span', { class: 'muted small', style: 'text-align:right' }, S.plan.summary) : null),
      h('div', { class: 'table-wrap' }, h('table', { class: 'plan-table' }, h('tr', {}, h('th', {}, 'Key'), h('th', { style: 'width:42%' }, 'Title / body'), h('th', {}, 'Agent / cell'), h('th', {}, 'Depends on'), h('th', {})),
        tk.map((t, i) => h('tr', {},
          h('td', { class: 'mono small' }, h('span', { class: 'pill mono' }, t.key)),
          h('td', {}, h('input', { value: t.title, oninput: e => t.title = e.target.value, style: 'margin-bottom:6px;font-weight:550' }), h('textarea', { oninput: e => t.body = e.target.value }, t.body)),
          h('td', {}, h('select', { onchange: e => t.agent = e.target.value }, assigneeOptions(t.agent)), (t.helpers || []).length ? h('div', { class: 'muted small', style: 'margin-top:6px' }, '+ ', t.helpers.join(', ')) : null),
          h('td', { class: 'small mono' }, (t.depends_on || []).length ? (t.depends_on || []).map(k => h('span', { class: 'pill mono', style: 'margin:0 4px 4px 0' }, k)) : h('span', { class: 'muted' }, '—')),
          h('td', {}, h('button', { class: 'icon-btn sm', type: 'button', title: 'Remove', onclick: () => { tk.splice(i, 1); render(); } }, icon('trash', 'sm'))))))),
      h('div', { class: 'actions', style: 'align-items:center;margin-bottom:0' },
        h('label', { class: 'switch', style: 'margin:0 auto 0 0' }, h('input', { type: 'checkbox', id: 'plan-start', checked: true }), h('span', { class: 'track' }), 'Start immediately (trigger column & queue)'),
        h('button', { class: 'btn ghost', type: 'button', onclick: () => { S.plan = null; render(); } }, 'Discard'),
        btn('github', `Create ${tk.length} GitHub issues`, { class: 'btn primary', onclick: () => act(async () => { await api('/api/plan/apply', { method: 'POST', body: { repo: S.planRepo, tickets: tk, start: $('#plan-start').checked } }); S.plan = null; render(); }, 'Issues created on GitHub') }))));
  }
  return box;
}

// ───────── AGENTS & SKILLS
const AGENT_TEMPLATE = `---
name: new-agent
description: What this specialist does (used by the orchestrator to route tickets).
role: worker
model: sonnet
color: '#38bdf8'
labels: []
tools: [Read, Edit, Write, Glob, Grep, Bash]
skills: [commit-conventions]
effort: ''
---

You are a … Describe expertise, rules and conventions here.
`;
const CELL_TEMPLATE = `---
name: new-cell
description: What this group of agents is good at (used by the orchestrator for routing).
lead: gamedev
members: [gamedesign, engine]
mode: delegate        # delegate = lead + members as subagents in one session | pipeline = lead, then each member in order
color: '#eab308'
labels: []
---

Instructions for the lead (delegate) or for every step (pipeline).
`;
const SKILL_TEMPLATE = n => `---
name: ${n}
description: When Claude should use this skill.
---

# ${n}

Instructions…
`;
async function newSkill() {
  const n = await promptDialog({ title: 'New skill', label: 'Skill name', placeholder: 'e.g. unit-testing', desc: 'Lowercase letters, digits and dashes.', ok: 'Create' });
  if (n) { S.editing = { kind: 'skill', name: n.trim(), isNew: true, raw: SKILL_TEMPLATE(n.trim()) }; render(); }
}
function viewAgents() {
  if (S.editing) return agentEditor();
  const stat = n => { const rs = S.runs.filter(r => r.agent === n); return { n: rs.length, cost: rs.reduce((a, r) => a + r.cost_usd, 0), live: rs.filter(r => r.status === 'running').length }; };
  const foot = st => h('div', { class: 'foot' }, icon('runs', 'sm'), `${st.n} run${st.n === 1 ? '' : 's'}`, h('span', { style: 'margin-left:auto', class: 'mono' }, money(st.cost)));
  return h('div', {},
    h('div', { class: 'sec-h' }, h('h2', {}, 'Agents'), h('span', { class: 'count' }, S.agents.length),
      btn('plus', 'New agent', { class: 'btn primary sm', onclick: () => { S.editing = { kind: 'agent', name: null, raw: AGENT_TEMPLATE }; render(); } })),
    h('div', { class: 'agents' }, S.agents.map(a => { const st = stat(a.name); return h('div', { class: 'acard', tabindex: 0, style: `--ac:${a.color || '#7c8799'}`, onclick: () => editAgent(a.name), onkeydown: e => { if (e.key === 'Enter') editAgent(a.name); } },
      h('div', { class: 'row' }, h('span', { class: 'avatar' }, initial(a.name)), h('div', { style: 'min-width:0' }, h('h3', {}, a.name), h('div', { class: 'muted small' }, a.model || 'default model')),
        st.live ? h('span', { class: 'spin', style: 'color:var(--ac);margin-left:auto' }) : null, h('span', { class: 'pill', style: st.live ? 'margin-left:6px' : null }, a.role || 'worker')),
      h('p', {}, a.description),
      (a.labels.length || a.skills.length) ? h('div', { class: 'tags' }, a.labels.map(l => h('span', { class: 'pill' }, icon('tag'), l)), a.skills.map(s => h('span', { class: 'pill' }, icon('skill'), s))) : null,
      foot(st)); })),
    h('div', { class: 'sec-h' }, h('h2', {}, 'Cells'), h('span', { class: 'count' }, (S.cells || []).length),
      btn('plus', 'New cell', { class: 'btn sm', onclick: () => { S.editing = { kind: 'cell', name: null, raw: CELL_TEMPLATE }; render(); } })),
    (S.cells || []).length ? h('div', { class: 'agents' }, S.cells.map(c => { const st = stat(c.name); return h('div', { class: 'acard', tabindex: 0, style: `--ac:${c.color || '#eab308'}`, onclick: () => editCell(c.name), onkeydown: e => { if (e.key === 'Enter') editCell(c.name); } },
      h('div', { class: 'row' }, h('span', { class: 'avatar' }, icon('cell')), h('div', { style: 'min-width:0' }, h('h3', {}, c.name), h('div', { class: 'muted small' }, `${[c.lead, ...c.members].filter(Boolean).length} agents`)), h('span', { class: 'pill' }, c.mode || 'delegate')),
      h('p', {}, c.description),
      h('div', { class: 'tags' }, c.lead ? h('span', { class: 'agent-chip', style: `--ac:${agentColor(c.lead)}`, title: 'lead' }, '★ ', c.lead) : null,
        c.members.map(m => agentChip(m)), c.labels.map(l => h('span', { class: 'pill' }, icon('tag'), l))),
      h('div', { class: 'muted small' }, c.mode === 'pipeline' ? `Order: ${[c.lead, ...c.members].filter(Boolean).join(' → ')}` : `${c.lead} delegates to ${c.members.join(', ') || 'nobody'}`),
      foot(st)); }))
      : h('div', { class: 'card empty' }, icon('cell'), h('b', {}, 'No cells yet'), 'A cell groups agents that work together on the same ticket.'),
    h('div', { class: 'sec-h' }, h('h2', {}, 'Skills'), h('span', { class: 'count' }, S.skills.length), btn('plus', 'New skill', { class: 'btn sm', onclick: newSkill })),
    h('div', { class: 'agents' }, S.skills.map(s => { const users = S.agents.filter(a => a.skills.includes(s.name)); return h('div', { class: 'acard', tabindex: 0, style: '--ac:var(--acc)', onclick: () => editSkill(s.name), onkeydown: e => { if (e.key === 'Enter') editSkill(s.name); } },
      h('div', { class: 'row' }, h('span', { class: 'avatar' }, icon('skill')), h('h3', {}, s.name)), h('p', {}, s.description),
      h('div', { class: 'foot' }, users.length ? [h('span', {}, 'Used by'), h('div', { class: 'tags' }, users.map(a => agentChip(a.name)))] : 'Not used by any agent')); })));
}
async function editAgent(name) { const list = await api('/api/agents'); const x = list.find(v => v.agent.name === name); S.editing = { kind: 'agent', name, raw: x.raw }; render(); }
async function editCell(name) { const list = await api('/api/cells'); const x = list.find(v => v.cell.name === name); S.editing = { kind: 'cell', name, raw: x.raw }; render(); }
async function editSkill(name) { const list = await api('/api/skills'); const x = list.find(v => v.name === name); S.editing = { kind: 'skill', name, raw: x.content }; render(); }
function agentEditor() {
  const e = S.editing, isAgent = e.kind === 'agent', isCell = e.kind === 'cell';
  const ta = h('textarea', { spellcheck: 'false', oninput: ev => e.raw = ev.target.value }, e.raw);
  const save = () => act(async () => {
    if (isAgent || isCell) {
      const nm = (e.raw.match(/^name:\s*(.+)$/m) || [])[1]?.trim().replace(/^['"]|['"]$/g, '');
      if (!nm) throw new Error('front-matter needs a name');
      await api(`/api/${isCell ? 'cells' : 'agents'}/${encodeURIComponent(e.name || nm)}`, { method: 'PUT', body: { raw: e.raw } });
    } else await api(`/api/skills/${encodeURIComponent(e.name)}`, { method: 'PUT', body: { content: e.raw } });
    S.editing = null; await load();
  }, 'Saved');
  const del = async () => {
    if (!await confirmDialog(`This permanently deletes the ${e.kind} “${e.name}”.`, { title: `Delete ${e.kind}?`, ok: 'Delete', danger: true })) return;
    act(async () => { await api(`/api/${isAgent ? 'agents' : isCell ? 'cells' : 'skills'}/${encodeURIComponent(e.name)}`, { method: 'DELETE' }); S.editing = null; await load(); }, 'Deleted');
  };
  const kindLabel = isAgent ? 'Agent' : isCell ? 'Cell' : 'Skill';
  return h('div', {},
    h('div', { class: 'page-h' },
      h('div', {}, h('div', { class: 'muted small', style: 'display:flex;align-items:center;gap:6px' },
        h('a', { href: '#agents', onclick: ev => { ev.preventDefault(); S.editing = null; render(); } }, 'Agents, cells, skills'), '/', kindLabel),
        h('h2', {}, e.name && !e.isNew ? e.name : e.isNew ? e.name : `New ${e.kind}`)),
      h('div', { class: 'row-btns' }, e.name && !e.isNew ? btn('trash', 'Delete', { class: 'btn danger', onclick: del }) : null,
        h('button', { class: 'btn ghost', type: 'button', onclick: () => { S.editing = null; render(); } }, 'Cancel'), btn('check', 'Save', { class: 'btn primary', onclick: save }))),
    h('div', { class: 'editor' }, ta,
      h('div', { class: 'card small' }, isAgent ? [
        h('h3', {}, 'Front-matter'),
        h('p', {}, h('code', {}, 'role'), ': ', h('b', {}, 'worker'), ' (codes tickets), ', h('b', {}, 'orchestrator'), ' (triage & planning, read-only), ', h('b', {}, 'reviewer'), ' (reviews PRs, read-only).'),
        h('p', {}, h('code', {}, 'model'), ': ', h('code', {}, 'opus'), ' / ', h('code', {}, 'sonnet'), ' / ', h('code', {}, 'haiku'), ' or a full model id. Empty = default from Settings.'),
        h('p', {}, h('code', {}, 'labels'), ': GitHub labels routed straight to this agent (skips triage). ', h('code', {}, 'agent:<name>'), ' always works.'),
        h('p', {}, h('code', {}, 'tools'), ': Claude Code tools allowed (', h('code', {}, 'Bash(npm test:*)'), ' patterns work). Empty = defaults.'),
        h('p', {}, h('code', {}, 'skills'), ': skills from the library, mounted through a generated Claude Code plugin (', h('code', {}, '--plugin-dir'), ').'),
        h('p', {}, 'The markdown body is appended to Claude Code\'s system prompt.'),
        h('p', {}, 'Skills available: ', S.skills.map(s => h('code', { style: 'margin:2px' }, s.name)))
      ] : isCell ? [
        h('h3', {}, 'Cell'),
        h('p', {}, 'A cell is a group of agents assigned to a ticket as one unit. Assign it like an agent (drawer, planner, ', h('code', {}, 'cell:<name>'), ' label, or the orchestrator).'),
        h('p', {}, h('code', {}, 'mode: delegate'), ': the ', h('code', {}, 'lead'), ' runs one Claude Code session where the ', h('code', {}, 'members'), ' are declared as subagents (', h('code', {}, '--agents'), '). The lead delegates and integrates. Skills of all members are mounted.'),
        h('p', {}, h('code', {}, 'mode: pipeline'), ': lead, then each member in order, one session each, on the same branch. Each step gets the previous summaries.'),
        h('p', {}, h('code', {}, 'labels'), ': GitHub labels routed to this cell.'),
        h('p', {}, 'Agents available: ', workersList().map(a => h('code', { style: 'margin:2px' }, a.name))),
      ] : [h('h3', {}, 'Skill'), h('p', {}, 'Standard Claude Code SKILL.md: front-matter ', h('code', {}, 'name'), ' + ', h('code', {}, 'description'), ' (tells Claude when to load it), then instructions. Attach it to agents via their ', h('code', {}, 'skills'), ' list.')])));
}

// repos of the current board first, then the ones GitHub says we can push to (fetched once, best effort)
function repoChoices() {
  if (S.repoList === undefined || S.repoList === null) { S.repoList = []; api('/api/github/repos').then(r => { S.repoList = r; }).catch(() => {}); }
  const own = [...new Set([...S.tickets.values()].map(t => t.repo))].sort();
  return [...own, ...S.repoList.filter(r => !own.includes(r))];
}

// ───────── PROJECTS (switcher, like GitHub Desktop)
const activeProject = () => (S.projects || []).find(p => p.key === S.activeProject);
const profileRepos = p => String(p.repos || '').split(/[\s,]+/).filter(r => r.includes('/'));
function projectSubtitle(p) {
  if (p.source !== 'project' && profileRepos(p).length) { const r = profileRepos(p); return r[0] + (r.length > 1 ? ` +${r.length - 1}` : '') + (p.project_number > 0 ? ' · board' : ''); }
  return `${p.project_owner} · board #${p.project_number}`;
}

function renderProjectSwitcher() {
  const cur = activeProject();
  const open = !!document.querySelector('.proj-menu');
  const btnEl = h('button', { class: 'proj-switch', type: 'button', 'aria-haspopup': 'menu', onclick: e => { e.stopPropagation(); toggleProjectMenu(); } },
    h('span', { class: 'ps-ico' }, cur ? initial(cur.name) : icon('folder', 'sm')),
    h('span', { class: 'ps-txt' }, h('small', {}, 'Project'), h('b', {}, cur ? cur.name : 'No project')),
    h('span', { class: 'ps-caret' }, icon('updown', 'sm')));
  put($('#proj-switch'), btnEl);
  if (open) toggleProjectMenu();
}

function closeProjectMenu() { document.querySelector('.proj-menu')?.remove(); }
function toggleProjectMenu() {
  if (document.querySelector('.proj-menu')) return closeProjectMenu();
  const items = (S.projects || []).map(p => h('div', { class: 'pm-item' + (p.key === S.activeProject ? ' on' : ''), onclick: () => { closeProjectMenu(); if (p.key !== S.activeProject) switchProject(p.key); } },
    h('span', { class: 'pm-check' }, p.key === S.activeProject ? icon('check', 'sm') : null),
    h('span', { class: 'pm-name' }, h('b', {}, p.name), h('small', {}, projectSubtitle(p)))));
  const menu = h('div', { class: 'proj-menu', role: 'menu', onclick: e => e.stopPropagation() },
    h('div', { class: 'pm-label' }, 'Projects'),
    items.length ? items : h('div', { class: 'pm-empty' }, 'No project yet'),
    h('div', { class: 'pm-sep' }),
    h('div', { class: 'pm-item', onclick: () => { closeProjectMenu(); openProjectPicker(); } }, h('span', { class: 'pm-check' }, icon('plus', 'sm')), h('span', { class: 'pm-name' }, h('b', {}, 'Add project…'))),
    h('div', { class: 'pm-item', onclick: () => { closeProjectMenu(); go('assistant'); } }, h('span', { class: 'pm-check' }, icon('chat', 'sm')), h('span', { class: 'pm-name' }, h('b', {}, 'Create with the Assistant…'))),
    h('div', { class: 'pm-item', onclick: () => { closeProjectMenu(); goSettings('project'); } }, h('span', { class: 'pm-check' }, icon('settings', 'sm')), h('span', { class: 'pm-name' }, h('b', {}, 'Project settings'))));
  $('#proj-switch').append(menu);
}
document.addEventListener('click', closeProjectMenu);

function goSettings(tab) {
  S.settingsTab = tab || S.settingsTab;
  go('settings');
}

async function switchProject(key) {
  const r = await act(() => api('/api/projects/switch', { method: 'POST', body: { key } }));
  if (!r) return;
  S.draft = null; S.filter.repo = ''; S.repoList = null;
  await load();
  toast(`Switched to ${activeProject()?.name || key} — engine paused`, 'ok');
}

async function openProjectPicker(tab = 'repo') {
  const known = new Set((S.projects || []).map(p => p.key));
  const st = { tab, repos: null, boards: null, q: '', err: '', sel: [], name: '', owner: '', number: '', manual: '' };
  const body = h('div', {});
  let close = () => {};
  const done = async r => { if (!r) return; close(); S.draft = null; S.filter.repo = ''; S.repoList = null; await load(); toast(`Switched to ${activeProject()?.name || 'the new project'} — engine paused`, 'ok'); };
  const addRepos = () => {
    const repos = [...st.sel, ...st.manual.split(/[\s,]+/).filter(r => r.includes('/'))];
    if (!repos.length) return toast('Pick at least one repository', 'warn');
    return act(() => api('/api/projects/add', { method: 'POST', body: { kind: 'repo', repos, name: st.name } })).then(done);
  };
  const addBoard = (owner, number, title) => act(() => api('/api/projects/add', { method: 'POST', body: { owner, number: +number, title } })).then(done);
  const fetchTab = async () => {
    st.err = '';
    try {
      if (st.tab === 'repo' && st.repos === null) st.repos = await api('/api/github/repos');
      if (st.tab === 'project' && st.boards === null) st.boards = await api('/api/github/projects');
    } catch (e) { st.err = e.message; if (st.tab === 'repo') st.repos = []; else st.boards = []; }
    draw();
  };
  const filterInput = placeholder => h('div', { class: 'search', style: 'display:flex' }, icon('search', 'sm'),
    h('input', { placeholder, value: st.q, autofocus: true, oninput: e => { st.q = e.target.value; draw(); const i = body.querySelector('.search input'); i.focus(); i.setSelectionRange(99, 99); } }));
  const loading = list => list === null ? h('div', { class: 'muted pp-note' }, st.err ? '' : [h('span', { class: 'spin' }), 'Loading from GitHub…']) : null;
  const errBox = () => st.err ? h('div', { class: 'pp-note', style: 'color:var(--err)' }, icon('alert', 'sm'), st.err) : null;
  const draw = () => {
    const q = st.q.toLowerCase();
    const tabs = h('div', { class: 'seg', style: 'margin-bottom:12px' },
      h('button', { type: 'button', class: st.tab === 'repo' ? 'on' : '', onclick: () => { st.tab = 'repo'; st.q = ''; draw(); fetchTab(); } }, icon('repo', 'sm'), 'Repositories'),
      h('button', { type: 'button', class: st.tab === 'project' ? 'on' : '', onclick: () => { st.tab = 'project'; st.q = ''; draw(); fetchTab(); } }, icon('board', 'sm'), 'GitHub Project board'));
    if (st.tab === 'repo') {
      const list = (st.repos || []).filter(r => !q || r.toLowerCase().includes(q));
      put(body, tabs,
        h('p', { class: 'small muted', style: 'margin:0 0 10px' }, 'Maestro watches the open issues of these repositories. An issue labelled ', h('code', {}, S.settings.trigger_label || 'maestro'), ' is picked up; progress is shown with ', h('code', {}, 'maestro:*'), ' labels. The first one is the primary repository.'),
        filterInput('Filter your repositories…'),
        h('div', { class: 'pp-list' }, loading(st.repos), errBox(),
          list.slice(0, 200).map(r => {
            const i = st.sel.indexOf(r);
            return h('div', { class: 'pm-item' + (i >= 0 ? ' on' : '') + (known.has(`repo:${r.toLowerCase()}`) ? ' dim' : ''), onclick: () => { if (i >= 0) st.sel.splice(i, 1); else st.sel.push(r); draw(); } },
              h('span', { class: 'pm-check' }, i >= 0 ? (i === 0 ? h('span', { class: 'pill', title: 'primary' }, '1') : icon('check', 'sm')) : null),
              h('span', { class: 'pm-name' }, h('b', {}, r)),
              known.has(`repo:${r.toLowerCase()}`) ? h('span', { class: 'pill' }, 'project exists') : null);
          }),
          st.repos && !list.length ? h('div', { class: 'muted pp-note' }, 'Nothing matches.') : null),
        h('div', { class: 'grid2', style: 'margin-top:12px' },
          h('label', { class: 'f' }, h('span', {}, 'Other repositories (owner/name)'), h('input', { value: st.manual, placeholder: 'acme/engine, acme/game', oninput: e => st.manual = e.target.value })),
          h('label', { class: 'f' }, h('span', {}, 'Project name (optional)'), h('input', { value: st.name, placeholder: 'My game', oninput: e => st.name = e.target.value }))),
        h('div', { class: 'row-btns', style: 'justify-content:space-between;align-items:center' },
          h('span', { class: 'small muted' }, st.sel.length ? `${st.sel.length} selected — primary: ${st.sel[0]}` : 'Click to select, in order of importance.'),
          btn('plus', 'Add project', { class: 'btn primary', onclick: addRepos })),
        h('p', { class: 'small muted', style: 'margin:12px 0 0' }, 'Need a new repository or a GitHub board? ', h('a', { href: '#assistant', onclick: e => { e.preventDefault(); close(); go('assistant'); } }, 'Ask the Assistant'), ' — it can create and wire them.'));
    } else {
      const list = (st.boards || []).filter(p => !q || `${p.owner} ${p.title}`.toLowerCase().includes(q));
      put(body, tabs,
        h('p', { class: 'small muted', style: 'margin:0 0 10px' }, 'The board is the source of truth: cards moved to the trigger column are picked up. Needs the token scope ', h('code', {}, 'project'), '. (To keep repositories as the source and only show a board, attach it as a mirror in Settings → Project.)'),
        filterInput('Filter your GitHub projects…'),
        h('div', { class: 'pp-list' }, loading(st.boards), errBox(),
          list.map(p => h('div', { class: 'pm-item' + (known.has(`${p.owner.toLowerCase()}/${p.number}`) ? ' dim' : ''), onclick: () => addBoard(p.owner, p.number, p.title) },
            h('span', { class: 'ps-ico', style: 'width:28px;height:28px' }, initial(p.title)),
            h('span', { class: 'pm-name' }, h('b', {}, p.title), h('small', {}, `${p.owner} · #${p.number}${p.description ? ' — ' + p.description : ''}`)),
            known.has(`${p.owner.toLowerCase()}/${p.number}`) ? h('span', { class: 'pill' }, 'added') : null)),
          st.boards && !list.length ? h('div', { class: 'muted pp-note' }, 'Nothing matches.') : null),
        h('details', { class: 'pp-manual' }, h('summary', {}, 'Not listed? Enter it manually'),
          h('div', { class: 'grid2', style: 'margin-top:10px' },
            h('label', { class: 'f' }, h('span', {}, 'Owner (user or org)'), h('input', { value: st.owner, oninput: e => st.owner = e.target.value })),
            h('label', { class: 'f' }, h('span', {}, 'Project number'), h('input', { type: 'number', value: st.number, oninput: e => st.number = e.target.value }))),
          btn('plus', 'Add project', { class: 'btn primary', onclick: () => addBoard(st.owner, st.number) })));
    }
  };
  close = openModal({ title: 'Add a project', desc: 'Switching pauses the engine. Each project keeps its own board, settings and tickets.', body, width: 620 });
  draw();
  fetchTab();
}

// ───────── MARKDOWN (assistant answers) — escapes everything, then a small safe subset
const escHtml = v => String(v).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
function mdInline(src) {
  return String(src).split(/(`[^`\n]+`)/).map(part => {
    if (/^`[^`]+`$/.test(part)) return `<code>${escHtml(part.slice(1, -1))}</code>`;
    return escHtml(part)
      .replace(/\[([^\]]+)\]\((https?:\/\/[^)\s]+)\)/g, '<a href="$2" target="_blank" rel="noopener">$1</a>')
      .replace(/(^|[\s(])(https?:\/\/[^\s<)]+[^\s<).,;:!?'"])/g, '$1<a href="$2" target="_blank" rel="noopener">$2</a>')
      .replace(/\*\*([^*\n]+)\*\*/g, '<b>$1</b>')
      .replace(/(^|[^\w*])\*([^*\n]+)\*(?!\w)/g, '$1<i>$2</i>')
      .replace(/(^|[^\w])_([^_\n]+)_(?!\w)/g, '$1<i>$2</i>')
      .replace(/~~([^~\n]+)~~/g, '<s>$1</s>');
  }).join('');
}
function mdToHtml(src) {
  const lines = String(src || '').replace(/\r/g, '').split('\n');
  const out = [];
  const isList = l => /^\s*([-*+]|\d+[.)])\s+/.test(l);
  const isTable = (l, n) => /^\s*\|.*\|\s*$/.test(l) && n != null && /^\s*\|?\s*:?-{2,}/.test(n);
  const cells = l => l.trim().replace(/^\||\|$/g, '').split('|').map(c => c.trim());
  for (let i = 0; i < lines.length;) {
    const l = lines[i];
    if (/^\s*```/.test(l)) {
      const buf = []; i++;
      while (i < lines.length && !/^\s*```/.test(lines[i])) buf.push(lines[i++]);
      i++; out.push(`<pre><code>${escHtml(buf.join('\n'))}</code></pre>`); continue;
    }
    if (!l.trim()) { i++; continue; }
    let m;
    if ((m = l.match(/^(#{1,4})\s+(.*)$/))) { const n = Math.min(m[1].length + 2, 6); out.push(`<h${n}>${mdInline(m[2])}</h${n}>`); i++; continue; }
    if (/^\s*(---|\*\*\*)\s*$/.test(l)) { out.push('<hr>'); i++; continue; }
    if (isTable(l, lines[i + 1])) {
      const head = cells(l); i += 2; const rows = [];
      while (i < lines.length && /^\s*\|.*\|\s*$/.test(lines[i])) rows.push(cells(lines[i++]));
      out.push(`<div class="md-table"><table><tr>${head.map(c => `<th>${mdInline(c)}</th>`).join('')}</tr>${rows.map(r => `<tr>${r.map(c => `<td>${mdInline(c)}</td>`).join('')}</tr>`).join('')}</table></div>`);
      continue;
    }
    if (isList(l)) {
      const ordered = /^\s*\d/.test(l), items = [];
      while (i < lines.length && ((isList(lines[i]) && /^\s*\d/.test(lines[i]) === ordered) || (/^\s{2,}\S/.test(lines[i]) && !isList(lines[i]) && items.length))) {
        if (isList(lines[i])) items.push(lines[i].replace(/^\s*([-*+]|\d+[.)])\s+/, '')); else items[items.length - 1] += ' ' + lines[i].trim();
        i++;
      }
      const li = items.map(t => { const c = t.match(/^\[( |x|X)\]\s+(.*)$/); return c ? `<li class="task">${c[1] === ' ' ? '☐' : '☑'} ${mdInline(c[2])}</li>` : `<li>${mdInline(t)}</li>`; }).join('');
      out.push(ordered ? `<ol>${li}</ol>` : `<ul>${li}</ul>`); continue;
    }
    if (/^\s*>/.test(l)) {
      const buf = [];
      while (i < lines.length && /^\s*>/.test(lines[i])) buf.push(lines[i++].replace(/^\s*>\s?/, ''));
      out.push(`<blockquote>${mdToHtml(buf.join('\n'))}</blockquote>`); continue;
    }
    const buf = [];
    while (i < lines.length && lines[i].trim() && !/^\s*```/.test(lines[i]) && !/^#{1,4}\s/.test(lines[i]) && !isList(lines[i]) && !isTable(lines[i], lines[i + 1]) && !/^\s*>/.test(lines[i])) buf.push(lines[i++]);
    out.push(`<p>${buf.map(mdInline).join('<br>')}</p>`);
  }
  return out.join('');
}
const md = (text, cls = '') => h('div', { class: `md ${cls}`, html: mdToHtml(text) });

// ───────── ASSISTANT (chat with the maestro agent)
const CHAT_SUGGESTIONS = [
  ['Where are we?', 'Where are we on the project? What needs me right now?'],
  ['Set up a new game', 'I want to start a new game on the AYLO engine: create a private repository for it, watch it together with the engine repository, and add a GitHub board mirrored from the labels. Tell me the plan before doing anything.'],
  ['Plan a feature', 'Plan this feature into tickets for the team: '],
  ['Why is it stuck?', 'Which tickets are blocked or failed, and why? Propose how to unblock each one.'],
  ['How does Maestro work?', 'Explain how Maestro works: where tickets come from, the states, and what each agent does.'],
];
async function loadChats() {
  try {
    const r = await api('/api/chats');
    S.chats = r.chats; S.chatBusy = r.busy || {};
    if (S.view === 'assistant') { renderChatList(); renderChatHead(); renderChatInput(); }
    renderNav();
  } catch (e) { /* offline */ }
}
async function openChat(id) {
  S.chatId = id; S.chatLive = [];
  try { localStorage.setItem('maestro-chat', String(id)); } catch (e) { /* storage unavailable */ }
  const d = await api(`/api/chats/${id}`).catch(e => { toast(e.message, 'error'); return null; });
  if (!d) { S.chatId = null; S.chatMsgs = []; return refreshChat(); }
  S.chatMsgs = d.messages;
  if (d.run_id) {
    S.chatBusy[id] = d.run_id;
    const logs = await api(`/api/runs/${d.run_id}/logs`).catch(() => []);
    S.chatLive = logs.filter(l => ['tool', 'text', 'error'].includes(l.kind));
  }
  refreshChat(true);
}
async function newChat() {
  const c = await act(() => api('/api/chats', { method: 'POST', body: {} }));
  if (!c) return null;
  (S.chats ||= []).unshift(c);
  await openChat(c.id);
  $('#chat-text')?.focus();
  return c.id;
}
async function sendChat(text) {
  text = (text ?? $('#chat-text')?.value ?? '').trim();
  if (!text) return;
  if (!S.chatId && !(await newChat())) return;
  const box = $('#chat-text'); if (box) box.value = '';
  S.chatDraft = '';
  const r = await act(() => api(`/api/chats/${S.chatId}/messages`, { method: 'POST', body: { text } }));
  if (!r) { if (box) box.value = text; return; }
  onChatMessage(r.message);
  onChatState(S.chatId, r.run_id);
}
function onChatMessage(m) {
  if (m.chat_id !== S.chatId) return;
  if (S.chatMsgs.some(x => x.id === m.id)) return;
  S.chatMsgs.push(m);
  if (m.role !== 'user') S.chatLive = [];
  if (S.view === 'assistant') renderChatThread(true);
}
function onChatState(chatId, runId) {
  if (runId) S.chatBusy[chatId] = runId; else delete S.chatBusy[chatId];
  if (chatId === S.chatId && runId) S.chatLive = [];
  renderNav();
  if (S.view === 'assistant') { renderChatList(); if (chatId === S.chatId) { renderChatThread(true); renderChatInput(); } }
}
function chatLog(l) {
  if (!S.chatId || S.chatBusy[S.chatId] !== l.run_id || !['tool', 'text', 'error'].includes(l.kind)) return;
  S.chatLive.push(l);
  if (S.view === 'assistant') renderChatLive();
}
const prettyTool = t => t.replace(/^mcp__maestro__/, '').replace(/ · \{\}$/, '');
function liveBox() {
  const tools = S.chatLive.filter(l => l.kind === 'tool');
  const lastText = [...S.chatLive].reverse().find(l => l.kind === 'text');
  return h('div', { class: 'msg assistant live', id: 'chat-live' },
    h('div', { class: 'msg-av' }, icon('chat', 'sm')),
    h('div', { class: 'msg-body' },
      h('div', { class: 'live-h' }, h('span', { class: 'spin' }), tools.length ? `Working · ${tools.length} tool call${tools.length > 1 ? 's' : ''}` : 'Thinking…'),
      tools.length ? h('div', { class: 'live-tools' }, tools.slice(-6).map(t => h('div', { class: 'lt' }, icon('bolt', 'xs'), prettyTool(t.text)))) : null,
      lastText ? md(lastText.text, 'dim') : null));
}
function renderChatLive() {
  const el = $('#chat-live'); if (!el) return renderChatThread(true);
  const th = $('#chat-thread'); const stick = th && th.scrollHeight - th.scrollTop - th.clientHeight < 80;
  el.replaceWith(liveBox());
  if (stick && th) th.scrollTop = th.scrollHeight;
}
function chatMessage(m) {
  if (m.role === 'system') return h('div', { class: 'msg-sys' }, m.text);
  const mine = m.role === 'user';
  return h('div', { class: `msg ${m.role}` },
    h('div', { class: 'msg-av' }, mine ? initial('you') : icon(m.role === 'error' ? 'alert' : 'chat', 'sm')),
    h('div', { class: 'msg-body' },
      mine ? h('div', { class: 'msg-text' }, m.text) : md(m.text),
      h('div', { class: 'msg-meta' }, time(m.ts), m.cost_usd ? ` · ${money(m.cost_usd)}` : '',
        m.run_id && !mine ? h('a', { href: '#runs', onclick: e => { e.preventDefault(); S.selRun = m.run_id; go('runs'); } }, ' · log') : null)));
}
function renderChatThread(scroll) {
  const th = $('#chat-thread'); if (!th) return;
  const busy = S.chatId && S.chatBusy[S.chatId];
  if (!S.chatId || (!S.chatMsgs.length && !busy)) {
    const ready = S.assistant?.available !== false;
    put(th, h('div', { class: 'chat-empty' },
      h('span', { class: 'logo', style: 'width:44px;height:44px;border-radius:12px' }, icon('chat')),
      h('h2', {}, 'Maestro Assistant'),
      h('p', {}, 'Ask about progress, plan work into tickets, create and wire repositories and GitHub boards, unblock stuck tickets or tune the agents. It confirms before anything outward-facing.'),
      ready ? null : h('div', { class: 'note' }, icon('alert', 'sm'), `The assistant agent “${S.assistant?.agent || 'maestro'}” was not found — create it in Agents, cells, skills or pick another one in Settings → Execution.`),
      h('div', { class: 'chips' }, CHAT_SUGGESTIONS.map(([l, t]) => h('button', { type: 'button', class: 'chip', onclick: () => {
        if (t.endsWith(': ')) { const b = $('#chat-text'); b.value = t; b.focus(); b.setSelectionRange(t.length, t.length); } else sendChat(t);
      } }, l)))));
    return;
  }
  put(th, h('div', { class: 'chat-inner' }, S.chatMsgs.map(chatMessage), busy ? liveBox() : null));
  if (scroll) th.scrollTop = th.scrollHeight;
}
function renderChatList() {
  const el = $('#chat-list'); if (!el) return;
  put(el,
    btn('plus', 'New chat', { class: 'btn sm primary', style: 'width:100%;margin-bottom:10px', onclick: newChat }),
    (S.chats || []).length ? (S.chats || []).map(c => h('div', { class: 'chat-item' + (c.id === S.chatId ? ' on' : ''), onclick: () => openChat(c.id) },
      h('div', { class: 'ci-t' }, S.chatBusy[c.id] ? h('span', { class: 'spin' }) : null, c.title),
      h('div', { class: 'ci-m' }, ago(c.updated_at), c.cost_usd ? ` · ${money(c.cost_usd)}` : '')))
      : h('div', { class: 'muted small', style: 'padding:8px' }, S.chats === null ? 'Loading…' : 'No conversations yet'));
}
function renderChatHead() {
  const el = $('#chat-head'); if (!el) return;
  const c = (S.chats || []).find(x => x.id === S.chatId);
  put(el, h('div', { class: 'ch-t' }, h('b', {}, c ? c.title : 'Assistant'), h('small', {}, `agent ${S.assistant?.agent || 'maestro'}${c?.cost_usd ? ' · ' + money(c.cost_usd) : ''}`)),
    c ? h('div', { class: 'row-btns', style: 'margin:0' },
      btn('edit', '', { class: 'icon-btn sm', title: 'Rename', onclick: async () => { const n = await promptDialog({ title: 'Rename conversation', label: 'Title', value: c.title, ok: 'Rename' }); if (n) act(async () => { await api(`/api/chats/${c.id}`, { method: 'PUT', body: { title: n } }); await loadChats(); }); } }),
      btn('reset', '', { class: 'icon-btn sm', title: 'Start a fresh context (the assistant forgets this conversation)', onclick: async () => {
        if (await confirmDialog('The assistant will not remember the messages above (they stay visible). Useful when the conversation got long or confused.', { title: 'Fresh context?', ok: 'Reset context' })) act(async () => { await api(`/api/chats/${c.id}/reset`, { method: 'POST' }); await openChat(c.id); }, 'Context reset');
      } }),
      btn('trash', '', { class: 'icon-btn sm', title: 'Delete conversation', onclick: async () => {
        if (await confirmDialog(`Delete “${c.title}”?`, { title: 'Delete conversation?', ok: 'Delete', danger: true })) act(async () => { await api(`/api/chats/${c.id}`, { method: 'DELETE' }); S.chatId = null; S.chatMsgs = []; await loadChats(); refreshChat(); });
      } })) : null);
}
function renderChatInput() {
  const el = $('#chat-form'); if (!el) return;
  const busy = S.chatId && S.chatBusy[S.chatId];
  const sendBtn = $('#chat-send');
  if (sendBtn) put(sendBtn, busy ? [icon('stop', 'sm'), lbl('Stop')] : [icon('send', 'sm'), lbl('Send')]);
  if (sendBtn) sendBtn.className = busy ? 'btn danger' : 'btn primary';
}
function refreshChat(scroll) { renderChatList(); renderChatHead(); renderChatThread(scroll); renderChatInput(); }
function viewAssistant() {
  if (S.chats === null) {
    loadChats().then(() => {
      let last = null; try { last = +localStorage.getItem('maestro-chat'); } catch (e) { /* storage unavailable */ }
      const pick = (S.chats || []).find(c => c.id === last) || (S.chats || [])[0];
      if (pick && !S.chatId) openChat(pick.id); else refreshChat(true);
    });
  }
  const ta = h('textarea', { id: 'chat-text', rows: 1, placeholder: 'Message the assistant…  (Enter to send, Shift+Enter for a new line)', autofocus: true,
    oninput: e => { S.chatDraft = e.target.value; e.target.style.height = 'auto'; e.target.style.height = Math.min(e.target.scrollHeight, 220) + 'px'; },
    onkeydown: e => { if (e.key === 'Enter' && !e.shiftKey && !e.isComposing) { e.preventDefault(); if (!(S.chatId && S.chatBusy[S.chatId])) sendChat(); } } }, S.chatDraft || '');
  const view = h('div', { class: 'chat' },
    h('aside', { class: 'chat-list', id: 'chat-list' }),
    h('section', { class: 'chat-main' },
      h('div', { class: 'chat-head', id: 'chat-head' }),
      h('div', { class: 'chat-thread', id: 'chat-thread' }),
      h('form', { class: 'chat-form', id: 'chat-form', onsubmit: e => { e.preventDefault(); if (S.chatId && S.chatBusy[S.chatId]) act(() => api(`/api/chats/${S.chatId}/cancel`, { method: 'POST' })); else sendChat(); } },
        ta, h('button', { class: 'btn primary', id: 'chat-send', type: 'submit' }, icon('send', 'sm'), lbl('Send')))));
  setTimeout(() => { refreshChat(true); $('#chat-text')?.focus(); }, 0);
  return view;
}

// ───────── SETTINGS
const SETTINGS_TABS = [['project', 'Project', 'folder'], ['claude', 'Claude Code', 'cpu'], ['github', 'GitHub access', 'github'], ['execution', 'Execution', 'play'], ['integrations', 'Git & MCP', 'git']];

function viewSettings() {
  S.settingsTab ||= 'project';
  const s = (S.draft ||= structuredClone(S.settings));
  const inp = (k, label, help, type = 'text', extra = {}) => {
    const env = (S.envSecrets || []).includes(k);
    return h('label', { class: 'f' }, h('span', {}, label),
      h('input', { type, value: env ? '' : (s[k] ?? ''), disabled: env, placeholder: env ? 'Set in .env' : '', oninput: e => s[k] = type === 'number' ? +e.target.value : e.target.value, ...extra }),
      env ? h('small', {}, icon('key', 'xs'), ' Provided by the environment (.env), not stored in the database') : help ? h('small', {}, help) : null);
  };
  const sel = (k, label, opts, help) => h('label', { class: 'f' }, h('span', {}, label),
    h('select', { onchange: e => { s[k] = e.target.value; } }, opts.map(([v, l]) => h('option', { value: v, selected: s[k] === v }, l))), help ? h('small', {}, help) : null);
  const chk = (k, label) => h('label', { class: 'switch' }, h('input', { type: 'checkbox', checked: !!s[k], onchange: e => s[k] = e.target.checked }), h('span', { class: 'track' }), label);
  const save = () => act(async () => { S.settings = await api('/api/settings', { method: 'PUT', body: s }); S.draft = null; await load(); }, 'Settings saved');
  const out = h('pre', { class: 'body-md mono small test-out', style: 'display:none' });
  const test = (path, label) => btn('bolt', label, { class: 'btn sm', onclick: async ev => {
    const b = ev.currentTarget; b.disabled = true; out.style.display = 'block'; out.textContent = `${label}…`;
    try { await api('/api/settings', { method: 'PUT', body: s }); const r = await api(path, { method: 'POST' }); out.textContent = JSON.stringify(r, null, 2); } catch (e) { out.textContent = '✕ ' + e.message; }
    b.disabled = false;
  } });
  const section = (title, desc, ...kids) => h('div', { class: 'card sec' }, h('div', { class: 'sec-head' }, h('h3', {}, title), desc ? h('p', {}, desc) : null), h('div', { class: 'sec-body' }, ...kids));
  const testRow = (...b) => h('div', { class: 'row-btns' }, ...b);

  const cur = activeProject();
  const tabs = {
    project: () => [
      cur ? section(cur.name, `${projectSubtitle(cur)}. These settings only apply to this project.`,
        testRow(
          btn('edit', 'Rename', { class: 'btn sm', onclick: async () => { const n = await promptDialog({ title: 'Rename project', label: 'Name', value: cur.name, ok: 'Rename' }); if (n) act(async () => { await api('/api/projects/rename', { method: 'POST', body: { key: cur.key, name: n } }); await load(); }); } }),
          test('/api/settings/test-github', 'Test connection'),
          btn('plus', 'Add another project', { class: 'btn sm', onclick: () => openProjectPicker() })), out) : section('No project yet', 'Add your repositories (or a GitHub Project board) to get started — or ask the Assistant to create them.',
        testRow(btn('repo', 'Add repositories…', { class: 'btn primary', onclick: () => openProjectPicker('repo') }), btn('board', 'Use a GitHub board…', { class: 'btn', onclick: () => openProjectPicker('project') }), btn('chat', 'Ask the Assistant', { class: 'btn', onclick: () => go('assistant') }))),
      section('Source', 'Where tickets come from and how their progress is shown on GitHub.',
        h('div', { class: 'seg', style: 'margin-bottom:14px' },
          h('button', { type: 'button', class: s.source !== 'project' ? 'on' : '', onclick: () => { s.source = 'repo'; render(); } }, icon('repo', 'sm'), 'Repositories (labels)'),
          h('button', { type: 'button', class: s.source === 'project' ? 'on' : '', onclick: () => { s.source = 'project'; render(); } }, icon('board', 'sm'), 'GitHub Project board')),
        s.source !== 'project' ? [
          h('label', { class: 'f' }, h('span', {}, 'Repositories'),
            h('textarea', { rows: 3, oninput: e => s.repos = e.target.value, placeholder: 'owner/game\nowner/engine' }, s.repos || ''),
            h('small', {}, 'One owner/name per line, the primary first. Maestro creates its labels on them when you save.')),
          h('div', { class: 'grid2' },
            inp('trigger_label', 'Trigger label', 'An open issue with this label is picked up'),
            h('div', { class: 'f' }, h('span', {}, 'Status labels'), h('div', { class: 'small muted', style: 'padding-top:6px' }, [s.col_in_progress, s.col_review, s.col_done, s.col_blocked].filter(Boolean).map(c => h('span', { class: 'pill mono', style: 'margin:0 4px 4px 0' }, 'maestro:' + c.toLowerCase().trim().split(/\s+/).join('-')))))),
          h('details', { class: 'pp-manual', open: s.project_number > 0 }, h('summary', {}, 'Mirror to a GitHub Project board (optional)'),
            h('p', { class: 'small muted' }, 'Issues are added to the board and its Status column follows the labels. Needs the token scope project.'),
            h('div', { class: 'grid3' },
              inp('project_owner', 'Board owner', 'User or organization (empty = no mirror)'),
              inp('project_number', 'Board number', 'github.com/users/<owner>/projects/<number>', 'number'),
              inp('status_field', 'Status field', 'Single-select field'))),
        ] : [
          h('div', { class: 'grid3' },
            inp('project_owner', 'Project owner', 'User or organization'),
            inp('project_number', 'Project number', 'github.com/users/<owner>/projects/<number>', 'number'),
            inp('status_field', 'Status field', 'Single-select field used as kanban columns')),
        ],
        h('div', { class: 'grid2' },
          inp('col_trigger', 'Trigger column', s.source !== 'project' ? 'Name shown for queued tickets (board mirror column)' : 'Cards moved here get picked up'),
          inp('col_in_progress', 'In progress', ''), inp('col_review', 'In review', ''), inp('col_done', 'Done', ''))),
      section('Repository', 'Where agents work for this project.',
        inp('base_branch', 'Base branch', 'Empty = repository default branch'),
        h('label', { class: 'f' }, h('span', {}, 'Local repository paths (optional)'),
          h('textarea', { rows: 3, oninput: e => s.repo_paths = e.target.value, placeholder: 'owner/repo=/Users/you/dev/repo' }, s.repo_paths || ''),
          h('small', {}, 'One per line. Otherwise Maestro clones into its data dir. Each ticket always gets its own git worktree + branch.'))),
      cur && S.projects.length > 1 ? h('div', { class: 'card sec danger' }, h('div', { class: 'sec-head' }, h('h3', {}, 'Switch project'), h('p', {}, 'To remove a project, switch to another one first.')),
        h('div', { class: 'sec-body' }, h('button', { class: 'btn danger sm', type: 'button', style: 'margin-bottom:14px', onclick: async () => {
          const other = S.projects.find(p => p.key !== cur.key);
          if (await confirmDialog(`Switch to “${other.name}”? The engine will be paused.`, { title: 'Switch project?', ok: 'Switch' })) switchProject(other.key);
        } }, 'Switch to another project…'))) : null,
    ],
    claude: () => [section('Claude Code', 'How agents authenticate and run.',
      testRow(test('/api/settings/test-claude', 'Test Claude')), out,
      h('div', { class: 'grid2' },
        sel('claude_auth', 'Authentication', [['account', 'My Claude account (local `claude` login — Pro/Max)'], ['oauth_token', 'Long-lived token (`claude setup-token`)'], ['api_key', 'Anthropic API key (pay as you go)']],
          'Account mode = whatever `claude` is logged into on this machine (run `claude` once and /login).'),
        inp('claude_bin', 'Claude binary', 'Path or command name, e.g. claude or /opt/homebrew/bin/claude'),
        inp('claude_oauth_token', 'OAuth token', 'From `claude setup-token`. Prefer MAESTRO_CLAUDE_OAUTH_TOKEN in .env', 'password'),
        inp('anthropic_api_key', 'API key', 'Prefer MAESTRO_ANTHROPIC_API_KEY in .env', 'password'),
        sel('permission_mode', 'Permission mode for workers', [['acceptEdits', 'acceptEdits — edits auto-approved, tools per agent allow-list'], ['bypassPermissions', 'bypassPermissions — everything allowed (isolated machines only)'], ['default', 'default — only allow-listed tools']]),
        inp('default_model', 'Default model', 'Used when an agent has no model (empty = Claude Code default)'),
        inp('max_budget_usd', 'Max budget per run (USD, 0 = none)', 'Only meaningful for API-key billing', 'number', { step: '0.5' })))],
    github: () => [section('GitHub access', 'Shared by all projects.',
      testRow(test('/api/settings/test-github', 'Test GitHub')), out,
      h('div', { class: 'grid2' },
        sel('github_auth', 'Authentication', [['token', 'Personal access token'], ['gh_cli', 'Use `gh auth token` (GitHub CLI login)']]),
        inp('github_token', 'Token', 'Classic PAT with scope repo (+ project for boards, + admin:org to create org repositories). Prefer MAESTRO_GITHUB_TOKEN in .env', 'password'),
        inp('poll_seconds', 'Sync interval (seconds)', '', 'number')))],
    execution: () => [
      section('Concurrency & limits', 'How many agents run at once and when to give up.',
        h('div', { class: 'grid3' },
          inp('max_parallel', 'Parallel agents', 'Simultaneous Claude processes', 'number'),
          inp('max_attempts', 'Attempts per ticket', 'Before “needs human”', 'number'),
          inp('max_review_rounds', 'Review rounds', 'Fix ↔ review loops', 'number'))),
      section('Routing & review', 'How tickets flow once they are picked up.',
        h('div', { class: 'grid2' },
          sel('orchestrator_agent', 'Orchestrator agent', S.agents.map(a => [a.name, a.name])),
          sel('reviewer_agent', 'Reviewer agent', S.agents.map(a => [a.name, a.name])),
          sel('assistant_agent', 'Assistant agent', S.agents.map(a => [a.name, a.name]), 'The agent you chat with in the Assistant view')),
        chk('review_enabled', 'Reviewer agent checks every PR (and sends feedback back to the worker)'),
        chk('auto_merge', 'Auto-merge (squash) approved PRs'),
        chk('always_triage', 'Always ask the orchestrator (ignore label routing)'))],
    integrations: () => {
      const cmd = S.mcp ? `claude mcp add --transport http maestro ${S.mcp.url} \\\n  --header "Authorization: Bearer ${S.mcp.token}"` : '';
      return [
        section('Git', 'Identity used for agent commits.', h('div', { class: 'grid2' }, inp('git_author_name', 'Commit author', ''), inp('git_author_email', 'Commit email', ''))),
        section('MCP (ticket tools)', 'Worker agents get list_tickets, get_ticket, list_agents, create_ticket, comment_ticket and close_ticket automatically.',
          h('p', { class: 'small muted', style: 'margin:0 0 14px' }, 'Opt out per agent with ', h('code', {}, 'maestro_tools: false'), '.'),
          chk('mcp_auto_queue', 'Tickets created by agents are queued right away (otherwise they land in the backlog for you to start)'),
          h('p', { class: 'small', style: 'margin:6px 0 8px;color:var(--tx-2)' }, 'Use the same tools from your own Claude Code (add ', h('code', {}, '?scope=assistant'), ' to the URL for the Assistant\'s admin tools: projects, repositories, boards, engine):'),
          h('div', { class: 'code-block', style: 'margin-bottom:14px' },
            h('pre', { class: 'body-md mono small', style: 'max-height:none;padding-right:80px' }, cmd),
            cmd ? btn('copy', 'Copy', { class: 'btn xs', onclick: async () => { try { await navigator.clipboard.writeText(cmd); toast('Copied to clipboard', 'ok'); } catch (e) { toast('Copy failed — select the text manually', 'warn'); } } }) : null)),
      ];
    },
  };
  return h('div', { class: 'settings' },
    h('div', { class: 'page-h' }, h('div', {}, h('h2', {}, 'Settings'), h('p', {}, 'Configuration for Maestro, the active project and the agents.')),
      btn('check', 'Save changes', { class: 'btn primary', onclick: save })),
    h('div', { class: 'tabs', role: 'tablist' }, SETTINGS_TABS.map(([k, l, ic]) => h('a', { class: k === S.settingsTab ? 'on' : '', role: 'tab', href: `#settings-${k}`, onclick: e => { e.preventDefault(); S.settingsTab = k; render(); } }, icon(ic, 'sm'), l))),
    tabs[S.settingsTab](),
    h('div', { class: 'row-btns', style: 'justify-content:flex-end' }, btn('check', 'Save changes', { class: 'btn primary', onclick: save })));
}

load().catch(e => toast(e.message, 'error'));
connect();
setInterval(renderChrome, 15000);
