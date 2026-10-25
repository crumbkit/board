// What board answered about pending effects, and the words the owner and the
// agent read. Pure functions, so the tests can pin every word.

import type { Effect, Pending } from '../types'

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

const PENDING = '/effects/pending/'

/**
 * The pending effects that no seen record answers, oldest id first, or the one
 * with `id`. Board writes a seen record at the same id under `/effects/seen/`
 * and keeps the pending effect, so an answered effect is one with a seen record.
 */
export function pendingSql(id?: string): string {
  const one = id === undefined ? '' : ` AND e.logical = '${PENDING}${id.replaceAll("'", "''")}'`
  return (
    `SELECT substr(e.logical, ${PENDING.length + 1}) AS id, e.body FROM records e ` +
    `WHERE e.type = 'board/effect/v1' AND e.logical LIKE '${PENDING}%'${one} ` +
    "AND NOT EXISTS (SELECT 1 FROM records s WHERE s.logical = '/effects/seen/' || " +
    `substr(e.logical, ${PENDING.length + 1})) ORDER BY e.logical`
  )
}

/** Reads `board --json query pendingSql()`; null when it printed no query report. */
export function parsePending(stdout: string): Pending | null {
  const report = parseJson(stdout)
  if (report?.schema !== 'board/query-report/v2' || !Array.isArray(report.data)) return null
  const effects = (report.data as unknown[][]).flatMap(([id, body]) => {
    const record = parseJson(String(body))
    if (record === null) return []
    const content =
      typeof record.text === 'string' ? record.text : record.record === undefined ? null : JSON.stringify(record.record, null, 2)
    const effect: Effect = {
      id: String(id),
      board: String(record.board),
      plugin: String(record.plugin),
      op: String(record.op),
      path: String(record.path),
      content,
      replaces: typeof record.prior === 'string',
    }
    return [effect]
  })
  return { effects, truncated: report.truncated === true }
}

/**
 * The ids of the effects a Bash call's output says were asked. Board names the
 * id in the message of `board::settings::asked`, as text and as JSON alike.
 */
export function askedIds(output: string): string[] {
  if (!output.includes('board::settings::asked')) return []
  return [...new Set([...output.matchAll(/pending effect ([0-9a-f]{64})/g)].map(m => m[1] as string))]
}

/** The text of a Bash call's result: its stdout and stderr, or the error text. */
export function outputText(result: unknown): string {
  if (typeof result === 'string') return result
  if (typeof result !== 'object' || result === null) return ''
  const { stdout, stderr } = result as { stdout?: unknown; stderr?: unknown }
  return [stdout, stderr].filter(s => typeof s === 'string').join('\n')
}

/** The last segment of a board's root directory. */
export const boardName = (root: string) => root.split('/').filter(Boolean).at(-1) ?? root

/** The band's line. */
export const waitingLine = (n: number) => (n === 1 ? '1 effect waits for your answer' : `${n} effects wait for your answer`)

/** One line naming who asks for what, where. */
export const headline = (e: Effect) => `${e.plugin} asks to ${e.op} ${e.path} in ${boardName(e.board)}`

/** The first lines of what a put would write, cut short, with a note when it replaces a file. */
export function preview(e: Effect, lines = 8, chars = 600): string {
  if (e.content === null) return e.replaces ? 'It replaces the file there.' : ''
  const all = e.content.split('\n')
  let shown = all.slice(0, lines).join('\n')
  if (shown.length > chars) shown = shown.slice(0, chars)
  const cut = shown.length < e.content.trimEnd().length ? '\n…' : ''
  return `${e.replaces ? 'It replaces the file there with:' : 'It writes a new file:'}\n${shown.trimEnd()}${cut}`
}

/** The dialog's question when a put is asked. */
export const question = (e: Effect) => `${headline(e)}. ${preview(e, 4, 300)}\nApprove it?`

/** What the agent that ran the put reads once the owner answered. */
export function told(e: Effect, answer: 'approve' | 'refuse'): string {
  return answer === 'approve'
    ? `The owner approved pending effect ${e.id}: ${headline(e)}. Run the same put again; it now writes.`
    : `The owner refused pending effect ${e.id}: ${headline(e)}. The same put is now refused with board::settings::refused, so do not retry it.`
}
