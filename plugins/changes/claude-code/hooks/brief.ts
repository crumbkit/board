// What the Agent hook checks in a builder's prompt whose brief line names a
// change in build, and the words of each piece of advice. Pure functions, so the tests
// can pin every word.

// briefOf stays as the artifacts mod copies it (a test there compares the two),
// so the object slicing below is this file's own.
import { briefOf } from './guard'

/** A path with any sentence punctuation after it removed. */
const bare = (path: string) => path.replace(/[.,;:!?]+$/, '')

/** The change the `brief_line` of the JSON object `prompt` starts with names, pretty or compact, or null. */
function leadingBrief(prompt: string): string | null {
  const start = prompt.search(/\S/)
  if (start < 0 || prompt[start] !== '{') return null
  const end = closing(prompt, start)
  if (end < 0) return null
  try {
    const line = (JSON.parse(prompt.slice(start, end + 1)) as { brief_line?: unknown }).brief_line
    // Given one line, briefOf reads only a brief line.
    return typeof line === 'string' ? briefOf(line) : null
  } catch {
    return null
  }
}

/**
 * The change a builder's prompt names, or null: the one the brief it starts
 * with names in its `brief_line`, else the one the first line that is
 * `board-brief: /changes/<name>` names, without punctuation after it. A path
 * named any other way, as a reviewer's prompt names one, is none.
 */
export function briefLineIn(prompt: string): string | null {
  const leading = leadingBrief(prompt)
  if (leading !== null) return bare(leading)
  // Given one line, briefOf reads only a brief line: a lone `{` holds no object.
  for (const line of prompt.split('\n')) {
    const named = briefOf(line)
    if (named !== null) return bare(named)
  }
  return null
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
 * The index of the `}` that closes the `{` at `start` in `text`, skipping
 * braces inside JSON strings, or -1 when none does.
 */
function closing(text: string, start: number): number {
  let depth = 0
  let quoted = false
  for (let at = start; at < text.length; at++) {
    const c = text[at]
    if (quoted) {
      if (c === '\\') at++
      else if (c === '"') quoted = false
    } else if (c === '"') quoted = true
    else if (c === '{') depth++
    else if (c === '}' && --depth === 0) return at
  }
  return -1
}

/**
 * Every JSON object in `text`, in order, in any layout and with any text
 * around it. An object that parses is read whole, so its nested objects are
 * not listed again. One that does not parse is skipped, and the objects
 * inside it are still read.
 */
export function objectsIn(text: string): unknown[] {
  const found: unknown[] = []
  for (let at = text.indexOf('{'); at >= 0; at = text.indexOf('{', at + 1)) {
    const end = closing(text, at)
    if (end < 0) continue
    try {
      found.push(JSON.parse(text.slice(at, end + 1)) as unknown)
      at = end
    } catch {
      // Not JSON: look for an object from the next brace.
    }
  }
  return found
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
      ? 'the prompt holds no JSON object with a `brief_line` field, such as `changes.brief` prints'
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
