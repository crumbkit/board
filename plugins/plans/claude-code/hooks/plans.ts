// What board answered, and the records and words the mod makes from it. Pure
// functions, so the tests can pin every word.

import type { Open, Question } from '../types'

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

/** The query for the question's blob and body, which the answer's batch rewrites with its status answered. */
export const questionSql = (path: string) => `SELECT blob, body FROM records WHERE logical = ${quoted(path)}`

/** The project a question's path names, `/projects/<project>/questions/<id>`. */
export const project = (path: string) => path.split('/')[2] ?? path

/** Where a question's answer lives: the same id under `answers/`. */
export const answerPath = (path: string) => path.replace(/\/questions\/([^/]+)$/, '/answers/$1')

export const iso = (ms: number) => new Date(ms).toISOString().replace(/\.\d{3}Z$/, 'Z')

/**
 * The `board put --batch` lines that record an answer: the answer, which must
 * be new, and the question with its status answered, which must still hold
 * `blob`. Board writes both or neither.
 */
export function answerBatch(question: string, blob: string, body: string, choice: string, words: string, by: string, now: number): string {
  const answer = {
    $type: 'plans/answer/v1',
    question,
    choice,
    words,
    answered: iso(now),
    by,
    written_by: WRITTEN_BY,
  }
  const asked = { ...(JSON.parse(body) as object), status: 'answered' }
  return [
    JSON.stringify({ path: answerPath(question), expect: 'none', record: answer }),
    JSON.stringify({ path: question, expect: blob, record: asked }),
  ].join('\n')
}

/** What the asking session is sent. */
export const answerText = (question: string, choice: string, words: string) =>
  `The owner answered ${question}: choice ${choice}, in these words: ${words}\n` +
  `The answer is recorded at ${answerPath(question)}.`

/** The band's line. */
export const openLine = (total: number) => `${total} open question${total === 1 ? '' : 's'} for you`

// The report covers the day before the current one, from 07:00 local time,
// the day start the personal board's config names, to 07:00 today. SQLite
// works out the local times, daylight saving included, from board's own clock.
const DAY = "datetime('now', 'localtime', '-7 hours', 'start of day', '+7 hours'"
const at = (record: string, field: string) => `julianday(json_extract(${record}.body, '$.${field}'))`
const within = (record: string, field: string) => `${at(record, field)} >= w.since AND ${at(record, field)} < w.until`
const field = (record: string, name: string) => `json_extract(${record}.body, '$.${name}')`

/** One query for every record of the report, with the window's start as the first row. */
export const REPORT_SQL = [
  `WITH w AS (SELECT julianday(${DAY}, '-1 day', 'utc')) AS since, julianday(${DAY}, 'utc')) AS until)`,
  "SELECT 'since' AS kind, NULL AS path, datetime(w.since, 'localtime') AS at, NULL AS title, NULL AS detail FROM w",
  `UNION ALL SELECT 'decision', r.logical, ${field('r', 'decided')}, ${field('r', 'decision')}, ${field('r', 'review')}`,
  `FROM w, records r WHERE r.type = 'plans/decision/v1' AND ${within('r', 'decided')}`,
  `UNION ALL SELECT 'asked', r.logical, ${field('r', 'asked')}, ${field('r', 'question')}, ${field('r', 'status')}`,
  `FROM w, records r WHERE r.type = 'plans/question/v1' AND ${within('r', 'asked')}`,
  `UNION ALL SELECT 'answered', a.logical, ${field('a', 'answered')},`,
  `coalesce(${field('q', 'question')}, ${field('a', 'question')}), ${field('a', 'words')}`,
  `FROM w, records a LEFT JOIN records q ON q.logical = ${field('a', 'question')}`,
  `WHERE a.type = 'plans/answer/v1' AND ${within('a', 'answered')}`,
  `UNION ALL SELECT 'run', r.logical, ${field('r', 'finished')}, coalesce(${field('r', 'summary')}, ${field('r', 'by')}), ${field('r', 'by')}`,
  `FROM w, records r WHERE r.type = 'runs/run/v1' AND ${within('r', 'finished')}`,
  `UNION ALL SELECT 'stub', r.logical, ${field('r', 'decided')}, ${field('r', 'decision')}, ${field('r', 'refs[0]')}`,
  `FROM records r WHERE r.type = 'plans/decision/v1' AND ${field('r', 'written_by')} IS NOT NULL AND ${field('r', 'why')} = ''`,
  'ORDER BY 1, 3',
].join('\n')

type Row = { kind: string; path: string; at: string; title: string; detail: string | null }

const SECTIONS = [
  { kind: 'decision', heading: 'Decisions made', line: (r: Row) => `${r.title} \`${r.path}\`${r.detail === 'wanted' ? ', review wanted' : ''}` },
  { kind: 'asked', heading: 'Questions asked', line: (r: Row) => `${r.title} (${r.detail}) \`${r.path}\`` },
  { kind: 'answered', heading: 'Questions answered', line: (r: Row) => `${r.title} Answer: ${r.detail} \`${r.path}\`` },
  { kind: 'run', heading: 'Runs recorded', line: (r: Row) => `${r.title} (${r.detail}) \`${r.path}\`` },
  { kind: 'stub', heading: 'Pull requests whose decision stubs still want a why', line: (r: Row) => `${r.title} ${r.detail ?? 'names no pull request'} \`${r.path}\`` },
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
    `that want a why, since ${since}.`
  return { markdown, line }
}
