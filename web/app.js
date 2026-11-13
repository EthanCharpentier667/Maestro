// Maestro web UI — vanilla JS, no build step. Live updates via SSE.
'use strict';

const S = { tickets: new Map(), agents: [], skills: [], settings: {}, project: null, stats: {}, sm: { states: [], transitions: [] },
  runs: [], busy: [], lastSync: null, claudeVersion: null, view: 'board', boardMode: 'state', filter: { q: '', repo: '', agent: '' },
  selRun: null, drawerTicket: null, feed: [], plan: null, planning: false, hideDone: false, graphView: null };

const STATE_LABEL = { backlog: 'Backlog', triage: 'Triage', split: 'Split', blocked: 'Blocked', ready: 'Ready', running: 'Running',
  reviewing: 'Reviewing', awaiting_merge: 'Awaiting merge', done: 'Done', needs_human: 'Needs human', paused: 'Paused' };
const BOARD_COLS = [
  ['Backlog', ['backlog', 'paused']], ['Triage', ['triage', 'split']], ['Queued', ['ready', 'blocked']],
  ['In progress', ['running']], ['Review', ['reviewing', 'awaiting_merge']], ['Needs human', ['needs_human']], ['Done', ['done']],
];
const sc = s => `var(--s-${s})`;

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
const esc = s => String(s ?? '').replace(/[&<>"]/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));
const money = v => '$' + (v || 0).toFixed(v >= 10 ? 1 : 3);
const ago = ts => { if (!ts) return '—'; const s = (Date.now() - new Date(ts)) / 1000; return s < 60 ? `${s | 0}s ago` : s < 3600 ? `${(s / 60) | 0}m ago` : s < 86400 ? `${(s / 3600) | 0}h ago` : `${(s / 86400) | 0}d ago`; };
const time = ts => ts ? new Date(ts).toLocaleTimeString() : '';

async function api(path, opts = {}) {
  const r = await fetch(path, { headers: { 'content-type': 'application/json' }, ...opts, body: opts.body ? JSON.stringify(opts.body) : undefined });
  const j = await r.json().catch(() => ({}));
  if (!r.ok) throw new Error(j.error || r.statusText);
  return j;
}
function toast(msg, level = 'info') {
  const t = h('div', { class: `toast ${level}` }, msg);
  $('#toasts').append(t);
  setTimeout(() => t.remove(), level === 'error' ? 9000 : 4500);
}
async function act(fn, okMsg) {
  try { const r = await fn(); if (okMsg) toast(okMsg, 'ok'); return r; } catch (e) { toast(e.message, 'error'); }
}

const agentOf = n => S.agents.find(a => a.name === n);
const cellOf = n => (S.cells || []).find(c => c.name === n);
const agentColor = n => agentOf(n)?.color || cellOf(n)?.color || '#7c8799';
const workersList = () => S.agents.filter(a => a.role !== 'orchestrator' && a.role !== 'reviewer');
function agentChip(n) { return n ? h('span', { class: 'agent-chip', style: `--ac:${agentColor(n)}`, title: cellOf(n) ? `cell: ${[cellOf(n).lead, ...cellOf(n).members].filter(Boolean).join(', ')}` : 'agent' }, cellOf(n) ? '▣ ' : '◉ ', n) : null; }
function assignees(t) { return [agentChip(t.agent), (t.helpers || []).map(x => h('span', { class: 'agent-chip', style: `--ac:${agentColor(x)}`, title: 'helper' }, '+ ', x))]; }
function assigneeOptions(selected, emptyLabel) {
  return [emptyLabel ? h('option', { value: '' }, emptyLabel) : null,
    h('optgroup', { label: 'Agents' }, workersList().map(a => h('option', { value: a.name, selected: a.name === selected }, a.name))),
    (S.cells || []).length ? h('optgroup', { label: 'Cells' }, S.cells.map(c => h('option', { value: c.name, selected: c.name === selected }, `${c.name} (${c.mode || 'delegate'})`))) : null];
}
function stateBadge(s) { return h('span', { class: 'pill state', style: `background:${sc(s)}` }, STATE_LABEL[s] || s); }

// ───────── data
async function load() {
  const d = await api('/api/state');
  S.tickets = new Map(d.tickets.map(t => [t.id, t]));
  S.agents = d.agents; S.cells = d.cells || []; S.skills = d.skills; S.settings = d.settings; S.project = d.project; S.stats = d.stats;
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
      case 'log': appendLog(e.line); return;
      case 'notice':
        if (e.level === 'plan') { S.planning = false; S.plan = JSON.parse(e.message).plan; S.planRun = JSON.parse(e.message).run_id; if (S.view === 'planner') render(); toast('Plan ready ✦', 'ok'); return; }
        toast(e.message, e.level); if (e.message.startsWith('planner')) { S.planning = false; if (S.view === 'planner') render(); } break;
    }
    scheduleRender();
  };
  es.onerror = () => { $('#sync-info').textContent = 'reconnecting…'; };
  es.onopen = () => load().catch(() => {});
}
let rt = null;
function scheduleRender() { if (rt) return; rt = setTimeout(() => { rt = null; renderChrome(); if (!['settings', 'agents', 'planner'].includes(S.view) || S.view === 'agents' && !S.editing) render(); }, 120); }

// ───────── chrome
function renderChrome() {
  const ts = [...S.tickets.values()];
  const count = s => ts.filter(t => t.state === s).length;
  $('#proj').innerHTML = S.project ? `${esc(S.project.title)} <small><a href="${esc(S.project.url)}" target="_blank">GitHub ↗</a></small>` : '<span class="muted">No project — open Settings</span>';
  put($('#kpis'),
    h('span', { class: 'kpi' }, h('b', {}, ts.length), ' tickets'),
    h('span', { class: 'kpi' }, h('span', { class: 'dot', style: `background:${sc('running')}` }), ' ', h('b', {}, count('running') + count('reviewing')), ' active'),
    h('span', { class: 'kpi' }, h('b', {}, count('ready') + count('triage')), ' queued'),
    count('needs_human') ? h('span', { class: 'kpi', style: 'border-color:#f43f5e66' }, h('b', {}, count('needs_human')), ' need you') : null,
    h('span', { class: 'kpi' }, h('b', {}, count('done')), ' done'),
    h('span', { class: 'kpi' }, 'spent ', h('b', {}, money(S.stats.total_cost_usd))),
  );
  const on = S.settings.engine_enabled;
  const b = $('#btn-engine'); b.textContent = on ? '❚❚ Pause engine' : '▶ Start engine'; b.classList.toggle('on', on);
  const ls = S.lastSync; $('#sync-info').innerHTML = ls ? (ls.error ? `<span style="color:var(--err)" title="${esc(ls.error)}">sync error</span>` : `synced ${ago(ls.ts)}`) : '';
  put($('#side-foot'),
    h('div', { class: 'row' }, h('span', {}, 'Claude Code'), h('span', {}, S.claudeVersion || '⚠ not found')),
    h('div', { class: 'row' }, h('span', {}, 'Auth'), h('span', {}, { account: 'account', oauth_token: 'token', api_key: 'API key' }[S.settings.claude_auth] || '—')),
    h('div', { class: 'row' }, h('span', {}, 'Workers'), h('span', {}, `${S.busy.length}/${S.settings.max_parallel || '?'}`)),
  );
}

$('#btn-engine').onclick = () => act(async () => { const r = await api('/api/engine', { method: 'POST', body: { enabled: !S.settings.engine_enabled } }); S.settings.engine_enabled = r.enabled; renderChrome(); });
$('#btn-sync').onclick = () => act(async () => { $('#btn-sync').disabled = true; try { const r = await api('/api/sync', { method: 'POST' }); await load(); toast(`Synced ${r.items} items`, 'ok'); } finally { $('#btn-sync').disabled = false; } });
$('#nav').onclick = e => { const a = e.target.closest('a'); if (!a) return; S.view = a.dataset.view; S.editing = null; document.querySelectorAll('#nav a').forEach(x => x.classList.toggle('active', x === a)); render(); };

function render() {
  const v = $('#view');
  const fn = { board: viewBoard, graph: viewGraph, machine: viewMachine, planner: viewPlanner, agents: viewAgents, runs: viewRuns, settings: viewSettings }[S.view];
  const keepScroll = v.scrollTop;
  put(v, fn());
  v.scrollTop = keepScroll;
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
    h('input', { placeholder: 'Search…', value: S.filter.q, oninput: e => { S.filter.q = e.target.value; clearTimeout(S._ft); S._ft = setTimeout(render, 200); } }),
    h('select', { onchange: e => { S.filter.repo = e.target.value; render(); } }, h('option', { value: '' }, 'All repos'), repos.map(r => h('option', { value: r, selected: r === S.filter.repo }, r))),
    h('select', { onchange: e => { S.filter.agent = e.target.value; render(); } }, h('option', { value: '' }, 'All agents & cells'), [...S.agents, ...(S.cells || [])].map(a => h('option', { value: a.name, selected: a.name === S.filter.agent }, a.name))),
    ...extra);
}

// ───────── BOARD
function ticketCard(t) {
  const running = S.busy.includes(t.id) && ['running', 'reviewing', 'triage'].includes(t.state);
  return h('div', { class: `tcard ${running ? 'running' : ''}`, style: `--c:${sc(t.state)}`, onclick: () => openTicket(t.id) },
    h('div', { class: 'meta' }, h('span', { class: 'num' }, `${t.repo.split('/')[1] || ''}#${t.number}`), stateBadge(t.state),
      running ? h('span', { class: 'spin' }) : null),
    h('div', { class: 't' }, t.title),
    h('div', { class: 'meta' }, assignees(t),
      t.deps.length ? h('span', { class: 'pill', title: 'dependencies' }, '⛓ ', t.deps.filter(d => S.tickets.get(d)?.state !== 'done').length, '/', t.deps.length) : null,
      t.pr_url ? h('a', { class: 'pill', href: t.pr_url, target: '_blank', onclick: e => e.stopPropagation() }, 'PR #', t.pr_number) : null,
      t.cost_usd ? h('span', { class: 'pill' }, money(t.cost_usd)) : null,
      t.gh_status ? h('span', { class: 'pill', title: 'GitHub column' }, '▦ ', t.gh_status) : null),
    t.note && t.state === 'needs_human' ? h('div', { class: 'note' }, t.note.slice(0, 220)) : null);
}
function viewBoard() {
  const ts = filtered();
  if (!S.project && !ts.length) return onboarding();
  let cols;
  if (S.boardMode === 'github') {
    const names = S.project?.options?.map(o => o[1]) || [...new Set(ts.map(t => t.gh_status))];
    cols = [...names, ''].map(n => [n || 'No status', ts.filter(t => (t.gh_status || '') === n), null]);
  } else cols = BOARD_COLS.map(([n, st]) => [n, ts.filter(t => st.includes(t.state)), st[0]]);
  return h('div', {},
    filterBar([h('button', { class: 'btn primary', onclick: () => newTicket() }, '+ New ticket'), h('div', { class: 'seg' },
      h('button', { class: S.boardMode === 'state' ? 'on' : '', onclick: () => { S.boardMode = 'state'; render(); } }, 'Maestro states'),
      h('button', { class: S.boardMode === 'github' ? 'on' : '', onclick: () => { S.boardMode = 'github'; render(); } }, 'GitHub columns'))]),
    h('div', { class: 'board' }, cols.map(([name, list, st]) => h('div', { class: 'col' },
      h('div', { class: 'col-h' }, st ? h('span', { class: 'dot', style: `background:${sc(st)}` }) : null, name, h('span', { class: 'count' }, list.length)),
      h('div', { class: 'col-b' }, list.length ? list.sort((a, b) => b.updated_at.localeCompare(a.updated_at)).map(ticketCard) : h('div', { class: 'muted small', style: 'padding:8px' }, '—'))))));
}
function onboarding() {
  return h('div', { class: 'card onboard' }, h('h2', {}, '🎼 Welcome to Maestro'),
    h('ol', {},
      h('li', {}, 'Settings → GitHub: token (scopes ', h('code', {}, 'repo'), ' + ', h('code', {}, 'project'), ') or the ', h('code', {}, 'gh'), ' CLI, then project owner + number.'),
      h('li', {}, 'Settings → Claude Code: “account” uses your local ', h('code', {}, 'claude'), ' login (Pro/Max). Or paste a token from ', h('code', {}, 'claude setup-token'), '.'),
      h('li', {}, 'Tune your team in Agents & skills (orchestrator, engine, gamedev, gamedesign…).'),
      h('li', {}, 'Move a card to the trigger column (“Todo” by default) or use the Planner, then ', h('b', {}, 'Start engine'), '.')),
    h('button', { class: 'btn primary', onclick: () => document.querySelector('[data-view=settings]').click() }, 'Open settings →'));
}

// ───────── TICKET DRAWER
$('#drawer').onclick = e => { if (e.target.id === 'drawer') closeDrawer(); };
document.addEventListener('keydown', e => { if (e.key === 'Escape') closeDrawer(); });
function closeDrawer() { $('#drawer').classList.add('hidden'); S.drawerTicket = null; S.drawerRun = null; }
async function openTicket(id, refresh = false) {
  const d = await api(`/api/tickets/${id}`).catch(e => toast(e.message, 'error'));
  if (!d) return;
  S.drawerTicket = id;
  const t = d.ticket;
  const others = [...S.tickets.values()].filter(x => x.id !== id).sort((a, b) => a.number - b.number);
  const A = (label, action, body, cls = '') => h('button', { class: `btn sm ${cls}`, onclick: () => act(() => api(`/api/tickets/${id}/${action}`, { method: 'POST', body }), `${label} ✓`) }, label);
  const actions = [];
  if (t.state === 'backlog') actions.push(A('▶ Start', 'start', null, 'primary'));
  if (['needs_human', 'paused', 'backlog', 'blocked', 'ready'].includes(t.state)) actions.push(A('↻ Retriage', 'retriage'));
  if (['needs_human', 'paused', 'awaiting_merge'].includes(t.state) && t.agent) actions.push(A('⟲ Retry agent', 'retry'));
  if (t.state === 'paused') actions.push(A('▶ Resume', 'resume'));
  if (['running', 'reviewing', 'triage'].includes(t.state)) actions.push(A('■ Cancel run', 'pause', null, 'danger'));
  else if (!['done', 'paused'].includes(t.state)) actions.push(A('❚❚ Pause', 'pause'));
  if (t.pr_number && t.state !== 'done') actions.push(A('⇲ Merge PR', 'merge', null, 'primary'));
  if (t.state !== 'backlog' && t.state !== 'done') actions.push(A('Reset', 'reset', null, 'danger'));
  if (t.gh_state === 'CLOSED' || t.state === 'done') actions.push(A('Reopen', 'reopen'));
  else actions.push(h('button', { class: 'btn sm danger', onclick: () => {
    const c = prompt('Close this issue. Optional comment:', ''); if (c === null) return;
    const notPlanned = confirm('Close as "not planned"? (Cancel = completed)');
    act(() => api(`/api/tickets/${id}/close`, { method: 'POST', body: { comment: c, reason: notPlanned ? 'not_planned' : 'completed' } }), 'Issue closed');
  } }, 'Close issue'));

  const drawerRun = refresh && S.drawerRun ? S.drawerRun : (d.runs[0]?.id || null);
  S.drawerRun = drawerRun;
  const logBox = h('div', { class: 'log log-inline', id: 'log-drawer' });
  put($('#drawer-inner'),
    h('div', { class: 'h' }, h('div', {}, h('span', { class: 'muted mono' }, `${t.repo}#${t.number}`), ' ', stateBadge(t.state)),
      h('div', { style: 'display:flex;gap:6px' }, h('a', { class: 'btn sm', href: t.url, target: '_blank' }, 'Issue ↗'), t.pr_url ? h('a', { class: 'btn sm', href: t.pr_url, target: '_blank' }, `PR #${t.pr_number} ↗`) : null,
        h('button', { class: 'btn sm ghost', onclick: closeDrawer }, '✕'))),
    h('h2', {}, t.title),
    h('div', { class: 'meta', style: 'display:flex;gap:6px;flex-wrap:wrap' }, assignees(t), t.labels.map(l => h('span', { class: 'pill' }, l)),
      h('span', { class: 'pill' }, `attempts ${t.attempts}`), h('span', { class: 'pill' }, `reviews ${t.review_rounds}`), h('span', { class: 'pill' }, money(t.cost_usd)),
      t.branch ? h('span', { class: 'pill mono' }, '⎇ ', t.branch) : null),
    t.note ? h('div', { class: 'note', style: 'font-size:13px' }, t.note) : null,
    h('div', { class: 'actions' }, actions),
    h('div', { class: 'grid2' },
      h('div', {},
        h('label', { class: 'f' }, h('span', {}, 'Assigned to (agent or cell)'), h('select', { onchange: e => act(() => api(`/api/tickets/${id}/assign`, { method: 'POST', body: { agent: e.target.value } }), 'Assigned') },
          assigneeOptions(t.agent, '- orchestrator decides -'))),
        h('label', { class: 'f' }, h('span', {}, 'Helpers (extra agents, ctrl/cmd-click)'), h('select', { multiple: true, size: 3, onchange: e => act(() => api(`/api/tickets/${id}/helpers`, { method: 'POST', body: { helpers: [...e.target.selectedOptions].map(o => o.value) } }), 'Helpers saved') },
          workersList().map(a => h('option', { value: a.name, selected: (t.helpers || []).includes(a.name) }, a.name))))),
      h('label', { class: 'f' }, h('span', {}, 'Depends on (ctrl/cmd-click)'), h('select', { multiple: true, size: 4, onchange: e => act(() => api(`/api/tickets/${id}/deps`, { method: 'POST', body: { deps: [...e.target.selectedOptions].map(o => +o.value) } }), 'Dependencies saved') },
        others.map(o => h('option', { value: o.id, selected: t.deps.includes(o.id) }, `#${o.number} ${o.title.slice(0, 50)}`))))),
    h('div', { class: 'section' }, h('h4', {}, 'Description'), h('div', { class: 'body-md' }, t.body || '(empty)')),
    h('div', { class: 'section' }, h('h4', {}, 'State history'), h('div', { class: 'timeline' },
      d.transitions.length ? d.transitions.slice().reverse().map(x => h('div', { class: 'ti', style: `--c:${sc(x.to)}` }, h('b', {}, STATE_LABEL[x.to]), h('span', { class: 'muted' }, ` ← ${STATE_LABEL[x.from]} · ${x.reason} · ${time(x.ts)}`))) : h('div', { class: 'muted small' }, 'no transitions yet'))),
    h('div', { class: 'section' }, h('h4', {}, 'Runs'),
      h('div', { style: 'display:flex;gap:6px;flex-wrap:wrap;margin-bottom:8px' }, d.runs.map(r => h('button', { class: `btn sm ${r.id === drawerRun ? 'primary' : ''}`, onclick: () => { S.drawerRun = r.id; openTicket(id, true); } },
        h('span', { class: `st-${r.status}` }, '●'), ` ${r.kind} · ${r.agent} · ${money(r.cost_usd)}`))),
      drawerRun ? logBox : h('div', { class: 'muted small' }, 'no runs yet')));
  $('#drawer').classList.remove('hidden');
  if (drawerRun) loadLog(drawerRun, logBox);
}

function newTicket() {
  const repos = [...new Set([...S.tickets.values()].map(t => t.repo))].sort();
  const f = { repo: S.planRepo || repos[0] || '', title: '', body: '', agent: '', helpers: [], deps: [], start: false };
  const others = [...S.tickets.values()].filter(t => t.state !== 'done').sort((a, b) => a.number - b.number);
  S.drawerTicket = null;
  put($('#drawer-inner'),
    h('div', { class: 'h' }, h('h2', {}, 'New ticket'), h('button', { class: 'btn sm ghost', onclick: closeDrawer }, '✕')),
    h('p', { class: 'muted small' }, 'Creates a GitHub issue and adds it to the project.'),
    h('label', { class: 'f' }, h('span', {}, 'Repository'), h('input', { list: 'nt-repos', value: f.repo, placeholder: 'owner/repo', oninput: e => f.repo = e.target.value }), h('datalist', { id: 'nt-repos' }, repos.map(r => h('option', { value: r })))),
    h('label', { class: 'f' }, h('span', {}, 'Title'), h('input', { oninput: e => f.title = e.target.value })),
    h('label', { class: 'f' }, h('span', {}, 'Description (markdown, acceptance criteria)'), h('textarea', { rows: 9, style: 'font-family:inherit', oninput: e => f.body = e.target.value })),
    h('div', { class: 'grid2' },
      h('div', {},
        h('label', { class: 'f' }, h('span', {}, 'Assigned to'), h('select', { onchange: e => f.agent = e.target.value }, assigneeOptions('', '- orchestrator decides -'))),
        h('label', { class: 'f' }, h('span', {}, 'Helpers'), h('select', { multiple: true, size: 3, onchange: e => f.helpers = [...e.target.selectedOptions].map(o => o.value) }, workersList().map(a => h('option', { value: a.name }, a.name))))),
      h('label', { class: 'f' }, h('span', {}, 'Depends on'), h('select', { multiple: true, size: 7, onchange: e => f.deps = [...e.target.selectedOptions].map(o => +o.value) }, others.map(o => h('option', { value: o.id }, `#${o.number} ${o.title.slice(0, 50)}`))))),
    h('label', { class: 'chk' }, h('input', { type: 'checkbox', onchange: e => f.start = e.target.checked }), 'Queue now (put it in the trigger column)'),
    h('div', { class: 'actions' }, h('button', { class: 'btn primary', onclick: () => act(async () => { const t = await api('/api/tickets', { method: 'POST', body: f }); S.tickets.set(t.id, t); openTicket(t.id); render(); }, 'Issue created') }, 'Create issue'),
      h('button', { class: 'btn ghost', onclick: closeDrawer }, 'Cancel')));
  $('#drawer').classList.remove('hidden');
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
  return h('div', { class: 'runs' },
    h('div', { class: 'runlist' }, S.runs.length ? S.runs.map(r => h('div', { class: `run ${r.id === S.selRun ? 'sel' : ''}`, onclick: () => { S.selRun = r.id; render(); } },
      h('div', { class: 'r1' }, h('span', { class: `st-${r.status}` }, r.status === 'running' ? h('span', { class: 'spin' }) : '●'), h('b', {}, r.kind), agentChip(r.agent), h('span', { class: 'muted', style: 'margin-left:auto' }, money(r.cost_usd))),
      h('div', { class: 'muted small' }, tk(r.ticket_id) ? `#${tk(r.ticket_id).number} ${tk(r.ticket_id).title.slice(0, 48)}` : r.kind === 'plan' ? 'planner' : '—', ' · ', ago(r.started_at), r.turns ? ` · ${r.turns} turns` : ''))) : h('div', { class: 'empty' }, 'No runs yet')),
    h('div', { style: 'display:flex;flex-direction:column;gap:8px;min-height:0' },
      sel ? h('div', { class: 'h', style: 'margin:0' }, h('h3', {}, `Run #${sel.id} — ${sel.kind} by ${sel.agent}`),
        h('div', { style: 'display:flex;gap:6px' }, sel.ticket_id ? h('button', { class: 'btn sm', onclick: () => openTicket(sel.ticket_id) }, 'Open ticket') : null,
          sel.status === 'running' ? h('button', { class: 'btn sm danger', onclick: () => act(() => api(`/api/runs/${sel.id}/cancel`, { method: 'POST' }), 'Cancel sent') }, '■ Cancel') : null)) : null,
      h('div', { class: 'log', id: 'log-main', style: 'flex:1' })));
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
  const W = 230, H = 62, GX = 300, GY = 84, pos = new Map();
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
      h('svg:rect', { class: 'box', width: W, height: H, rx: 10, fill: '#141822', stroke: getComputedStyle(document.documentElement).getPropertyValue(`--s-${t.state}`), 'stroke-width': 1.6 }),
      h('svg:rect', { width: 5, height: H, rx: 2, fill: agentColor(t.agent) }),
      h('svg:text', { x: 14, y: 20, fill: '#6b7486', 'font-size': 11, 'font-family': 'monospace' }, `#${t.number} · ${STATE_LABEL[t.state]}`),
      h('svg:text', { x: 14, y: 38, fill: '#e6e9ef', 'font-size': 12.5 }, t.title.length > 30 ? t.title.slice(0, 29) + '…' : t.title),
      h('svg:text', { x: 14, y: 54, fill: agentColor(t.agent), 'font-size': 11 }, t.agent ? `${cellOf(t.agent) ? '▣' : '◉'} ${t.agent}${(t.helpers || []).length ? ` +${t.helpers.length}` : ''}` : '○ unassigned'),
      h('svg:title', {}, `#${t.number} ${t.title}`)));
  }
  const svg = h('svg:svg', {}, h('svg:defs', {}, h('svg:marker', { id: 'arr', viewBox: '0 0 10 10', refX: 9, refY: 5, markerWidth: 7, markerHeight: 7, orient: 'auto-start-reverse' }, h('svg:path', { d: 'M0,0 L10,5 L0,10 z', fill: '#4b5563' }))), g);
  panZoom(svg, g, 'graphView');
  return h('div', {},
    filterBar([h('label', { class: 'chk', style: 'margin:0' }, h('input', { type: 'checkbox', checked: S.hideDone, onchange: e => { S.hideDone = e.target.checked; render(); } }), 'Hide done'),
      h('span', { class: 'muted small' }, 'Arrows = dependency (solid) / parent→sub-ticket (dashed). Drag to pan, wheel to zoom.')]),
    h('div', { class: 'canvas' }, ts.length ? svg : h('div', { class: 'empty' }, 'No tickets'),
      h('div', { class: 'legend' }, Object.keys(STATE_LABEL).map(s => h('span', { class: 'pill' }, h('span', { class: 'dot', style: `background:${sc(s)}` }), STATE_LABEL[s])))));
}
function panZoom(svg, g, key) {
  const v = S[key] ||= { x: 0, y: 0, k: 1 };
  const apply = () => g.setAttribute('transform', `translate(${v.x},${v.y}) scale(${v.k})`);
  apply();
  let drag = null;
  svg.addEventListener('mousedown', e => { if (e.target.closest('.gnode')) return; drag = { x: e.clientX - v.x, y: e.clientY - v.y }; });
  window.addEventListener('mousemove', e => { if (drag) { v.x = e.clientX - drag.x; v.y = e.clientY - drag.y; apply(); } });
  window.addEventListener('mouseup', () => drag = null);
  svg.addEventListener('wheel', e => {
    e.preventDefault(); const r = svg.getBoundingClientRect(), mx = e.clientX - r.left, my = e.clientY - r.top;
    const k2 = Math.min(2.5, Math.max(0.25, v.k * (e.deltaY < 0 ? 1.1 : 0.9)));
    v.x = mx - (mx - v.x) * k2 / v.k; v.y = my - (my - v.y) * k2 / v.k; v.k = k2; apply();
  }, { passive: false });
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
    const col = getComputedStyle(document.documentElement).getPropertyValue(`--s-${st}`);
    g.append(h('svg:g', { class: 'sm-node', id: `smn-${st}`, transform: `translate(${x - NW / 2},${y - NH / 2})` },
      h('svg:rect', { width: NW, height: NH, rx: 12, fill: n ? `${col}2a` : '#141822', stroke: col, 'stroke-width': n ? 2.2 : 1.4 }),
      h('svg:text', { x: NW / 2, y: 23, 'text-anchor': 'middle', fill: '#e6e9ef', 'font-size': 14, 'font-weight': 600 }, STATE_LABEL[st]),
      h('svg:text', { x: NW / 2, y: 42, 'text-anchor': 'middle', fill: n ? col : '#6b7486', 'font-size': 12 }, `${n} ticket${n === 1 ? '' : 's'}`)));
  }
  g.append(labels);
  if (S.smLabels) labels.classList.add('all');
  const svg = h('svg:svg', { viewBox: '0 0 1290 610' }, h('svg:defs', {}, h('svg:marker', { id: 'arr2', viewBox: '0 0 10 10', refX: 9, refY: 5, markerWidth: 7, markerHeight: 7, orient: 'auto-start-reverse' }, h('svg:path', { d: 'M0,0 L10,5 L0,10 z', fill: '#6b7486' }))), g);
  panZoom(svg, g, 'smView');
  return h('div', {},
    h('div', { class: 'toolbar' }, h('label', { class: 'chk', style: 'margin:0' }, h('input', { type: 'checkbox', checked: !!S.smLabels, onchange: e => { S.smLabels = e.target.checked; render(); } }), 'Show all transition labels'),
      h('span', { class: 'muted small' }, 'Hover an arrow to see its trigger. Transitions flash live as tickets move.')),
    h('div', { class: 'split' },
      h('div', { class: 'canvas' }, svg),
      h('div', { class: 'card', style: 'padding:12px' }, h('div', { class: 'h' }, h('h3', {}, 'Live transitions')),
        h('div', { class: 'feed' }, S.feed.length ? S.feed.map(e => { const t = S.tickets.get(e.ticket_id); return h('div', { class: 'ev', onclick: () => openTicket(e.ticket_id), style: 'cursor:pointer' },
          h('div', {}, h('b', {}, t ? `#${t.number}` : '?'), ' ', stateBadge(e.from), ' → ', stateBadge(e.to)), h('div', { class: 'muted' }, e.reason, ' · ', time(e.ts))); }) : h('div', { class: 'muted small' }, 'Waiting for activity…')))));
}
function flashTransition(e) {
  if (S.view !== 'machine') return;
  setTimeout(() => {
    for (const id of [`sme-${e.from}-${e.to}`, `smn-${e.to}`]) { const el = document.getElementById(id); if (el) { el.classList.add('flash'); setTimeout(() => el.classList.remove('flash'), 1400); } }
  }, 150);
}

// ───────── PLANNER
function viewPlanner() {
  const repos = [...new Set([...S.tickets.values()].map(t => t.repo))].sort();
  S.planRepo ||= repos[0] || '';
  const box = h('div', {},
    h('div', { class: 'card' }, h('div', { class: 'h' }, h('h2', {}, '✦ Planner'), h('span', { class: 'muted small' }, 'The orchestrator turns a goal into a dependency graph of GitHub issues.')),
      h('label', { class: 'f' }, h('span', {}, 'Goal'), h('textarea', { id: 'goal', rows: 5, style: 'font-family:inherit', placeholder: 'e.g. Add a dash ability to the player: design doc with tuning, engine support for i-frames, gameplay implementation, VFX hook.' }, S.goal || '')),
      h('div', { class: 'grid2' },
        h('label', { class: 'f' }, h('span', {}, 'Repository'), h('input', { id: 'plan-repo', list: 'repos', value: S.planRepo, placeholder: 'owner/repo', oninput: e => S.planRepo = e.target.value }), h('datalist', { id: 'repos' }, repos.map(r => h('option', { value: r })))),
        h('div', { style: 'display:flex;align-items:flex-end;padding-bottom:12px' },
          h('button', { class: 'btn primary', disabled: S.planning, onclick: () => { S.goal = $('#goal').value; S.planning = true; S.plan = null; render(); act(() => api('/api/plan', { method: 'POST', body: { goal: S.goal, repo: S.planRepo } })).then(r => { if (!r) { S.planning = false; render(); } }); } },
            S.planning ? [h('span', { class: 'spin' }), ' Orchestrator is thinking…'] : '✦ Generate plan')))));
  if (S.planning) box.append(h('div', { class: 'card', style: 'margin-top:14px' }, h('div', { class: 'muted' }, 'Follow it live in Runs & logs.')));
  if (S.plan) {
    const tk = S.plan.tickets || [];
    box.append(h('div', { class: 'card', style: 'margin-top:14px' },
      h('div', { class: 'h' }, h('h3', {}, `${tk.length} tickets`), S.plan.summary ? h('span', { class: 'muted small' }, S.plan.summary) : null),
      h('table', { class: 'plan-table' }, h('tr', {}, h('th', {}, 'Key'), h('th', { style: 'width:35%' }, 'Title / body'), h('th', {}, 'Agent / cell'), h('th', {}, 'Depends on'), h('th', {})),
        tk.map((t, i) => h('tr', {},
          h('td', { class: 'mono small' }, t.key),
          h('td', {}, h('input', { value: t.title, oninput: e => t.title = e.target.value, style: 'margin-bottom:6px' }), h('textarea', { oninput: e => t.body = e.target.value }, t.body)),
          h('td', {}, h('select', { onchange: e => t.agent = e.target.value }, assigneeOptions(t.agent)), (t.helpers || []).length ? h('div', { class: 'muted small', style: 'margin-top:4px' }, '+ ', t.helpers.join(', ')) : null),
          h('td', { class: 'small mono' }, (t.depends_on || []).join(', ') || '—'),
          h('td', {}, h('button', { class: 'btn sm danger', onclick: () => { tk.splice(i, 1); render(); } }, '✕'))))),
      h('div', { class: 'actions' },
        h('label', { class: 'chk', style: 'margin:0 10px 0 0' }, h('input', { type: 'checkbox', id: 'plan-start', checked: true }), 'Start immediately (put in trigger column & queue)'),
        h('button', { class: 'btn primary', onclick: () => act(async () => { await api('/api/plan/apply', { method: 'POST', body: { repo: S.planRepo, tickets: tk, start: $('#plan-start').checked } }); S.plan = null; render(); }, 'Issues created on GitHub') }, `Create ${tk.length} GitHub issues`),
        h('button', { class: 'btn ghost', onclick: () => { S.plan = null; render(); } }, 'Discard'))));
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
function viewAgents() {
  if (S.editing) return agentEditor();
  const stat = n => { const rs = S.runs.filter(r => r.agent === n); return { n: rs.length, cost: rs.reduce((a, r) => a + r.cost_usd, 0), live: rs.filter(r => r.status === 'running').length }; };
  return h('div', {},
    h('div', { class: 'h' }, h('h2', {}, 'Agents'), h('button', { class: 'btn primary', onclick: () => { S.editing = { kind: 'agent', name: null, raw: AGENT_TEMPLATE }; render(); } }, '+ New agent')),
    h('div', { class: 'agents' }, S.agents.map(a => { const st = stat(a.name); return h('div', { class: 'acard', style: `--ac:${a.color || '#7c8799'}`, onclick: () => editAgent(a.name) },
      h('div', { class: 'row' }, h('span', { class: 'dot', style: `background:${a.color}` }), h('h3', {}, a.name), h('span', { class: 'pill', style: 'margin-left:auto' }, a.role || 'worker'), st.live ? h('span', { class: 'spin' }) : null),
      h('p', {}, a.description),
      h('div', { class: 'tags' }, h('span', { class: 'pill' }, '⚙ ', a.model || 'default'), a.labels.map(l => h('span', { class: 'pill' }, '🏷 ', l)), a.skills.map(s => h('span', { class: 'pill', style: 'border-color:#7c5cff66' }, '✦ ', s))),
      h('div', { class: 'muted small', style: 'margin-top:8px' }, `${st.n} runs · ${money(st.cost)}`)); })),
    h('div', { class: 'h', style: 'margin-top:26px' }, h('h2', {}, 'Cells'), h('button', { class: 'btn', onclick: () => { S.editing = { kind: 'cell', name: null, raw: CELL_TEMPLATE }; render(); } }, '+ New cell')),
    (S.cells || []).length ? h('div', { class: 'agents' }, S.cells.map(c => { const st = stat(c.name); return h('div', { class: 'acard', style: `--ac:${c.color || '#eab308'}`, onclick: () => editCell(c.name) },
      h('div', { class: 'row' }, h('span', { class: 'dot', style: `background:${c.color || '#eab308'}` }), h('h3', {}, '▣ ', c.name), h('span', { class: 'pill', style: 'margin-left:auto' }, c.mode || 'delegate')),
      h('p', {}, c.description),
      h('div', { class: 'tags' }, c.lead ? h('span', { class: 'agent-chip', style: `--ac:${agentColor(c.lead)}` }, '★ ', c.lead) : null,
        c.members.map(m => h('span', { class: 'agent-chip', style: `--ac:${agentColor(m)}` }, m)), c.labels.map(l => h('span', { class: 'pill' }, '🏷 ', l))),
      h('div', { class: 'muted small', style: 'margin-top:8px' }, c.mode === 'pipeline' ? `order: ${[c.lead, ...c.members].filter(Boolean).join(' → ')}` : `${c.lead} delegates to ${c.members.join(', ') || 'nobody'}`)); }))
      : h('div', { class: 'muted small' }, 'No cells yet. A cell groups agents that work together on the same ticket.'),
    h('div', { class: 'h', style: 'margin-top:26px' }, h('h2', {}, 'Skills'), h('button', { class: 'btn', onclick: () => { const n = prompt('Skill name (a-z, 0-9, -)'); if (n) { S.editing = { kind: 'skill', name: n, isNew: true, raw: SKILL_TEMPLATE(n) }; render(); } } }, '+ New skill')),
    h('div', { class: 'agents' }, S.skills.map(s => h('div', { class: 'acard', style: '--ac:#7c5cff', onclick: () => editSkill(s.name) },
      h('div', { class: 'row' }, h('h3', {}, '✦ ', s.name)), h('p', {}, s.description),
      h('div', { class: 'muted small' }, 'used by: ', S.agents.filter(a => a.skills.includes(s.name)).map(a => a.name).join(', ') || '—')))));
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
  const del = () => { if (confirm(`Delete ${e.kind} ${e.name}?`)) act(async () => { await api(`/api/${isAgent ? 'agents' : isCell ? 'cells' : 'skills'}/${encodeURIComponent(e.name)}`, { method: 'DELETE' }); S.editing = null; await load(); }, 'Deleted'); };
  return h('div', {},
    h('div', { class: 'h' }, h('h2', {}, isAgent ? (e.name ? `Agent · ${e.name}` : 'New agent') : isCell ? (e.name ? `Cell · ${e.name}` : 'New cell') : `Skill · ${e.name}`),
      h('div', { style: 'display:flex;gap:6px' }, e.name && !e.isNew ? h('button', { class: 'btn danger', onclick: del }, 'Delete') : null,
        h('button', { class: 'btn ghost', onclick: () => { S.editing = null; render(); } }, 'Cancel'), h('button', { class: 'btn primary', onclick: save }, 'Save'))),
    h('div', { class: 'editor' }, ta,
      h('div', { class: 'card small', style: 'color:var(--tx2);overflow:auto;max-height:62vh' }, isAgent ? [
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

// ───────── SETTINGS
function viewSettings() {
  const s = structuredClone(S.settings);
  const inp = (k, label, help, type = 'text', extra = {}) => {
    const env = (S.envSecrets || []).includes(k);
    return h('label', { class: 'f' }, h('span', {}, label),
      h('input', { type, value: env ? '' : (s[k] ?? ''), disabled: env, placeholder: env ? 'Set in .env' : '', oninput: e => s[k] = type === 'number' ? +e.target.value : e.target.value, ...extra }),
      env ? h('small', {}, 'Provided by the environment (.env), not stored in the database') : help ? h('small', {}, help) : null);
  };
  const sel = (k, label, opts, help) => h('label', { class: 'f' }, h('span', {}, label),
    h('select', { onchange: e => { s[k] = e.target.value; } }, opts.map(([v, l]) => h('option', { value: v, selected: s[k] === v }, l))), help ? h('small', {}, help) : null);
  const chk = (k, label) => h('label', { class: 'chk' }, h('input', { type: 'checkbox', checked: !!s[k], onchange: e => s[k] = e.target.checked }), label);
  const save = () => act(async () => { S.settings = await api('/api/settings', { method: 'PUT', body: s }); await load(); }, 'Settings saved');
  const out = h('pre', { class: 'body-md mono small', style: 'display:none' });
  const test = (path, label) => h('button', { class: 'btn', onclick: async ev => {
    ev.target.disabled = true; out.style.display = 'block'; out.textContent = `${label}…`;
    try { await api('/api/settings', { method: 'PUT', body: s }); const r = await api(path, { method: 'POST' }); out.textContent = JSON.stringify(r, null, 2); } catch (e) { out.textContent = '✕ ' + e.message; }
    ev.target.disabled = false;
  } }, label);
  return h('div', { style: 'max-width:980px' },
    h('div', { class: 'h' }, h('h2', {}, 'Settings'), h('button', { class: 'btn primary', onclick: save }, 'Save')),
    h('div', { class: 'card' }, h('div', { class: 'h' }, h('h3', {}, '1 · Claude Code'), test('/api/settings/test-claude', 'Test Claude')),
      h('div', { class: 'grid2' },
        sel('claude_auth', 'Authentication', [['account', 'My Claude account (local `claude` login — Pro/Max)'], ['oauth_token', 'Long-lived token (`claude setup-token`)'], ['api_key', 'Anthropic API key (pay as you go)']],
          'Account mode = whatever `claude` is logged into on this machine (run `claude` once and /login).'),
        inp('claude_bin', 'Claude binary', 'Path or command name, e.g. claude or /opt/homebrew/bin/claude'),
        inp('claude_oauth_token', 'OAuth token', 'From `claude setup-token`. Prefer MAESTRO_CLAUDE_OAUTH_TOKEN in .env', 'password'),
        inp('anthropic_api_key', 'API key', 'Prefer MAESTRO_ANTHROPIC_API_KEY in .env', 'password'),
        sel('permission_mode', 'Permission mode for workers', [['acceptEdits', 'acceptEdits — edits auto-approved, tools per agent allow-list'], ['bypassPermissions', 'bypassPermissions — everything allowed (isolated machines only)'], ['default', 'default — only allow-listed tools']]),
        inp('default_model', 'Default model', 'Used when an agent has no model (empty = Claude Code default)'),
        inp('max_budget_usd', 'Max budget per run (USD, 0 = none)', 'Only meaningful for API-key billing', 'number', { step: '0.5' }))),
    h('div', { class: 'card', style: 'margin-top:14px' }, h('div', { class: 'h' }, h('h3', {}, '2 · GitHub'), test('/api/settings/test-github', 'Test GitHub')),
      h('div', { class: 'grid2' },
        sel('github_auth', 'Authentication', [['token', 'Personal access token'], ['gh_cli', 'Use `gh auth token` (GitHub CLI login)']]),
        inp('github_token', 'Token', 'Classic PAT with scopes repo + project. Prefer MAESTRO_GITHUB_TOKEN in .env', 'password'),
        inp('project_owner', 'Project owner (user or org)', 'e.g. Miou-zora'),
        inp('project_number', 'Project number', 'github.com/users/<owner>/projects/<number>', 'number'),
        inp('status_field', 'Status field name', 'Single-select field used as kanban columns'),
        inp('poll_seconds', 'Sync interval (seconds)', '', 'number')),
      h('div', { class: 'grid3' },
        inp('col_trigger', '▶ Trigger column', 'Cards moved here get picked up'),
        inp('col_in_progress', 'In-progress column', ''),
        inp('col_review', 'Review column', ''),
        inp('col_done', 'Done column', ''))),
    h('div', { class: 'card', style: 'margin-top:14px' }, h('h3', { style: 'margin-top:0' }, '3 · Engine'),
      h('div', { class: 'grid3' },
        inp('max_parallel', 'Parallel agents', 'Simultaneous Claude processes', 'number'),
        inp('max_attempts', 'Attempts per ticket', 'Before “needs human”', 'number'),
        inp('max_review_rounds', 'Review rounds', 'Fix ↔ review loops', 'number'),
        sel('orchestrator_agent', 'Orchestrator agent', S.agents.map(a => [a.name, a.name])),
        sel('reviewer_agent', 'Reviewer agent', S.agents.map(a => [a.name, a.name]))),
      chk('review_enabled', 'Reviewer agent checks every PR (and sends feedback back to the worker)'),
      chk('auto_merge', 'Auto-merge (squash) approved PRs'),
      chk('always_triage', 'Always ask the orchestrator (ignore label routing)')),
    h('div', { class: 'card', style: 'margin-top:14px' }, h('h3', { style: 'margin-top:0' }, '4 · Git'),
      h('div', { class: 'grid2' },
        inp('base_branch', 'Base branch', 'Empty = repository default branch'),
        h('div', { class: 'grid2' }, inp('git_author_name', 'Commit author', ''), inp('git_author_email', 'Commit email', ''))),
      h('label', { class: 'f' }, h('span', {}, 'Local repository paths (optional)'),
        h('textarea', { rows: 3, oninput: e => s.repo_paths = e.target.value, placeholder: 'owner/repo=/Users/you/dev/repo' }, s.repo_paths || ''),
        h('small', {}, 'One per line. Otherwise Maestro clones into its data dir. Each ticket always gets its own git worktree + branch.'))),
    h('div', { class: 'card', style: 'margin-top:14px' }, h('h3', { style: 'margin-top:0' }, '5 · MCP (ticket tools)'),
      h('p', { class: 'small muted' }, 'Worker agents get these tools automatically: list_tickets, get_ticket, list_agents, create_ticket, comment_ticket, close_ticket. Opt out per agent with ', h('code', {}, 'maestro_tools: false'), '.'),
      chk('mcp_auto_queue', 'Tickets created by agents are queued right away (otherwise they land in the backlog for you to start)'),
      h('p', { class: 'small', style: 'margin:10px 0 6px' }, 'Use the same tools from your own Claude Code:'),
      h('pre', { class: 'body-md mono small', style: 'max-height:none' }, S.mcp ? `claude mcp add --transport http maestro ${S.mcp.url} \\\n  --header "Authorization: Bearer ${S.mcp.token}"` : '')),
    out,
    h('div', { style: 'margin:14px 0 30px' }, h('button', { class: 'btn primary', onclick: save }, 'Save settings')));
}

load().catch(e => toast(e.message, 'error'));
connect();
setInterval(renderChrome, 15000);
