// What the Agent hook checks in a builder's prompt that names a change in
// build, and the words of each piece of advice. Pure functions, so the tests
// can pin every word.

// briefOf stays as the artifacts mod copies it (a test there compares the two),
// so the object slicing below is this file's own.
import { briefOf } from './guard'

const NAME = String.raw`\/changes\/[\w.-]+`

/**
 * The changes `prompt` names, once each: those in a `board-brief:` line first,
 * then each `/changes/<name>` at the start of the text or after whitespace, a
 * backtick, a quote or `(`. So `plugins/changes/x` and `~/board/changes/x` name none.
 */
export function changesIn(prompt: string): string[] {
  const found = [
    ...[...prompt.matchAll(new RegExp(`board-brief:\\s*(${NAME})`, 'g'))].map(m => m[1] ?? ''),
    ...[...prompt.matchAll(new RegExp(`(?:^|[\\s\`'"(])(${NAME})`, 'g'))].map(m => m[1] ?? ''),
  ]
  return [...new Set(found.map(path => path.replace(/\.+$/, '')))]
}

/** The advice when the first non-empty line of `prompt` is not `board-brief: <path>`, or null. */
export function noLine(prompt: string, path: string): string | null {
  const first = prompt.split('\n').find(line => line.trim() !== '')?.trim() ?? ''
  // Given one line, briefOf reads only a brief line: a lone `{` holds no object.
  if (briefOf(first) === path) return null
  return (
    `changes::brief::no_line: if this agent builds ${path}, start its prompt with \`board-brief: ${path}\`, ` +
    `not \`${first.slice(0, 80)}\`.`
  )
}

/**
 * Every JSON object in `text` that opens on a line that is `{`, in order; one
 * that does not parse is skipped. Board prints the object pretty, so it ends at
 * the next line that is `}` with nothing but a line end after it.
 */
export function objectsIn(text: string): unknown[] {
  const lines = text.split('\n')
  return lines.flatMap((line, at) => {
    const end = line.trim() === '{' ? lines.findIndex((l, i) => i > at && l.trimEnd() === '}') : -1
    try {
      return end < 0 ? [] : [JSON.parse(lines.slice(at, end + 1).join('\n')) as unknown]
    } catch {
      return []
    }
  })
}

/** The fields of a brief the change record and its plan record decide. */
export type Expected = { brief_line: string; path: string; phase: string; plan: string | null }
const FIELDS = ['brief_line', 'path', 'phase', 'plan'] as const

const isRecord = (value: unknown): value is Record<string, unknown> =>
  value !== null && typeof value === 'object' && !Array.isArray(value)

/**
 * The advice when no JSON object in `prompt` that holds `brief_line` agrees
 * with `expected` on its brief line, path, phase and plan text, or null. It
 * names the first field that differs in the first such object. Other fields,
 * such as the rules, are not compared.
 */
export function notRendered(prompt: string, expected: Expected, board: string): string | null {
  const briefs = objectsIn(prompt).filter(isRecord).filter(sent => 'brief_line' in sent)
  const differs = briefs.map(sent => FIELDS.find(key => (sent[key] ?? null) !== expected[key]))
  if (differs.includes(undefined)) return null
  const key = differs[0]
  const what =
    key === undefined
      ? 'the prompt holds no brief as JSON'
      : `its \`${key}\` is ${JSON.stringify(briefs[0]?.[key] ?? null)}, and the change's is ${JSON.stringify(expected[key])}`
  return (
    `changes::brief::not_rendered: if this agent builds ${expected.path}, send it the brief ` +
    `\`board -C ${board} call changes.brief --path ${expected.path}\` prints now; ${what}.`
  )
}

/** The `text` of the plan record in a `board --json get` report, or null when it holds none. */
export function planText(stdout: string): string | null {
  try {
    const text = (JSON.parse(stdout) as { data?: { record?: { text?: unknown } }[] }).data?.[0]?.record?.text
    return typeof text === 'string' ? text : null
  } catch {
    return null
  }
}
