// What board answered, and the records and words the mod makes from it. Pure
// functions, so the tests can pin every word.

import type { Open, Question, Unread, Waiting } from '../types'

/** The `written_by` of an answer the owner gave in the pane. */
export const WRITTEN_BY = 'plans/pane-answer'

type Report = Record<string, unknown>

function parseJson(text: string): Report | null {
  try {
    const value: unknown = JSON.parse(text)
    return typeof value === 'object' && value !== null ? (value as Report) : null
  } catch {
    return null
  }
}

/** Board's failure code and message in `stdout`; null when it printed no failure report. */
export function refused(stdout: string): string | null {
  const report = parseJson(stdout)
  return typeof report?.code === 'string' ? `${report.code} ${String(report.message ?? '')}`.trim() : null
}

/** Board's failure code and message in `stdout`, or the exit status and stderr when it printed none. */
export const failure = (exitCode: number, stdout: string, stderr: string): string =>
  refused(stdout) ?? `exit ${exitCode} ${stderr.trim().slice(0, 500)}`.trim()

/** What `board --json call plans.answer` returned: the answer's path, and the change it attached the answer to. */
export type Answered = { answer?: string; attached?: { change: string; refused?: string } }

/** Reads `board --json call plans.answer`'s output; empty when it printed no call report. */
export function parseAnswered(stdout: string): Answered {
  const output = (parseJson(stdout)?.data as { output?: Answered } | undefined)?.output
  return typeof output === 'object' && output !== null ? output : {}
}

/** What `board --json call changes.approve` returned: the owner's answer so far, and the effect that asks. */
export type Approved = { state?: 'asked' | 'approved' | 'refused'; effect?: string }

/** Reads `board --json call changes.approve`'s output; empty when it printed no call report. */
export function parseApproved(stdout: string): Approved {
  const output = (parseJson(stdout)?.data as { output?: Approved } | undefined)?.output
  return typeof output === 'object' && output !== null ? output : {}
}

/** The toast's sentence about the approval of `change`, by the state changes.approve returned. */
export function approvalText(change: string, approved: Approved): string {
  switch (approved.state) {
    case 'asked':
      return `. The approval of ${change} waits for Touch ID in the effects band`
    case 'approved':
      return `. The owner has already approved ${change} as its plan stands`
    case 'refused':
      return `. The owner refused the approval of ${change} earlier; delete /effects/seen/${approved.effect ?? '<effect>'} in the personal board to ask again`
    default:
      return `. changes.approve gave no state for ${change}; run board call changes.approve --path ${change}`
  }
}

/**
 * True when `text` names `path` whole, and not as part of a longer path, as the changes gate reads an option.
 * A `#`, or a `?` before a letter or digit, after the path joins it, as a fragment or a query does.
 */
export function namesWhole(text: string, path: string): boolean {
  // Whole code points on each side, as Rust's chars are, so a letter outside the BMP counts as one.
  const joined = (c: string | undefined) => c !== undefined && /[\p{L}\p{N}_/-]/u.test(c)
  for (let at = text.indexOf(path); at !== -1; at = text.indexOf(path, at + 1)) {
    const preceding = Array.from(text.slice(0, at))
    const before = preceding[preceding.length - 1]
    const [after, next] = Array.from(text.slice(at + path.length))
    const fragment = after === '#' || (after === '?' && /[\p{L}\p{N}]/u.test(next ?? ''))
    if (!joined(before) && !joined(after) && !fragment) return true
  }
  return false
}

/**
 * The change the owner's answer approves: the change plans.answer attached
 * the answer to, when the chosen option's text names it. Null otherwise.
 */
export function approvedChange(q: Question, choice: string | undefined, attached: Answered['attached']): string | null {
  if (attached === undefined || attached.refused !== undefined) return null
  const text = q.options.find(o => o.id === choice)?.text
  return text !== undefined && namesWhole(text, attached.change) ? attached.change : null
}

/** Reads `board --json call plans.open`; null when it printed no call report. */
export function parseOpen(stdout: string): Open | null {
  const report = parseJson(stdout)
  const output = (report?.data as { output?: { questions?: Question[]; total?: number } } | undefined)?.output
  if (report?.schema !== 'board/call-report/v2' || !Array.isArray(output?.questions)) return null
  return { questions: output.questions, total: output.total ?? output.questions.length }
}

/** Reads `board --json query`: each row as an object keyed by its column. */
export function parseRows(stdout: string): { rows: Record<string, unknown>[]; truncated: boolean } | null {
  const report = parseJson(stdout)
  const columns = (report?.summary as { columns?: string[] } | undefined)?.columns
  if (report?.schema !== 'board/query-report/v2' || !Array.isArray(report.data) || columns === undefined) return null
  const rows = (report.data as unknown[][]).map(row => Object.fromEntries(columns.map((c, i) => [c, row[i]])))
  return { rows, truncated: report.truncated === true }
}

/** The question `q`'s path in its own board, which a reference from that board names it by. */
const OWN = "'/' || CASE WHEN q.path LIKE '%.json' THEN substr(q.path, 1, length(q.path) - 5) ELSE q.path END"

/**
 * True when the answer `a` names the question `q`: by its path in the board
 * both are in, since a mounted board's reference is read from that board's
 * root, or by its logical path from an answer in the root board or in the
 * question's own board. plans.open
 * and plans.answer use the same rule (question::NAMES in the plugin's crate).
 */
export const NAMES =
  "(((a.source = 'worktree' OR a.source = q.source) AND json_extract(a.body, '$.question') = q.logical) " +
  `OR (a.source = q.source AND json_extract(a.body, '$.question') = ${OWN}))`

/** The project a question's path names, `/projects/<project>/questions/<id>`. */
export const project = (path: string) => path.split('/')[2] ?? path

/** What a question holds up, as its row says it: the change's name, or the work in words, or nothing. */
export const blocking = (q: { blocks: string | null; blocked_work: string | null }) => {
  const what = q.blocks === null ? q.blocked_work : (q.blocks.split('/').pop() ?? q.blocks)
  return what === null ? '' : ` · blocks ${what}`
}

/** What the asking session is sent; `answer` is the answer's path as plans.answer returned it. */
export const answerText = (question: string, answer: string | undefined, choice: string | undefined, words: string) =>
  `The owner answered ${question}: choice ${choice ?? 'none'}, in these words: ${words}` +
  (answer === undefined ? '' : `\nThe answer is recorded at ${answer}.`)

/**
 * Reads `board --json call plans.waiting`, whose output is
 * `{total, blocking, line, unreadable}`.
 * `absent` when the plans plugin has no `waiting` capability yet, so the
 * band counts open questions instead; `failed` with Board's words otherwise.
 */
export function parseWaiting(exitCode: number, stdout: string, stderr: string): { kind: 'read'; waiting: Waiting } | { kind: 'absent' } | { kind: 'failed'; why: string } {
  const report = parseJson(stdout)
  const output = (report?.data as { output?: { total?: unknown; blocking?: unknown } } | undefined)?.output
  if (exitCode === 0 && report?.schema === 'board/call-report/v2' && typeof output?.total === 'number') {
    return { kind: 'read', waiting: { total: output.total, blocking: typeof output.blocking === 'number' ? output.blocking : 0 } }
  }
  const fields = report?.fields as { capability?: unknown } | undefined
  // An older board gives a missing capability the code `not_installed`.
  const missing = report?.code === 'board::plugin::no_capability' || report?.code === 'board::plugin::not_installed'
  if (missing && fields?.capability === 'waiting') return { kind: 'absent' }
  return { kind: 'failed', why: failure(exitCode, stdout, stderr) }
}

/** The Waiting button's label, `Waiting 14 · 11 blocking`; null when nothing waits. */
export function waitingLabel(w: Waiting): string | null {
  if (w.total === 0) return null
  return w.blocking === 0 ? `Waiting ${w.total}` : `Waiting ${w.total} · ${w.blocking} blocking`
}

/** The button's label while plans.waiting does not exist: the open questions; null when none are open. */
export const questionsLabel = (total: number) => (total === 0 ? null : `Questions ${total}`)

/** Failure codes the band does not report: no personal board, and no plans plugin in it. */
const QUIET = ['board::config::not_found', 'board::plugin::not_installed']

/**
 * The band's count button. A count that is read draws as before. A failed
 * read of either count draws in the warning colour: `Questions 4: the last
 * read failed` beside the last count read, or `Questions could not be read`
 * when none ever was. Nothing is drawn for a failure that only says there is
 * no personal board or no plans plugin, and nothing when the count is zero.
 */
export function countBadge(waiting: Waiting | null, open: Open | null, failed: Unread): { label: string; problem: boolean } | null {
  const label = waiting !== null ? waitingLabel(waiting) : open === null ? null : questionsLabel(open.total)
  // The failure that matters is that of the read whose count is drawn. With no
  // count drawn, any failure that is not quiet says the questions could not be read.
  const loud = (why: string | null) => (why === null || QUIET.some(code => why.startsWith(code)) ? null : why)
  const why = waiting !== null ? loud(failed.waiting) : open !== null ? loud(failed.open) : (loud(failed.open) ?? loud(failed.waiting))
  if (why === null) return label === null ? null : { label, problem: false }
  if (waiting === null && open === null) return { label: 'Questions could not be read', problem: true }
  return { label: `${label ?? (waiting !== null ? 'Waiting 0' : 'Questions 0')}: the last read failed`, problem: true }
}

/** `text` with each control character as a space, so a message from Board draws on one line. */
// eslint-disable-next-line no-control-regex
export const plain = (text: string) => text.replace(/[\u0000-\u001f\u007f-\u009f]/g, ' ')

// The report covers the day before the current one, from 07:00 local time
// to 07:00 today. The 07:00 day start is fixed here; no config changes it. SQLite
// works out the local times, daylight saving included, from board's own clock.
const DAY = "datetime('now', 'localtime', '-7 hours', 'start of day', '+7 hours'"
const at = (record: string, field: string) => `julianday(json_extract(${record}.body, '$.${field}'))`
const within = (record: string, field: string) => `${at(record, field)} >= w.since AND ${at(record, field)} < w.until`
const field = (record: string, name: string) => `json_extract(${record}.body, '$.${name}')`

/** The most rows the report reads; the line says when Board cut it there. */
export const REPORT_LIMIT = 1000

/** One query for every record of the report, with the window's start as the first row. */
export const REPORT_SQL = [
  `WITH w AS (SELECT julianday(${DAY}, '-1 day', 'utc')) AS since, julianday(${DAY}, 'utc')) AS until)`,
  "SELECT 'since' AS kind, NULL AS path, datetime(w.since, 'localtime') AS at, NULL AS title, NULL AS detail FROM w",
  `UNION ALL SELECT 'decision', r.logical, ${field('r', 'decided')}, ${field('r', 'decision')}, ${field('r', 'review')}`,
  `FROM w, records r WHERE r.type = 'plans/decision/v1' AND ${within('r', 'decided')}`,
  `UNION ALL SELECT 'asked', r.logical, ${field('r', 'asked')}, ${field('r', 'question')},`,
  // A question is answered when an answer names it; its status says only open or withdrawn.
  `CASE WHEN EXISTS (SELECT 1 FROM records a, records q WHERE q.logical = r.logical AND a.type = 'plans/answer/v1' AND ${NAMES})`,
  `THEN 'answered' ELSE ${field('r', 'status')} END`,
  `FROM w, records r WHERE r.type = 'plans/question/v1' AND ${within('r', 'asked')}`,
  `UNION ALL SELECT 'answered', a.logical, ${field('a', 'answered')},`,
  `coalesce(${field('q', 'question')}, ${field('a', 'question')}), ${field('a', 'words')}`,
  `FROM w, records a LEFT JOIN records q ON q.type = 'plans/question/v1' AND ${NAMES}`,
  `WHERE a.type = 'plans/answer/v1' AND ${within('a', 'answered')}`,
  `UNION ALL SELECT 'run', r.logical, ${field('r', 'finished')}, coalesce(${field('r', 'summary')}, ${field('r', 'by')}), ${field('r', 'by')}`,
  `FROM w, records r WHERE r.type = 'runs/run/v1' AND ${within('r', 'finished')}`,
  `UNION ALL SELECT 'stub', r.logical, ${field('r', 'decided')}, ${field('r', 'decision')}, ${field('r', 'urls[0]')}`,
  `FROM records r WHERE r.type = 'plans/decision/v1' AND ${field('r', 'written_by')} IS NOT NULL AND ${field('r', 'why')} = ''`,
  'ORDER BY 1, 3',
].join('\n')

type Row = { kind: string; path: string; at: string; title: string; detail: string | null }

const SECTIONS = [
  { kind: 'decision', heading: 'Decisions made', line: (r: Row) => `${r.title} \`${r.path}\`${r.detail === 'wanted' ? ', review wanted' : ''}` },
  { kind: 'asked', heading: 'Questions asked', line: (r: Row) => `${r.title} (${r.detail}) \`${r.path}\`` },
  { kind: 'answered', heading: 'Questions answered', line: (r: Row) => `${r.title} Answer: ${r.detail} \`${r.path}\`` },
  { kind: 'run', heading: 'Runs recorded', line: (r: Row) => `${r.title} (${r.detail}) \`${r.path}\`` },
  { kind: 'stub', heading: 'Decision stubs that still want a why', line: (r: Row) => `${r.title} ${r.detail ?? 'names no pull request'} \`${r.path}\`` },
] as const

const plural = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`

/** The report as Markdown for the pane, and the one line the command prints. */
export function report(rows: readonly Record<string, unknown>[], truncated: boolean): { markdown: string; line: string } {
  const all = rows as readonly Row[]
  const since = all.find(r => r.kind === 'since')?.at.slice(0, 16) ?? 'yesterday 07:00'
  const of = (kind: string) => all.filter(r => r.kind === kind)
  const sections = SECTIONS.map(s => {
    const found = of(s.kind)
    const lines = found.length === 0 ? ['- none'] : found.map(r => `- ${s.line(r)}`)
    return `## ${s.heading} (${found.length})\n\n${lines.join('\n')}`
  })
  const cut = truncated ? '\n\nBoard returned only the first rows; `board query` shows the rest.' : ''
  const markdown = `Records from ${since} until 07:00 today.\n\n${sections.join('\n\n')}${cut}`
  const line =
    `board: ${plural(of('decision').length, 'decision', 'decisions')}, ` +
    `${plural(of('asked').length, 'question', 'questions')} asked, ${of('answered').length} answered, ` +
    `${plural(of('run').length, 'run', 'runs')} and ${plural(of('stub').length, 'decision stub', 'decision stubs')} ` +
    `that want a why, since ${since}${truncated ? `, counting only the first ${REPORT_LIMIT} records` : ''}.`
  return { markdown, line }
}
