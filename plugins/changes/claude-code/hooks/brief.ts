// What the Agent hook checks in a builder's prompt that names a change in
// build, and the words of each piece of advice. Pure functions, so the tests
// can pin every word.

// briefOf stays as the artifacts mod copies it (a test there compares the two),
// so the object slicing below is this file's own.
import { briefOf } from './guard'

/** The first change `prompt` names, as `/changes/<name>`, or null. */
export const changeIn = (prompt: string) => prompt.match(/\/changes\/[\w.-]+/)?.[0].replace(/\.+$/, '') ?? null

/** The advice when the first non-empty line of `prompt` is not `board-brief: <path>`, or null. */
export function noLine(prompt: string, path: string): string | null {
  const first = prompt.split('\n').find(line => line.trim() !== '')?.trim() ?? ''
  // Given one line, briefOf reads only a brief line: a lone `{` holds no object.
  if (briefOf(first) === path) return null
  return (
    `changes::brief::no_line: this prompt names ${path}, and its first line is \`${first.slice(0, 80)}\`. ` +
    `The edit guard reads the builder's first line, so put \`board-brief: ${path}\` first.`
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

/** `value` as JSON with every object's keys sorted, so two values compare by content. */
const canonical = (value: unknown): string =>
  JSON.stringify(value, (_, v: unknown) =>
    v !== null && typeof v === 'object' && !Array.isArray(v)
      ? Object.fromEntries(Object.entries(v).sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0)))
      : v,
  )

/** The first key, in sorted order, whose value differs between `sent` and `rendered`, or null when none does. */
function differsAt(sent: Record<string, unknown>, rendered: Record<string, unknown>): string | null {
  const keys = [...new Set([...Object.keys(sent), ...Object.keys(rendered)])].sort()
  return keys.find(key => canonical(sent[key]) !== canonical(rendered[key])) ?? null
}

const isRecord = (value: unknown): value is Record<string, unknown> =>
  value !== null && typeof value === 'object' && !Array.isArray(value)

/**
 * The advice when no JSON object in `prompt` equals `rendered`, what
 * `changes.brief` renders for `path` in `board` now, or null. It names the
 * first differing key of the object that holds a `brief_line`, or else of
 * the first object.
 */
export function notRendered(prompt: string, rendered: Record<string, unknown>, path: string, board: string): string | null {
  const objects = objectsIn(prompt).filter(isRecord)
  const keys = objects.map(sent => differsAt(sent, rendered))
  if (keys.includes(null)) return null
  const at = Math.max(0, objects.findIndex(sent => 'brief_line' in sent))
  const what = objects.length === 0 ? 'the prompt holds no brief as JSON' : `its brief differs at \`${keys[at]}\``
  return (
    `changes::brief::not_rendered: ${what}, so it is not what changes.brief renders for ${path} now. Render it ` +
    `with \`board -C ${board} call changes.brief --path ${path}\` and send its whole output after the brief line.`
  )
}

/** The brief in a `board --json call changes.brief` report, or null when it holds none. */
export function parseRendered(stdout: string): Record<string, unknown> | null {
  try {
    const output = (JSON.parse(stdout) as { data?: { output?: unknown } }).data?.output
    return isRecord(output) ? output : null
  } catch {
    return null
  }
}
