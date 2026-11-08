// What the controls capabilities answered, and the words a session and the
// owner read from them. Pure functions, so the tests can pin every word.

import type { Checked, Control, Kind, StateBadge } from '../types'

/** The name of the context block the session's first message carries. */
export const BLOCK = 'boardControls'

/** Board's code for a directory that no known board holds. */
const NO_BOARD = 'board::config::not_found'

type Report = Record<string, unknown>

function parseJson(text: string): Report | null {
  try {
    const value: unknown = JSON.parse(text)
    return typeof value === 'object' && value !== null ? (value as Report) : null
  } catch {
    return null
  }
}

function output(stdout: string): Report | null {
  const report = parseJson(stdout)
  if (report?.schema !== 'board/call-report/v2') return null
  const data = report.data as { output?: Report } | undefined
  return data?.output ?? null
}

function failure(exitCode: number, stdout: string, stderr: string): { code: string; message: string } {
  const report = parseJson(stdout) ?? parseJson(stderr)
  if (typeof report?.code === 'string' && typeof report.message === 'string') {
    return { code: report.code, message: report.message }
  }
  const text = (stderr.trim() || stdout.trim()).slice(0, 2000)
  return { code: `exit ${exitCode}`, message: text || 'board printed nothing' }
}

type Stated = { control: string; requirement: string; kind?: string; close?: string }
type Measured = Stated & { budget: string; used: number; max: number; unreadable?: unknown[] }
type Pinned = Stated & { at_least: string; behind?: unknown[]; unreadable?: unknown[] }

/** `, N unreadable` when a measured control could not read N of its files. */
function unread(list: unknown[] | undefined): string {
  return Array.isArray(list) && list.length > 0 ? `, ${list.length} unreadable` : ''
}

/** How a version control's files stand against its floor. */
function pins(v: Pinned): string {
  const behind = Array.isArray(v.behind) ? v.behind.length : 0
  return behind > 0 ? `${behind} older than ${v.at_least}` : `every pin at least ${v.at_least}`
}

/** An obligation's `close` line, as the controls plugin gives it; nothing for a standing control. */
function closing(c: Stated): { close?: string } {
  return typeof c.close === 'string' ? { close: c.close } : {}
}

/** A control's line in a list, with an obligation's close line indented beneath it. */
function line(c: Control): string {
  const head = `- ${c.control}: ${c.requirement}${c.usage ? ` (now ${c.usage})` : ''}`
  return c.close ? `${head}\n  ${c.close}` : head
}

/**
 * Reads `board --json call controls.check --path .` run at `root`. `null`
 * when no board holds the repository, which is no failure: it has no controls.
 */
export function parseCheck(root: string, exitCode: number, stdout: string, stderr: string): Checked | null {
  const found = exitCode === 0 ? output(stdout) : null
  if (found !== null && Array.isArray(found.stated) && Array.isArray(found.measured)) {
    const measured = (found.measured as Measured[]).map(m => ({
      kind: 'budget' as const,
      control: m.control,
      requirement: m.requirement,
      usage: `${m.used} of ${m.max} ${m.budget}${m.used > m.max ? ', over budget' : ''}${unread(m.unreadable)}`,
      ...closing(m),
    }))
    const versions = (Array.isArray(found.versions) ? (found.versions as Pinned[]) : []).map(v => ({
      kind: 'standing' as const,
      control: v.control,
      requirement: v.requirement,
      usage: `${pins(v)}${unread(v.unreadable)}`,
      ...closing(v),
    }))
    const stated = (found.stated as Stated[]).map(s => ({
      kind: (s.kind === 'obligation' ? 'obligation' : 'standing') as Kind,
      control: s.control,
      requirement: s.requirement,
      ...closing(s),
    }))
    return { kind: 'read', root, controls: [...measured, ...versions, ...stated] }
  }
  const failed = failure(exitCode, stdout, stderr)
  if (failed.code === NO_BOARD) return null
  return { kind: 'failed', root, ...failed }
}

/** Reads `board --json call controls.for-path`; `null` when no board holds the file. */
export function parseForPath(exitCode: number, stdout: string): Control[] | null {
  const found = exitCode === 0 ? output(stdout) : null
  if (found === null || !Array.isArray(found.controls)) return null
  return (found.controls as Stated[]).map(c => ({
    kind: (c.kind === 'obligation' ? 'obligation' : 'standing') as Kind,
    control: c.control,
    requirement: c.requirement,
    ...closing(c),
  }))
}

/** The context block's text, or `null` when there is nothing to say. */
export function contextText(checked: Checked): string | null {
  if (checked.kind === 'failed') {
    return (
      `\`board call controls.check\` failed for ${checked.root}, so the controls on its files are unknown:\n\n` +
      `${checked.code}: ${checked.message}\n\n` +
      'Before you edit a file, run `board call controls.for-path --path <file>` in its directory.'
    )
  }
  if (checked.controls.length === 0) return null
  const lines = checked.controls.map(line)
  return (
    `The board's controls state requirements on files in ${checked.root}. Follow each one that applies to a ` +
    `file you change. \`board get <control>\` shows which files a control names.\n\n${lines.join('\n')}`
  )
}

/** The last segment of a path, such as `crumbkit` for `/work/crumbkit`. */
const lastSegment = (path: string) => path.split('/').filter(Boolean).at(-1) ?? path

/** The Controls button's label, and whether it is drawn in red; null when there are no controls. */
export function controlsBadge(checked: Checked): { label: string; problem: boolean } | null {
  if (checked.kind === 'failed') return { label: `Controls: ${checked.code}`, problem: true }
  return checked.controls.length === 0 ? null : { label: `Controls ${checked.controls.length}`, problem: false }
}

/** The pane's heading. */
export const paneTitle = (checked: Checked) => `Controls on ${lastSegment(checked.root)}`

/** The pane's sections, in order, each with the controls of its kind. */
export function sections(controls: readonly Control[]): { heading: string; controls: Control[] }[] {
  const of = (kind: Kind) => controls.filter(c => c.kind === kind)
  return [
    { heading: 'Standing rules', controls: of('standing') },
    { heading: 'Obligations owed', controls: of('obligation') },
    { heading: 'Budgets', controls: of('budget') },
  ]
}

/** What the pane says under its sections. */
export const REMINDER_NOTE =
  'Editing a file adds a reminder for each control on that file that this session was not told about.'

/** A state badge the owner configured: its label and the command whose first line of output is its value. */
export type Configured = { label: string; argv: string[] }

/** Reads the known boards file's `personal`, the personal board's directory; null when it names none. */
export function parsePersonal(text: string): string | null {
  const config = parseJson(text)
  return typeof config?.personal === 'string' ? config.personal : null
}

/**
 * Reads the badges file, `{"badges": [{"label", "argv"}]}`: the configured
 * badges, or what is wrong with the file.
 */
export function parseBadges(text: string): { badges: Configured[] } | { problem: string } {
  const file = parseJson(text)
  if (file === null) return { problem: 'is not a JSON object' }
  if (!Array.isArray(file.badges)) return { problem: 'has no badges list' }
  const badges: Configured[] = []
  for (const [i, entry] of (file.badges as unknown[]).entries()) {
    const { label, argv } = (typeof entry === 'object' && entry !== null ? entry : {}) as { label?: unknown; argv?: unknown }
    const ok = typeof label === 'string' && label.trim() !== '' && Array.isArray(argv) && argv.length > 0 && argv.every(a => typeof a === 'string')
    if (!ok) return { problem: `entry ${i + 1} needs a label and an argv of strings` }
    badges.push({ label: label.trim(), argv: argv as string[] })
  }
  return { badges }
}

/**
 * A badge command's value, the first line of its output, or why it shows no
 * badge: it failed or printed nothing.
 */
export function badgeValue(exitCode: number, stdout: string): { value: string } | { why: string } {
  if (exitCode !== 0) return { why: `exited with status ${exitCode}` }
  const first = (stdout.split('\n')[0] ?? '').trim()
  return first === '' ? { why: 'printed nothing on its first line' } : { value: first }
}

/**
 * The Board badge's value from `board --json ls / --limit 1`: `personal` for
 * the personal board, else its directory's name; null when no board answered.
 */
export function boardName(exitCode: number, stdout: string, personal: string | null): string | null {
  const report = exitCode === 0 ? parseJson(stdout) : null
  const location = (report?.summary as { location?: { repository?: unknown } } | undefined)?.location
  if (typeof location?.repository !== 'string') return null
  const trim = (path: string) => path.replace(/\/+$/, '')
  return personal !== null && trim(personal) === trim(location.repository) ? 'personal' : lastSegment(location.repository)
}

/** The red badge for a badges file that could not be read or is malformed. */
export const badgesProblem = (file: string, problem: string): StateBadge => ({ problem: `${file} ${problem}` })

/** The state badges in the order they show: the configured ones, then Board. */
export const stateBadges = (configured: readonly StateBadge[], board: string | null): StateBadge[] =>
  board === null ? [...configured] : [...configured, { label: 'Board', value: board, problem: null }]

/** The hidden row that names controls on `file` the conversation was not told about. */
export function reminder(file: string, controls: readonly Control[]): string {
  const lines = controls.map(line)
  const these = controls.length === 1 ? 'This board control applies' : 'These board controls apply'
  return `${these} to ${file}, which you are editing now. Follow each one in this edit and any that follows.\n\n${lines.join('\n')}`
}
