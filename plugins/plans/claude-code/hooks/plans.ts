// What board answered, and the records and words the mod makes from it. Pure
// functions, so the tests can pin every word.

import type { Open, Question, Waiting } from '../types'

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

/** Board's failure code and message in `stdout`, or the exit status and stderr when it printed none. */
export function failure(exitCode: number, stdout: string, stderr: string): string {
  const report = parseJson(stdout)
  if (typeof report?.code === 'string') return `${report.code} ${String(report.message ?? '')}`.trim()
  return `exit ${exitCode} ${stderr.trim().slice(0, 500)}`.trim()
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

const quoted = (text: string) => `'${text.replaceAll("'", "''")}'`

/**
 * The query for the question's blob and body, which the answer's batch puts
 * back unchanged, and for every answer already naming the question, at its
 * own id or another.
 */
export const questionSql = (path: string) =>
  `SELECT 'question' AS kind, logical AS path, blob, body, ${OWN} AS own FROM records q WHERE logical = ${quoted(path)} ` +
  `UNION ALL SELECT 'answer', a.logical, a.blob, NULL, NULL FROM records a, records q ` +
  `WHERE q.logical = ${quoted(path)} AND a.type = 'plans/answer/v1' AND ${NAMES}`

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

/**
 * Why the owner's answer to `path` must not be recorded, or null when it may:
 * the question could not be read, already has an answer, or is no longer open.
 */
export function refusal(path: string, rows: readonly Record<string, unknown>[] | undefined): string | null {
  const question = rows?.find(r => r.kind === 'question')
  if (question === undefined) return `${path} could not be read, so nothing was recorded`
  const answers = (rows ?? []).filter(r => r.kind === 'answer').map(r => String(r.path))
  if (answers.length > 0) return `${path} already has an answer at ${answers.join(', ')}, so nothing was recorded`
  const status = (JSON.parse(String(question.body)) as { status?: unknown }).status
  return status === 'open' ? null : `${path} is ${String(status)} now, so nothing was recorded`
}

/** The project a question's path names, `/projects/<project>/questions/<id>`. */
export const project = (path: string) => path.split('/')[2] ?? path

/** Where a question's answer lives: the same id under `answers/`. */
export const answerPath = (path: string) => path.replace(/\/questions\/([^/]+)$/, '/answers/$1')

export const iso = (ms: number) => new Date(ms).toISOString().replace(/\.\d{3}Z$/, 'Z')

/**
 * The `board put --batch` lines that record an answer: the answer, which must
 * be new, and the question unchanged, which must still hold `blob`, so a
 * question withdrawn or edited since the read refuses the batch. The answer
 * is what marks the question answered; its status stays open. `by` names the
 * pane's own session by its `local_` id, and is left out when the pane
 * cannot name it. `own` is the question's path in its own board, which the
 * answer names it by. `choice` is undefined when the owner's words choose no
 * option, and the answer then leaves it out.
 */
export function answerBatch(question: string, own: string, blob: string, body: string, choice: string | undefined, words: string, by: string | undefined, now: number): string {
  // The answer lands in the question's board, where a reference is read from that board's root.
  const answer = {
    $type: 'plans/answer/v1',
    question: own,
    ...(choice === undefined ? {} : { choice }),
    words,
    answered: iso(now),
    ...(by === undefined ? {} : { by: { session: by } }),
    written_by: WRITTEN_BY,
  }
  return [
    JSON.stringify({ path: answerPath(question), expect: 'none', record: answer }),
    JSON.stringify({ path: question, expect: blob, record: JSON.parse(body) as object }),
  ].join('\n')
}

/** What the asking session is sent. */
export const answerText = (question: string, choice: string | undefined, words: string) =>
  `The owner answered ${question}: choice ${choice ?? 'none'}, in these words: ${words}\n` +
  `The answer is recorded at ${answerPath(question)}.`

/**
 * Reads `board --json call plans.waiting`, whose output is
 * `{total, blocking, line, questions, gates, effects, truncated, unreadable}`.
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
  if (report?.code === 'board::plugin::not_installed' && fields?.capability === 'waiting') return { kind: 'absent' }
  return { kind: 'failed', why: failure(exitCode, stdout, stderr) }
}

/** The Waiting button's label, `Waiting 14 · 11 blocking`; null when nothing waits. */
export function waitingLabel(w: Waiting): string | null {
  if (w.total === 0) return null
  return w.blocking === 0 ? `Waiting ${w.total}` : `Waiting ${w.total} · ${w.blocking} blocking`
}

/** The button's label while plans.waiting does not exist: the open questions; null when none are open. */
export const questionsLabel = (total: number) => (total === 0 ? null : `Questions ${total}`)

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
