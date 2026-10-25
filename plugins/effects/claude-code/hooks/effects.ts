// What board answered about pending effects, and the words the owner and the
// agent read. Pure functions, so the tests can pin every word.

import type { Effect, Pending } from '../types'
import { PENDING_SQL } from './query'

type Report = Record<string, unknown>

/** What one read of the pending effects found. */
export type Listed = Pick<Pending, 'effects' | 'truncated'>

function parseJson(text: string): Report | null {
  try {
    const value: unknown = JSON.parse(text)
    return typeof value === 'object' && value !== null ? (value as Report) : null
  } catch {
    return null
  }
}

/** Board's failure code and message, when `stdout` holds a Board failure report. */
export function boardFailure(stdout: string): string | null {
  const report = parseJson(stdout)
  return typeof report?.code === 'string' ? `${report.code} ${String(report.message ?? '')}`.trim() : null
}

/** Board's failure, or the exit status and stderr when it printed none. */
export const failure = (exitCode: number, stdout: string, stderr: string) =>
  boardFailure(stdout) ?? `exit ${exitCode} ${stderr.trim().slice(0, 500)}`.trim()

const ID = /^[0-9a-f]{64}$/

// The query's filter on every pending effect, which the one-effect query
// narrows to a single path.
const EVERY = "logical LIKE '/effects/pending/%'"

/**
 * The query for every pending effect, or for the one with `id`, which reads
 * that effect's record alone. An id that is not 64 hex digits matches none.
 */
export const pendingSql = (id?: string) =>
  id === undefined ? PENDING_SQL : PENDING_SQL.replace(EVERY, `logical = '/effects/pending/${ID.test(id) ? id : ''}'`)

/** Writes `home` at the start of `path` as `~`. */
const tilde = (path: string, home: string | undefined) =>
  home && (path === home || path.startsWith(`${home}/`)) ? `~${path.slice(home.length)}` : path

/** Reads `board --json query pendingSql()`; null when it printed no query report. A row whose id is not 64 hex digits is left out. */
export function parsePending(stdout: string, home?: string): Listed | null {
  const report = parseJson(stdout)
  const columns = (report?.summary as { columns?: string[] } | undefined)?.columns
  if (report?.schema !== 'board/query-report/v2' || !Array.isArray(report.data) || columns === undefined) return null
  const effects = (report.data as unknown[][]).flatMap(row => {
    const r = Object.fromEntries(columns.map((c, i) => [c, row[i]]))
    if (!ID.test(String(r.id))) return []
    const effect: Effect = {
      id: String(r.id),
      board: tilde(String(r.board), home),
      plugin: String(r.plugin),
      op: String(r.op),
      path: String(r.path),
      replaces: r.replaces === 1,
      nul: r.nul === 1,
      head: typeof r.head === 'string' ? r.head : null,
      bytes: Number(r.bytes ?? 0),
      lines: Number(r.lines ?? 0),
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

// The characters Board's `shown()` escapes (crates/board-tree/src/symlink.rs):
// controls, Unicode format characters, line and paragraph separators, and
// backslash.
const HIDDEN = /[\p{Cc}\p{Cf}\p{Zl}\p{Zp}\\]/gu

/**
 * Text with each character of `HIDDEN` escaped, as
 * `\\` or `\u{202e}`. A control character makes Claude Code refuse the whole
 * drawing, and a format character such as a bidi override or a zero-width
 * space could make a plugin's path read as another on the consent line.
 * Line feed and tab are kept unless `keepLines` is false.
 */
export const visible = (text: string, keepLines = true) =>
  text.replace(HIDDEN, c =>
    keepLines && (c === '\n' || c === '\t')
      ? c
      : c === '\\'
        ? '\\\\'
        : `\\u{${(c.codePointAt(0) as number).toString(16)}}`,
  )

/** The band's line. */
export const waitingLine = (n: number) => (n === 1 ? '1 effect waits for your answer' : `${n} effects wait for your answer`)

/** `text` escaped and in double quotes, with a double quote in it escaped, so it reads as one value. */
const quoted = (text: string) => `"${visible(text, false).replace(/"/g, '\\"')}"`

/**
 * One line naming where, who and what. The board comes first and the path is
 * quoted, because a plugin chooses the path, and a path such as
 * `/README.md in ~/Code/scratch` could otherwise name another board.
 */
export const headline = (e: Effect) => `In board ${quoted(e.board)}, ${visible(e.plugin, false)} asks to ${visible(e.op, false)} ${quoted(e.path)}`

/** The command that prints an effect whole. */
export const getCommand = (e: Effect) => `board --personal get /effects/pending/${e.id}`

const size = (bytes: number) => (bytes < 1024 ? `${bytes} bytes` : `${Math.round(bytes / 1024)} KB`)

/**
 * What the owner reads under the headline: the first lines of what a put
 * would write, how much is left out and how to read it whole, and, for an op
 * other than put, that an approval stands until its seen record is removed.
 */
export function preview(e: Effect, lines = 8, chars = 600): string {
  const parts: string[] = []
  if (e.head !== null) {
    let shown = e.head.split('\n').slice(0, lines).join('\n')
    // By code point, so a surrogate pair is never split.
    const points = [...shown]
    if (points.length > chars) shown = points.slice(0, chars).join('')
    shown = shown.replace(/\n$/, '')
    const whole = new TextEncoder().encode(e.head).length === e.bytes && shown === e.head.replace(/\n$/, '')
    parts.push(e.replaces ? 'It replaces the file there with:' : 'It writes a new file:', visible(shown))
    if (e.nul) {
      // SQLite's text functions stop at a NUL, so the head and the line count
      // cover only the part before it.
      parts.push(
        `… The content holds a NUL character, so only the part before it is shown. It is ${size(e.bytes)} in all. ${getCommand(e)} prints it whole.`,
      )
    } else if (!whole) {
      const more = e.lines - shown.split('\n').length
      const left = more > 0 ? `and ${more} more line${more === 1 ? '' : 's'}` : 'and the rest of the line'
      parts.push(`… ${left}, ${size(e.bytes)} in all. ${getCommand(e)} prints it whole.`)
    }
  } else if (e.replaces) {
    parts.push('It replaces the file there.')
  }
  if (e.op !== 'put') {
    parts.push(`An approval lets ${visible(e.plugin, false)} ${visible(e.op, false)} ${quoted(e.path)} again until /effects/seen/${e.id} is removed.`)
  }
  return parts.join('\n')
}

/** The dialog's question when a put is asked. */
export const question = (e: Effect) => `${headline(e)}. ${preview(e, 4, 300)}\nApprove it?`

/** What the agent that ran the put reads once the owner answered, or chose Later. */
export function told(e: Effect, answer: 'approve' | 'refuse' | 'later'): string {
  if (answer === 'later') {
    return `The owner has not answered pending effect ${e.id} yet: ${headline(e)}. Do not retry the put, and do not answer it yourself; it stays asked until the owner answers.`
  }
  return answer === 'approve'
    ? `The owner approved pending effect ${e.id}: ${headline(e)}. Run the same put again; it now writes.`
    : `The owner refused pending effect ${e.id}: ${headline(e)}. The same put is now refused with board::settings::refused, so do not retry it.`
}
