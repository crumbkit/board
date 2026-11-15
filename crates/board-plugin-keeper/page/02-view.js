// ---- view: render(model, route) -> { title, crumbs, main, panel }. Pure functions from the model to HTML text. No DOM, no measuring.
const esc = s => String(s).replace(/[&<>"]/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[c]);
const num = n => String(n).replace(/\B(?=(\d{3})+$)/g, ',');
const plural = (n, w, ws) => num(n) + ' ' + (n === 1 ? w : ws || w + 's');
const MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];
const day = (d, year) => (d ? Number(d.slice(8, 10)) + ' ' + MONTHS[Number(d.slice(5, 7)) - 1] + (year ? ' ' + d.slice(0, 4) : '') : '');

function parseRoute(hash) {
  const [path, query] = String(hash || '').replace(/^#\/?/, '').split('?');
  const seg = path.split('/').filter(Boolean).map(decodeURIComponent);
  const sel = /(?:^|&)sel=([^&]*)/.exec(query || '');
  // The overview's filter and its collapsed realms live in the address, in a fixed order, so one address always gives one page.
  const one = k => { const x = new RegExp('(?:^|&)' + k + '=([^&]*)').exec(query || ''); return x ? decodeURIComponent(x[1]) : ''; };
  const r = { page: 'overview', id: null, view: 'list', f: FILTER[one('f')] ? one('f') : 'all', shut: uniq(one('shut').split(',').filter(Boolean)), quiet: uniq(one('quiet').split(',').filter(Boolean)), sel: sel ? decodeURIComponent(sel[1]) : null };
  if (seg[0] === 'systems') r.page = 'systems';
  else if ((seg[0] === 'project' || seg[0] === 'system') && seg[1]) { r.page = seg[0]; r.id = seg[1]; if (seg[2] === 'timeline') r.view = 'timeline'; }
  return r;
}
const href = r => {
  const q = [];
  if (r.page === 'overview') { if (r.f && r.f !== 'all') q.push('f=' + r.f); if (r.shut && r.shut.length) q.push('shut=' + uniq(r.shut).map(encodeURIComponent).join(',')); if (r.quiet && r.quiet.length) q.push('quiet=' + uniq(r.quiet).map(encodeURIComponent).join(',')); }
  if (r.sel) q.push('sel=' + encodeURIComponent(r.sel).replace(/%3A/g, ':').replace(/%2F/g, '/'));
  return '#/' + (r.page === 'overview' ? '' : r.page === 'systems' ? 'systems' : r.page + '/' + encodeURIComponent(r.id) + (r.view === 'timeline' ? '/timeline' : '')) + (q.length ? '?' + q.join('&') : '');
};
const toProject = id => href({ page: 'project', id });
const toSystem = id => href({ page: 'system', id });

function render(m, route) {
  route = Object.assign({}, route, { f: FILTER[route.f] ? route.f : 'all', shut: (route.shut || []).filter(id => m.R[id]), quiet: (route.quiet || []).filter(id => m.R[id]) });
  const flip = (key, id) => href(Object.assign({}, route, { [key]: route[key].includes(id) ? route[key].filter(x => x !== id) : route[key].concat(id) }));
  const pick = sel => href(Object.assign({}, route, { sel }));
  const isSel = sel => route.sel === sel;

  // ---- marks and small parts
  const glyph = c => (c.status === 'paused' ? 'paused' : c.col);
  const svg = (g, cls) => `<svg class="m m-${g}${cls ? ' ' + cls : ''}" viewBox="0 0 14 14" aria-hidden="true"><use href="#m-${g}"/></svg>`;
  const markSvg = c => svg(glyph(c), c.status === 'live' ? '' : 'muted');
  const about = c => PHASE_LONG[c.col] + (c.status === 'live' ? '' : ', ' + STATUS_WORD[c.status].toLowerCase());
  const tidyText = c => c.tidy.map(t => TIDY_WORD[t]).join(', ');
  // A change whose plan needs tidying carries a corner dot. A change the filter does not pick is drawn faint.
  const mark = (c, off) => `<a class="mk${c.tidy.length ? ' tidy' : ''}${off ? ' off' : ''}${isSel('change:' + c.id) ? ' on' : ''}" href="${pick('change:' + c.id)}" title="${esc(c.title)} (${esc(about(c))})${c.tidy.length ? esc('. Needs tidying: ' + tidyText(c) + '.') : ''}" data-k="c:${esc(c.id)}">${markSvg(c)}</a>`;
  const short = t => (t.mod.startsWith(t.sys + '-') ? t.mod.slice(t.sys.length + 1) : t.mod);
  const ptag = t => (t.mod ? `<a class="tag t-crate" href="${href({ page: 'system', id: t.sys, sel: 'module:' + t.sys + '/' + t.mod })}" title="${esc(m.S[t.sys].title)} crate ${esc(t.mod)}, touched by ${plural(t.n, 'change')} of this project">${esc(short(t))}</a>` : tag(t.sys));
  const ptags = (ts, max) => ts.slice(0, max).map(ptag).join('') + (ts.length > max ? `<span class="more" title="${esc(ts.slice(max).map(t => t.mod || t.sys).join(', '))}">+${ts.length - max}</span>` : '');
  const tag = (id, link) => { const s = m.S[id]; return link === false ? `<span class="tag t-${s.kind}">${esc(s.title)}</span>` : `<a class="tag t-${s.kind}" href="${toSystem(id)}" title="${esc(SYS_KIND_WORD[s.kind])} system">${esc(s.title)}</a>`; };
  const wait = (n, long) => `<span class="wait" title="${plural(n, 'open question')} waiting on the owner">${num(n)}${long ? ' waiting on the owner' : ''}</span>`;
  const NO_Q = 'Waiting on the owner for a review, a merge or a decision. The board holds no open question for this project.';
  const waitCell = p => (p.waiting ? wait(p.waiting) : p.state === 'waiting-on-owner' ? `<span class="wait dot" title="${NO_Q}"></span>` : '');
  const phaseCells = (changes, empty, off) => '<div class="c-ph">' + (changes.length || !empty ? PHASES.map(k => `<div class="pc pc-${k}">${changes.filter(c => c.col === k).map(c => mark(c, off && off(c))).join('')}</div>`).join('') : `<span class="none">${empty}</span>`) + '</div>';
  const qItem = q => `<div class="part q"><span class="mono">${day(q.asked)}</span><span class="mono r">${esc(q.from ? q.from : q.id.split('/').pop())}</span><p>${esc(q.text)}</p></div>`;
  const phaseTotals = changes => '<div class="c-ph">' + PHASES.map(k => { const n = changes.filter(c => c.col === k).length; return `<div class="pc pc-${k} tot">${n || ''}</div>`; }).join('') + '</div>';
  const phaseHead = '<div class="c-ph">' + PHASES.map(k => `<div class="pc pc-${k}">${PHASE_WORD[k]}</div>`).join('') + '</div>';
  const widths = (cols, cap) => 'style="' + PHASES.map(k => `--w-${k}:${Math.max(4, Math.min(cap, cols[k]))}`).join(';') + '"'; // a phase column is as wide as its fullest cell, up to a cap
  const colVars = widths(m.cols, 8), sysVars = widths(m.sysCols, 10);
  const seg = items => '<nav class="seg">' + items.map(([label, h, on]) => `<a href="${h}"${on ? ' class="on" aria-current="page"' : ''}>${label}</a>`).join('') + '</nav>';
  const fact = (k, v) => (v === '' || v == null ? '' : `<div class="f"><dt>${k}</dt><dd>${v}</dd></div>`);
  const mono = s => `<span class="mono">${esc(s)}</span>`;
  const changeLink = id => { const c = m.C[id]; return `<a class="cl" href="${href({ page: 'project', id: c.project, sel: 'change:' + c.id })}">${markSvg(c)}<span>${esc(c.title)}</span></a>`; };
  const changeList = ids => (ids.length ? '<div class="cls">' + ids.map(changeLink).join('') + '</div>' : '');

  // The distribution of changes over the phases, as one stacked bar with its counts. It doubles as the key to the marks.
  const dist = changes => {
    const n = k => changes.filter(c => c.col === k).length, stale = changes.filter(c => c.status === 'stale').length, paused = changes.filter(c => c.status === 'paused').length, tidy = changes.filter(c => c.tidy.length).length;
    return '<div class="dist"><div class="bar" role="img" aria-label="Changes by phase">' + PHASES.filter(n).map(k => `<i class="b-${k}" style="flex-grow:${n(k)}"></i>`).join('') + '</div><ul class="key">'
      + PHASES.map(k => `<li${n(k) ? '' : ' class="zero"'}>${svg(k)}<span>${PHASE_LONG[k]}</span><b>${n(k)}</b></li>`).join('')
      + `<li class="sep${tidy ? '' : ' zero'}" title="The plan is out of date, the plan is empty, or the change overlaps another change."><span class="mk tidy">${svg('plan')}</span><span>Plan needs tidying</span><b>${tidy}</b></li>`
      + `<li${stale ? '' : ' class="zero"'}>${svg('plan', 'muted')}<span>Stale, drawn muted</span><b>${stale}</b></li><li${paused ? '' : ' class="zero"'}>${svg('paused', 'muted')}<span>Paused</span><b>${paused}</b></li></ul></div>`;
  };

  // ---- project rows, shared by the overview and a system's page
  const projectRow = (p, only, off) => {
    const cs = only ? p.changes.filter(c => c.sys === only) : p.changes;
    return `<div class="row k-${p.know}${isSel('project:' + p.id) ? ' on' : ''}" data-k="p:${esc(p.id)}">`
      + `<div class="c-title"><a class="t" href="${toProject(p.id)}" title="${esc(p.title)}">${esc(p.title)}</a></div>`
      + `<div class="c-state s-${p.state}">${STATE_WORD[p.state]}</div>`
      + `<div class="c-sys">${ptags(p.tags, 3)}</div>` + phaseCells(cs, 'Named only. No changes yet.', off) + `<div class="c-wait">${waitCell(p)}</div></div>`;
  };
  const projectHead = `<div class="thead"><div class="c-title">Project</div><div class="c-state">State</div><div class="c-sys">Systems</div>${phaseHead}<div class="c-wait">Waiting</div></div>`;

  // ---- overview: one section per realm
  const overview = () => {
    const F = FILTER[route.f], all = route.f === 'all';
    const head = `<header class="ph"><div class="ph-top"><h1>Work in play</h1>${seg([['Projects', href({ page: 'overview' }), true], ['Systems', href({ page: 'systems' }), false]])}</div>`
      + `<p class="meta"><span>${day(m.date, true)}</span><span>${plural(m.realms.length, 'realm')}</span><span>${plural(m.projects.length, 'project')}</span><span>${plural(m.changes.length, 'open change')}</span><span>${plural(m.systems.length, 'system')}</span>${m.missing ? `<span>${m.missing === 1 ? '1 open change is' : m.missing + ' open changes are'} not in the map of ${day(m.date)}</span>` : ''}<span class="attn">${plural(m.waiting, 'question')} waiting on the owner${m.asOf && m.asOf !== m.date ? ', counted ' + day(m.asOf) : ''}</span></p>${dist(m.changes)}</header>`;
    // The filter bar. Each filter states how many projects it keeps. The link at its right folds or unfolds every realm.
    const allShut = m.realms.every(r => route.shut.includes(r.id));
    const bar = '<div class="fbar"><nav class="seg filters" aria-label="Filter projects">' + FILTERS.map(f => `<a href="${href(Object.assign({}, route, { f: f.id }))}"${f.id === route.f ? ' class="on" aria-current="true"' : ''} title="${esc(f.about)}">${f.word}<b>${f.id === 'all' ? m.projects.length : m.projects.filter(p => keeps(f, p)).length}</b></a>`).join('') + '</nav>'
      + `<a class="fold" href="${href(Object.assign({}, route, { shut: allShut ? [] : m.realms.map(r => r.id) }))}">${allShut ? 'Expand all realms' : 'Collapse all realms'}</a></div>`;
    const caret = open => `<i class="caret${open ? ' dn' : ''}"></i>`;
    const sections = m.realms.map(r => {
      const ps = r.projects.map(id => m.P[id]), cs = ps.flatMap(p => p.changes), shut = route.shut.includes(r.id), openQuiet = route.quiet.includes(r.id);
      const kept = all ? ps : ps.filter(p => keeps(F, p)), quiet = all ? kept.filter(p => p.quiet) : [], shown = all ? kept.filter(p => !p.quiet) : kept;
      // A project the filter keeps for its own sake shows every mark. Otherwise the marks the filter does not pick are drawn faint.
      const row = p => projectRow(p, null, F.change && !(F.project && F.project(p)) ? c => !F.change(c) : null);
      const from = {}; for (const q of r.unassigned) from[q.from] = (from[q.from] || 0) + 1;
      const un = (all || route.f === 'owner') && r.unassigned.length ? `<div class="row line" data-k="u:${r.id}"><div class="c-title"><a class="t" href="${pick('realm:' + r.id)}" title="Open questions that no rule ties to a project of this realm">Not tied to a project</a><span class="sub">${esc(Object.keys(from).sort(by(k => -from[k], k => k)).map(k => k + ' ' + from[k]).join(', '))}</span></div><div class="c-wait">${wait(r.unassigned.length)}</div></div>` : '';
      const qline = quiet.length ? `<div class="row line" data-k="q:${r.id}"><div class="c-title"><a class="t" href="${flip('quiet', r.id)}" aria-expanded="${openQuiet}" title="Projects with no changes or only stale ones, and nothing waiting on the owner">${caret(openQuiet)}${plural(quiet.length, 'quiet project')}</a><span class="sub">${esc(quiet.map(p => p.title).join(', '))}</span></div>${phaseTotals(quiet.flatMap(p => p.changes))}<div class="c-wait"></div></div>` : '';
      const sub = all ? `${plural(ps.length, 'project')}, ${plural(cs.length, 'change')}` : `${kept.length ? kept.length : 'None'} of ${plural(ps.length, 'project')}`;
      const body = shut ? '' : shown.filter(p => p.needsOwner).map(row).join('') + un + shown.filter(p => !p.needsOwner).map(row).join('') + qline + (openQuiet ? quiet.map(row).join('') : '');
      return `<section class="realm" data-k="r:${r.id}"><div class="rh${isSel('realm:' + r.id) ? ' on' : ''}"><div class="c-title"><a class="tw" href="${flip('shut', r.id)}" aria-expanded="${!shut}" aria-label="${shut ? 'Expand' : 'Collapse'} ${esc(r.title)}" title="${shut ? 'Expand' : 'Collapse'} ${esc(r.title)}">${caret(!shut)}</a><a class="t" href="${pick('realm:' + r.id)}">${esc(r.title)}</a><span class="sub">${sub}</span></div><div class="c-state"></div><div class="c-sys"></div>${phaseTotals(cs)}<div class="c-wait">${r.waiting ? wait(r.waiting) : ''}</div></div>${body}</section>`;
    }).join('');
    return head + bar + `<div class="tbl t-projects" ${colVars}>${projectHead}${sections}</div>`;
  };

  // ---- systems: every system, grouped by realm, then every link by kind
  const endText = (e, plain) => (e.project ? (plain ? esc(m.P[e.project].title) : `<a href="${toProject(e.project)}">${esc(m.P[e.project].title)}</a>`) + ' <span class="dim">project</span>'
    : (plain ? `<b>${esc(m.S[e.sys].title)}</b>` : `<a href="${toSystem(e.sys)}"><b>${esc(m.S[e.sys].title)}</b></a>`) + (e.mod ? ` <span class="dim">${esc(m.S[e.sys].M[e.mod].title)}</span>` : ''));
  const linkRow = l => `<a class="row lrow${isSel('link:' + l.id) ? ' on' : ''}" href="${pick('link:' + l.id)}" data-k="l:${l.id}"><span class="c-name">${esc(l.name)}</span><span class="c-ends">${endText(l.a, true)}<i class="arr">→</i>${endText(l.b, true)}</span>`
    + `<span class="c-prod${l.prod ? '' : ' not'}">${l.prod ? 'In production' : 'Not in production'}</span><span class="c-ctl">${l.controls.map(c => esc(c.caption)).join(', ')}</span></a>`;
  const linkGroups = ls => KIND_ORDER.filter(k => ls.some(l => l.kind === k)).map(k => `<div class="grp"><h3><i class="lk lk-${k}"></i>${KIND_WORD[k]}<span class="n">${ls.filter(l => l.kind === k).length}</span></h3>${ls.filter(l => l.kind === k).map(linkRow).join('')}</div>`).join('');
  const linkHead = '<div class="thead"><div class="c-name">Link</div><div class="c-ends">From and to</div><div class="c-prod">Status</div><div class="c-ctl">Controls</div></div>';
  const systemRow = s => `<div class="row k-${s.kind}${isSel('system:' + s.id) ? ' on' : ''}" data-k="s:${esc(s.id)}"><div class="c-title"><a class="t" href="${toSystem(s.id)}">${esc(s.title)}</a></div><div class="c-kind">${SYS_KIND_WORD[s.kind]}</div>`
    + `<div class="c-n">${s.modules.length ? s.modules.length : ''}</div><div class="c-n">${s.loc ? num(s.loc) : ''}</div><div class="c-n">${s.projects.length || ''}</div>${phaseCells(s.changes.map(i => m.C[i]).sort(by(c => PHASES.indexOf(c.col), c => (c.status === 'live' ? 0 : 1), c => c.id)))}<div class="c-n">${s.links.length || ''}</div></div>`;
  const systemsPage = () => {
    const head = `<header class="ph"><div class="ph-top"><h1>Systems</h1>${seg([['Projects', href({ page: 'overview' }), false], ['Systems', href({ page: 'systems' }), true]])}</div>`
      + `<p class="meta"><span>${day(m.date, true)}</span><span>${plural(m.systems.length, 'system')}</span><span>${m.systems.filter(s => s.kind === 'surveyed').length} with known modules</span><span>${plural(m.links.length, 'link')} between systems</span></p>`
      + `<ul class="key solo"><li><span class="tag t-surveyed">name</span><span>Modules known</span></li><li><span class="tag t-named">name</span><span>Named only, by the changes that touch it</span></li><li><span class="tag t-external">name</span><span>Outside the repositories</span></li></ul></header>`;
    const groups = m.realms.map(r => [r.title, m.systems.filter(s => s.realm === r.id)]).concat([['Outside the repositories', m.systems.filter(s => !s.realm)]]).filter(g => g[1].length);
    const thead = `<div class="thead"><div class="c-title">System</div><div class="c-kind">Known as</div><div class="c-n">Modules</div><div class="c-n">Lines</div><div class="c-n">Projects</div>${phaseHead}<div class="c-n">Links</div></div>`;
    return head + `<div class="tbl t-systems" ${sysVars}>${thead}` + groups.map(([title, ss]) => `<section class="realm"><div class="rh"><div class="c-title"><span class="t">${esc(title)}</span><span class="sub">${plural(ss.length, 'system')}</span></div></div>${ss.map(systemRow).join('')}</section>`).join('') + '</div>'
      + `<section class="sec"><h2>Links between systems<span class="n">${m.links.length}</span></h2><div class="tbl t-links">${linkHead}${linkGroups(m.links)}</div></section>`;
  };

  // ---- a project: its changes as a list or as a phase timeline
  const flagText = c => c.flags.filter(f => f !== 'landed').map(f => FLAG_WORD[f]).concat(c.status === 'paused' ? ['Paused'] : []);
  const rowMark = c => (c.tidy.length ? `<span class="mk tidy" title="Needs tidying: ${esc(tidyText(c))}.">${markSvg(c)}</span>` : markSvg(c));
  const changeRow = c => `<a class="row crow${isSel('change:' + c.id) ? ' on' : ''}${c.status === 'live' ? '' : ' muted'}" href="${pick('change:' + c.id)}" data-k="c:${esc(c.id)}">${rowMark(c)}<span class="c-title">${esc(c.title)}</span>`
    + `<span class="c-flags">${flagText(c).map(esc).join(' · ')}</span><span class="c-sys">${c.sys ? tag(c.sys, false) : ''}</span><span class="c-ord">${c.rank ? 'Step ' + c.rank : ''}</span><span class="c-date mono">${day(c.date)}</span></a>`;
  const listView = p => PHASES.filter(k => p.changes.some(c => c.col === k)).map(k => { const cs = p.changes.filter(c => c.col === k); return `<div class="grp"><h3>${svg(k)}${PHASE_LONG[k]}<span class="n">${cs.length}</span></h3>${cs.map(changeRow).join('')}</div>`; }).join('');
  const timeline = p => {
    const cs = p.changes.slice().sort(by(c => -PHASES.indexOf(c.col), c => c.rank || 99, c => c.id));
    const row = c => { const i = PHASES.indexOf(c.col);
      return `<a class="row trow${isSel('change:' + c.id) ? ' on' : ''}${c.status === 'live' ? '' : ' muted'}" href="${pick('change:' + c.id)}" data-k="c:${esc(c.id)}"><span class="c-title">${esc(c.title)}</span>`
        + `<span class="track" style="--i:${i}"><i class="rail"></i><i class="done"></i>${PHASES.map((k, j) => `<i class="st${j < i ? ' past' : ''}" style="--j:${j}"></i>`).join('')}<span class="at">${markSvg(c)}</span></span>`
        + `<span class="c-ord">${c.rank ? 'Step ' + c.rank : ''}</span><span class="c-date mono">${day(c.date)}</span></a>`; };
    return `<div class="tbl t-time"><div class="thead"><div class="c-title">Change</div><div class="track">${PHASES.map((k, j) => `<span style="--j:${j}">${PHASE_WORD[k]}</span>`).join('')}</div><div class="c-ord">Order</div><div class="c-date">Updated</div></div>${cs.map(row).join('')}</div>`;
  };
  const crumb = (...xs) => xs.map(([t, h]) => (h ? `<a href="${h}">${esc(t)}</a>` : `<span>${esc(t)}</span>`)).join('<i>/</i>');
  const projectPage = p => {
    const r = m.R[p.realm];
    const head = `<header class="ph"><p class="eyebrow">Project in ${esc(r.title)}</p><h1>${esc(p.title)}</h1>${p.summary ? `<p class="lead">${esc(p.summary)}</p>` : ''}`
      + `<dl class="strip">${fact('State', `<span class="s-${p.state}">${STATE_WORD[p.state]}</span>`)}${fact('Known as', KNOW_WORD[p.know])}${fact('Open changes', p.changes.length)}${fact('Waiting on the owner', p.waiting ? wait(p.waiting) + (p.waiting === 1 ? ' question' : ' questions') : p.state === 'waiting-on-owner' ? `<span class="wait dot" title="${NO_Q}"></span> A review, a merge or a decision` : 'Nothing')}${fact('Systems', p.systems.length ? `<span class="tags">${p.systems.map(i => tag(i)).join('')}</span>` : 'None named')}${fact('Crates', p.tags.some(t => t.mod) ? `<span class="tags">${p.tags.filter(t => t.mod).map(ptag).join('')}</span>` : '')}</dl></header>`;
    const qs = p.questions.length ? `<section class="sec"><h2>Questions waiting on the owner<span class="n">${p.questions.length}</span></h2><div class="parts qs">${p.questions.map(qItem).join('')}</div></section>` : '';
    if (!p.changes.length) return head + qs + '<p class="empty">This project is named only. It has no changes yet.</p>';
    return head + qs + `<section class="sec"><div class="sec-top"><h2>Changes<span class="n">${p.changes.length}</span></h2>${seg([['List', href({ page: 'project', id: p.id }), route.view === 'list'], ['Timeline', href({ page: 'project', id: p.id, view: 'timeline' }), route.view === 'timeline']])}</div>${dist(p.changes)}`
      + (route.view === 'timeline' ? timeline(p) : `<div class="tbl t-changes">${listView(p)}</div>`) + '</section>';
  };

  // ---- a system: the projects that change it, its modules, and its links by kind
  const systemPage = s => {
    const max = Math.max(1, ...s.modules.map(x => x.loc)), isCrate = s.modules.some(x => x.crate);
    const head = `<header class="ph"><p class="eyebrow">System${s.realm ? ' in ' + esc(m.R[s.realm].title) : ' outside the repositories'}</p><h1>${esc(s.title)}</h1>`
      + `<dl class="strip">${fact('Known as', `<span class="tag t-${s.kind}">${SYS_KIND_WORD[s.kind]}</span>`)}${fact('Language', s.lang)}${fact('Lines', s.loc ? num(s.loc) : '')}${fact('Modules', s.modules.length || '')}${fact('Projects', s.projects.length)}${fact('Open changes', s.changes.length)}${fact('Links', s.links.length)}</dl></header>`;
    const ps = s.projects.map(id => m.P[id]);
    const projects = `<section class="sec"><h2>Projects that change it<span class="n">${ps.length}</span></h2>` + (ps.length ? `${dist(s.changes.map(i => m.C[i]))}<div class="tbl t-projects no-sys" ${colVars}>${projectHead}${ps.map(p => projectRow(p, s.id)).join('')}</div>` : '<p class="empty">No project in play changes this system.</p>') + '</section>';
    const modRow = x => `<a class="row mrow${isSel('module:' + s.id + '/' + x.id) ? ' on' : ''}" href="${pick('module:' + s.id + '/' + x.id)}" data-k="m:${esc(s.id + '/' + x.id)}"><span class="c-name mono">${esc(x.title)}</span>`
      + `<span class="c-purpose">${x.purpose ? esc(x.purpose) : `${plural(x.parts.length, 'module')}, depends on ${plural(x.deps.length, 'crate')}`}</span><span class="c-loc"><span class="mono">${num(x.loc)}</span><i class="lb"><i style="width:${Math.max(1, Math.round(100 * x.loc / max))}%"></i></i></span>`
      + `<span class="c-ch">${x.changes.slice(0, 9).map(i => markSvg(m.C[i])).join('')}${x.changes.length > 9 ? `<span class="more">+${x.changes.length - 9}</span>` : ''}</span></a>`;
    const modules = `<section class="sec"><h2>Modules<span class="n">${s.modules.length}</span></h2>` + (s.modules.length ? `<div class="tbl t-modules${s.modules.some(x => x.changes.length) ? '' : ' no-ch'}" style="--w-ch:${Math.max(5, Math.min(11, ...[Math.max(...s.modules.map(x => x.changes.length))]))}"><div class="thead"><div class="c-name">${isCrate ? 'Crate' : 'Module'}</div><div class="c-purpose">${isCrate ? 'Contents' : 'Purpose'}</div><div class="c-loc">Lines</div><div class="c-ch">Changes</div></div>${s.modules.map(modRow).join('')}</div>`
      : `<p class="empty">${s.kind === 'external' ? 'This system is outside the repositories. Its modules are not known.' : 'This system is named by the changes that touch it. Its modules have not been surveyed.'}</p>`) + '</section>';
    const ls = s.links.map(i => m.L[i]);
    const links = `<section class="sec"><h2>Links to other systems<span class="n">${ls.length}</span></h2>` + (ls.length ? `<div class="tbl t-links">${linkHead}${linkGroups(ls)}</div>` : '<p class="empty">No link to another system is recorded.</p>') + '</section>';
    return head + projects + modules + links;
  };

  // ---- the detail panel: the facts of the selected item
  const panel = () => {
    if (!route.sel) return '';
    const i = route.sel.indexOf(':'), kind = route.sel.slice(0, i), id = route.sel.slice(i + 1);
    const wrap = (eyebrow, title, body, open) => `<div class="p-head"><p class="eyebrow">${eyebrow}</p><a class="x" href="${pick(null)}" aria-label="Close panel" title="Close (Esc)">Close</a></div><h2>${title}</h2>${body}${open ? `<a class="open" href="${open[1]}">${open[0]}<i class="arr">→</i></a>` : ''}`;
    const phaseList = cs => PHASES.filter(k => cs.some(c => c.col === k)).map(k => `<span class="pl">${svg(k)}${cs.filter(c => c.col === k).length} ${PHASE_LONG[k].toLowerCase()}</span>`).join('') || 'None';
    if (kind === 'change' && m.C[id]) { const c = m.C[id], p = m.P[c.project];
      return wrap('Change', esc(c.title), `<dl class="facts">${fact('Phase', `<span class="pl">${markSvg(c)}${PHASE_LONG[c.col]}</span>`)}${fact('Status', STATUS_WORD[c.status])}${fact('Notes', flagText(c).concat(c.flags.includes('landed') ? [FLAG_WORD.landed] : []).map(esc).join('<br>'))}`
        + `${fact('Project', `<a href="${toProject(p.id)}">${esc(p.title)}</a>`)}${fact('Realm', esc(m.R[c.realm].title))}${fact('System', c.sys ? tag(c.sys) : `<span class="dim">No mounted repository</span>`)}${fact('Modules', c.modules.map(x => `<a class="mono" href="${href({ page: 'system', id: c.sys, sel: 'module:' + c.sys + '/' + x })}">${esc(x)}</a>`).join('<br>'))}`
        + `${fact('Updated', c.date ? day(c.date, true) : '<span class="dim">No date recorded</span>')}${fact('Landing order', c.rank ? 'Step ' + c.rank + ' of ' + Math.max(...p.changes.map(x => x.rank)) + ' in its project' : '')}${fact('Lands after', changeList(c.after))}${fact('Lands before', changeList(c.before))}${fact('Open tasks', c.tasks.map(esc).join('<br>'))}${fact('Id', mono(c.id))}</dl>`,
        route.page === 'project' && route.id === p.id ? null : ['Open project', href({ page: 'project', id: p.id, sel: 'change:' + c.id })]); }
    if (kind === 'realm' && m.R[id]) { const r = m.R[id], ps = r.projects.map(x => m.P[x]), cs = ps.flatMap(p => p.changes);
      return wrap('Realm', esc(r.title), `<dl class="facts">${fact('Waiting on the owner', r.waiting ? wait(r.waiting) + (r.waiting === 1 ? ' question' : ' questions') : 'Nothing')}${fact('Projects', ps.length)}${fact('By state', Object.keys(STATE_RANK).filter(k => ps.some(p => p.state === k)).map(k => `${ps.filter(p => p.state === k).length} ${STATE_WORD[k].toLowerCase()}`).join('<br>'))}${fact('Open changes', `<span class="pls">${phaseList(cs)}</span>`)}`
        + `${fact('Systems', `<span class="tags">${m.systems.filter(s => s.realm === r.id).map(s => tag(s.id)).join('')}</span>`)}</dl>`
        + (r.waiting ? `<h3>Questions by project<span class="n">${r.waiting}</span></h3><div class="parts">${ps.filter(p => p.waiting).map(p => `<div class="part"><a href="${toProject(p.id)}">${esc(p.title)}</a><span class="mono r">${p.waiting}</span></div>`).join('')}${r.unassigned.length ? `<div class="part"><span class="dim">Not tied to a project</span><span class="mono r">${r.unassigned.length}</span></div>` : ''}</div>` : '')
        + (r.unassigned.length ? `<h3>Not tied to a project<span class="n">${r.unassigned.length}</span></h3><div class="parts">${r.unassigned.map(qItem).join('')}</div>` : '')); }
    if (kind === 'module') { const [sid, mid] = id.split('/'), s = m.S[sid], x = s && s.M[mid]; if (!x) return '';
      return wrap(x.crate ? 'Crate' : 'Module', `<span class="mono">${esc(x.title)}</span>`, `<dl class="facts">${fact('System', tag(s.id))}${fact('Purpose', x.purpose ? esc(x.purpose) : '')}${fact('Lines', num(x.loc))}`
        + `${fact('Open changes', changeList(x.changes))}${fact('Depends on', x.deps.map(d => `<a class="mono" href="${pick('module:' + s.id + '/' + d)}">${esc(d)}</a>`).join('<br>'))}`
        + `${fact('Links', s.links.map(l => m.L[l]).filter(l => [l.a, l.b].some(e => e.sys === s.id && e.mod === x.id)).map(l => `<a href="${pick('link:' + l.id)}">${esc(l.name)}</a>`).join('<br>'))}</dl>`
        + (x.parts.length ? `<h3>Modules<span class="n">${x.parts.length}</span></h3><div class="parts">${x.parts.map(q => `<div class="part"><span class="mono">${esc(q.id)}</span><span class="mono r">${num(q.loc)}</span>${q.items.length ? `<p>${q.items.map(esc).join(', ')}</p>` : ''}</div>`).join('')}</div>` : '')); }
    if (kind === 'link' && m.L[id]) { const l = m.L[id];
      return wrap(KIND_ONE[l.kind], esc(l.name), `<dl class="facts">${fact('From', endText(l.a))}${fact('To', endText(l.b))}${fact('Kind', `<span class="pl"><i class="lk lk-${l.kind}"></i>${KIND_ONE[l.kind]}</span>`)}${fact('Status', l.prod ? 'In production' : 'Not in production')}${fact('Carries', l.note ? esc(l.note[0].toUpperCase() + l.note.slice(1)) : '')}${fact('Traffic', `<span class="meter" title="Relative traffic, ${l.traffic} of 9">${Array.from({ length: 9 }, (_, j) => `<i${j < l.traffic ? ' class="on"' : ''}></i>`).join('')}</span>`)}</dl>`
        + (l.controls.length ? `<h3>Controls<span class="n">${l.controls.length}</span></h3><div class="parts">${l.controls.map(c => `<div class="part"><span>${esc(c.caption)}</span><p>${esc(c.detail)}</p></div>`).join('')}</div>` : '')); }
    return '';
  };

  const p = route.page === 'project' ? m.P[route.id] : null, s = route.page === 'system' ? m.S[route.id] : null;
  const home = ['Work', href({ page: 'overview' })];
  if (p) return { title: p.title, crumbs: crumb(home, [m.R[p.realm].title, href({ page: 'overview', sel: 'realm:' + p.realm })], [p.title]), main: projectPage(p), panel: panel() };
  if (s) return { title: s.title, crumbs: crumb(home, ['Systems', href({ page: 'systems' })], [s.title]), main: systemPage(s), panel: panel() };
  if (route.page === 'systems') return { title: 'Systems', crumbs: crumb(home, ['Systems']), main: systemsPage(), panel: panel() };
  return { title: 'Work in play', crumbs: crumb(['Work']), main: overview(), panel: panel() };
}

// Every route the page can show, used by the determinism check and the screenshots.
function allRoutes(m) {
  const ids = m.realms.map(r => r.id).sort(cmp), fs = FILTERS.filter(f => f.id !== 'all').map(f => f.id);
  const views = fs.map(f => '#/?f=' + f).concat(ids.map(r => '#/?shut=' + r), ids.map(r => '#/?quiet=' + r), ['#/?shut=' + ids.join(','), '#/?quiet=' + ids.join(',')],
    fs.map(f => '#/?f=' + f + '&shut=' + ids[0]), ['#/?f=owner&shut=' + ids.join(','), '#/?shut=' + ids[0] + '&quiet=' + ids.join(','), '#/?f=tidy&sel=realm:' + ids[0]]);
  return ['#/', '#/systems'].concat(views, m.realms.map(r => '#/?sel=realm:' + r.id), m.projects.flatMap(p => ['#/project/' + p.id, '#/project/' + p.id + '/timeline']), m.changes.map(c => '#/project/' + c.project + '?sel=change:' + c.id),
    m.systems.map(s => '#/system/' + s.id), m.systems.flatMap(s => s.modules.map(x => '#/system/' + s.id + '?sel=module:' + s.id + '/' + x.id)), m.links.map(l => '#/systems?sel=link:' + l.id));
}
