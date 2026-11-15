// ---- model: buildModel(data, systems, questions) -> realms, projects, changes, systems, links.
// The model holds what exists and how it is related. Every list is sorted by a stated rule, so input order never matters.
const cmp = (a, b) => (a < b ? -1 : a > b ? 1 : 0);
const by = (...keys) => (a, b) => { for (const k of keys) { const d = cmp(k(a), k(b)); if (d) return d; } return 0; };
const uniq = xs => Array.from(new Set(xs)).sort(cmp);

const PHASES = ['plan', 'build', 'review', 'land', 'merged'];
const PHASE_WORD = { plan: 'Planned', build: 'Building', review: 'Review', land: 'Landing', merged: 'Merged' };
const PHASE_LONG = { plan: 'Planned', build: 'Being built', review: 'In review', land: 'Landing', merged: 'Merged, still open' };
const STATUS_WORD = { live: 'In play', paused: 'Paused', stale: 'Stale' };
const STATE_RANK = { 'waiting-on-owner': 0, held: 1, active: 2, queued: 3, stale: 4, done: 5 };
const STATE_WORD = { 'waiting-on-owner': 'Waiting on the owner', held: 'Held', active: 'Active', queued: 'Queued', stale: 'Stale', done: 'Done, still open' };
const KNOW_WORD = { named: 'Named only', planned: 'Planned', built: 'Built' };
const FLAG_WORD = { stale: 'No recent work', superseded: 'Superseded by other work', 'stale-plan': 'Plan is out of date', 'empty-plan': 'Plan is empty', merge: 'Overlaps another change', landed: 'Pull request merged, change not closed' };
const KIND_WORD = { road: 'API calls', rail: 'Event streams', river: 'Logs and records' };
const KIND_ONE = { road: 'API call', rail: 'Event stream', river: 'Logs and records' };
const KIND_ORDER = ['road', 'rail', 'river'];
const SYS_KIND_WORD = { surveyed: 'Modules known', named: 'Named only', external: 'External' };
// The three findings that say a change's plan needs tidying. They get one quiet mark on the overview.
const TIDY = ['stale-plan', 'empty-plan', 'merge'];
const TIDY_WORD = { 'stale-plan': 'plan is out of date', 'empty-plan': 'plan is empty', merge: 'overlaps another change' };

// The overview's filters. A filter keeps a project when `project` says so, or when one of its changes passes `change`.
const FILTERS = [
  { id: 'all', word: 'All', about: 'Every project. Quiet projects fold into one line per realm.' },
  { id: 'owner', word: 'Waiting on the owner', about: 'Projects with an open question for the owner, and projects whose state is waiting on the owner.', project: p => p.needsOwner },
  { id: 'flight', word: 'In flight', about: 'Projects with a change that is being built, in review or landing, and is neither stale nor paused.', change: c => c.status === 'live' && (c.col === 'build' || c.col === 'review' || c.col === 'land') },
  { id: 'merged', word: 'Merged but open', about: 'Projects with a change whose pull request merged while the change stayed open.', change: c => c.col === 'merged' },
  { id: 'stale', word: 'Stale', about: 'Projects whose state is stale, and projects with a stale or superseded change.', project: p => p.state === 'stale', change: c => c.status === 'stale' },
  { id: 'tidy', word: 'Needs tidying', about: 'Projects with a change whose plan is out of date or empty, or that overlaps another change.', change: c => c.tidy.length > 0 },
];
const FILTER = Object.fromEntries(FILTERS.map(f => [f.id, f]));
const keeps = (f, p) => !!((f.project && f.project(p)) || (f.change && p.changes.some(f.change)));

// questions is the extract that extract-questions.js writes: the open questions with no answer, per project and per realm.
function buildModel(data, sys, questions) {
  const Q = questions || { asOf: null, projects: {}, unassigned: {} };
  const qsort = xs => (xs || []).map(q => ({ id: q.id, asked: q.asked, text: q.text, from: q.from || null })).sort(by(q => q.asked, q => q.id));
  const base = s => s.split('/').pop();
  const C = {}, P = {}, S = {};

  // Systems that have surveyed modules: a Cargo workspace from its crates, the others from the survey.
  const addSystem = (id, o) => (S[id] = Object.assign({ id, title: id, realm: null, kind: 'named', lang: null, loc: null, modules: [], M: {}, projects: [], changes: [], links: [] }, o));
  for (const w of data.workspaces) {
    const s = addSystem(w.id, { title: w.title, realm: w.realm, kind: 'surveyed', lang: w.lang });
    for (const c of w.crates) {
      s.modules.push({ id: c.id, sys: w.id, title: c.id, loc: c.loc, crate: true, purpose: null, deps: c.deps.slice().sort(cmp), changes: [],
        parts: c.modules.map(m => ({ id: m.id, loc: m.loc, items: m.items.slice().sort(cmp) })).sort(by(m => -m.loc, m => m.id)) });
    }
  }
  for (const c of sys.cities) {
    const s = addSystem(c.id, { title: c.title, realm: c.realm, kind: 'surveyed', lang: c.lang });
    for (const d of c.districts) s.modules.push({ id: d.id.slice(c.id.length + 1), sys: c.id, title: d.title, loc: d.loc, crate: false, purpose: d.purpose, deps: [], changes: [], parts: [] });
  }
  for (const s of Object.values(S)) { s.modules.sort(by(m => -m.loc, m => m.id)); s.loc = s.modules.reduce((t, m) => t + m.loc, 0); for (const m of s.modules) s.M[m.id] = m; }

  // Projects and their changes.
  const projects = data.projects.map(p => {
    const ids = p.changes.map(c => c.id).sort(cmp);
    const find = path => { const b = base(path); return ids.find(i => i === b) || ids.find(i => i.startsWith(b) || b.startsWith(i)); };
    const pairs = p.order.map(([a, b]) => [find(a), find(b)]).filter(([a, b]) => a && b).sort(by(x => x[0], x => x[1]));
    const rank = {}; // landing order: 1 + the longest chain of changes that must land first
    for (let pass = 0; pass <= pairs.length; pass++) for (const [a, b] of pairs) { rank[a] = rank[a] || 1; rank[b] = Math.max(rank[b] || 1, rank[a] + 1); }
    const changes = p.changes.map(c => {
      const f = c.findings.slice().sort(cmp), merged = f.includes('landed');
      const repo = c.repo && c.repo[0] !== '/' ? c.repo : null;
      const ch = { id: c.id, title: c.title, project: p.id, realm: p.realm, sys: repo, source: c.repo, date: /^\d{4}-/.test(c.date) ? c.date : null,
        col: merged ? 'merged' : c.state === 'paused' ? 'build' : c.state, phase: c.state,
        status: c.state === 'paused' ? 'paused' : f.includes('stale') || f.includes('superseded') ? 'stale' : 'live',
        flags: f, tidy: TIDY.filter(t => f.includes(t)), modules: c.crates.slice().sort(cmp), tasks: c.tasks.slice().sort(cmp), rank: rank[c.id] || 0,
        after: pairs.filter(x => x[1] === c.id).map(x => x[0]), before: pairs.filter(x => x[0] === c.id).map(x => x[1]) };
      C[c.id] = ch; return ch;
    }).sort(by(c => PHASES.indexOf(c.col), c => (c.status === 'live' ? 0 : 1), c => c.rank || 99, c => c.id));
    const know = !changes.length ? 'named' : changes.every(c => c.col === 'plan') ? 'planned' : 'built';
    const count = {}; for (const c of changes) if (c.sys) count[c.sys] = (count[c.sys] || 0) + 1;
    const systems = Object.keys(count).sort(by(s => -count[s], s => s));
    // Tags. A system whose changes name its modules (Board's crates) is shown as those modules, most changes first, then by name.
    const tags = systems.flatMap(s => {
      const n = {}; for (const c of changes) if (c.sys === s) for (const x of c.modules) if (S[s] && S[s].M[x]) n[x] = (n[x] || 0) + 1;
      const mods = Object.keys(n).sort(by(x => -n[x], x => x));
      return mods.length ? mods.map(x => ({ sys: s, mod: x, n: n[x] })) : [{ sys: s, mod: null, n: count[s] }];
    });
    const qs = qsort(Q.projects[p.id]), needsOwner = qs.length > 0 || p.state === 'waiting-on-owner';
    const pr = { id: p.id, title: p.title, realm: p.realm, state: p.state, summary: p.summary, know, changes, systems, tags, questions: qs, waiting: qs.length, needsOwner,
      quiet: !needsOwner && changes.every(c => c.status === 'stale') }; // quiet: no changes, or only stale ones, and nothing for the owner
    P[p.id] = pr; return pr;
  }).sort(by(p => (p.needsOwner ? 0 : 1), p => STATE_RANK[p.state], p => -p.changes.length, p => p.id));
  for (const l of data.links || []) { const a = C[base(l.before)], b = C[base(l.after)]; if (a && b) { a.before = uniq(a.before.concat(b.id)); b.after = uniq(b.after.concat(a.id)); } }

  // Every repository a change names is a system. Its realm is the realm of most of the projects that change it.
  for (const p of projects) for (const s of p.systems) if (!S[s]) addSystem(s, {});
  for (const p of projects) for (const s of p.systems) S[s].projects.push(p.id);
  for (const c of Object.values(C)) if (c.sys) { S[c.sys].changes.push(c.id); for (const m of c.modules) if (S[c.sys].M[m]) S[c.sys].M[m].changes.push(c.id); }
  for (const s of Object.values(S)) {
    s.changes.sort(cmp); for (const m of s.modules) m.changes.sort(cmp);
    if (!s.realm && s.projects.length) { const v = {}; for (const p of s.projects) v[P[p].realm] = (v[P[p].realm] || 0) + 1; s.realm = Object.keys(v).sort(by(r => -v[r], r => r))[0]; }
  }

  // Links. An end is a module of a system, a system, or a project. An end that is none of these is a system outside the repositories.
  const names = Object.fromEntries((sys.external || []).map(e => [e.id, e.title]));
  const end = ref => {
    const dot = ref.indexOf('.');
    if (dot > 0 && S[ref.slice(0, dot)] && S[ref.slice(0, dot)].M[ref.slice(dot + 1)]) return { sys: ref.slice(0, dot), mod: ref.slice(dot + 1) };
    if (S[ref]) return { sys: ref };
    if (P[ref]) return { project: ref };
    addSystem(ref, { title: names[ref] || ref, kind: 'external' }); return { sys: ref };
  };
  const links = sys.links.map(l => ({ id: l.id, name: l.name, kind: l.kind, prod: !!l.prod, traffic: l.traffic, note: l.buslane || null, a: end(l.from), b: end(l.to),
    controls: (l.symbols || []).map(s => ({ caption: s.caption, detail: s.detail, at: s.at })).sort(by(s => s.caption)) })).sort(by(l => KIND_ORDER.indexOf(l.kind), l => l.id));
  const L = {}; for (const l of links) { L[l.id] = l; for (const e of [l.a, l.b]) if (e.sys && !S[e.sys].links.includes(l.id)) S[e.sys].links.push(l.id); }

  const realms = data.realms.map(r => {
    const ps = projects.filter(p => p.realm === r.id);
    const unassigned = qsort(Q.unassigned[r.id]); // a realm's total is the sum of its rows and its unassigned questions
    return { id: r.id, title: r.title, unassigned, waiting: ps.reduce((t, p) => t + p.waiting, 0) + unassigned.length, projects: ps.map(p => p.id) };
  }).sort(by(r => -r.waiting, r => r.id));
  const systems = Object.values(S).sort(by(s => ({ surveyed: 0, named: 1, external: 2 })[s.kind], s => -s.changes.length, s => s.id));
  for (const s of systems) s.projects.sort(by(p => projects.indexOf(P[p])));
  const R = Object.fromEntries(realms.map(r => [r.id, r]));
  const allChanges = Object.values(C);
  const cols = Object.fromEntries(PHASES.map(k => [k, Math.max(0, ...projects.map(p => p.changes.filter(c => c.col === k).length))])); // widest cell per phase column
  const sysCols = Object.fromEntries(PHASES.map(k => [k, Math.max(0, ...systems.map(s => s.changes.filter(c => C[c].col === k).length))]));
  return { date: data.date, asOf: Q.asOf, missing: data.missing, realms, R, projects, P, C, systems, S, links, L, cols, sysCols, changes: allChanges.sort(by(c => c.id)), waiting: realms.reduce((t, r) => t + r.waiting, 0) };
}

// Reverses every list and every object's key order, for the determinism check. A pair inside a list is a tuple and keeps its order.
function reverseDeep(x, inList) {
  if (Array.isArray(x)) { const y = x.map(v => reverseDeep(v, true)); return inList ? y : y.reverse(); }
  if (x && typeof x === 'object') return Object.fromEntries(Object.keys(x).reverse().map(k => [k, reverseDeep(x[k], false)]));
  return x;
}
