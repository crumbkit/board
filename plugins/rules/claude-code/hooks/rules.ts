// What the rules capabilities answered, and the words a session and the
// owner read from them. Pure functions, so the tests can pin every word.

import type { Checked, Kind, Rule, StateBadge, Warning } from '../types'

/**
 * The speed check, which session start runs detached: the shell returns at
 * once, so the start never waits on it, and the check records at most one
 * run an hour (docs/architecture/performance/timing.md, The speed check).
 * It runs in a session or process group of its own, so a signal to the
 * hook's group does not reach it: through `setsid -f` where the system has
 * it, as Linux does, and else through the shell's job control, as macOS's
 * sh gives. nohup has it ignore SIGHUP, which `board timing` leaves ignored.
 * A `session` the line sets first is passed as `--session`, so a question
 * the check files names the session that asked.
 */
export const SPEED_CHECK =
  'run="nohup board timing check --record${session:+ --session $session}"; if command -v setsid >/dev/null 2>&1; then setsid -f $run </dev/null >/dev/null 2>&1; else set -m; $run </dev/null >/dev/null 2>&1 & fi'

/** A Claude Code session's `local_` id, the only form a question names and the only one put in a shell line. */
const LOCAL_SESSION = /^local_[0-9a-f-]+$/

/** The speed check's line, naming `host` as the session when it is a `local_` id. */
export function speedLine(path: string, host: string | undefined): string {
  const session = host !== undefined && LOCAL_SESSION.test(host) ? `session=${host}; ` : ''
  return `${session}PATH=${path} ${SPEED_CHECK}`
}

/** The name of the context block the session's first message carries. */
export const BLOCK = 'boardRules'

/** Board's code for a directory that no known board holds. */
const NO_BOARD = 'board::config::not_found'

/** The code of a check whose answer this mod cannot read. */
const UNREADABLE = 'board-rules::output::unreadable'

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

type Stated = { rule: string; requirement: string; kind?: string; state?: string; close?: string }
type Measured = Stated & { budget: string; used: number; max: number; unreadable?: unknown[] }
type Pinned = Stated & { at_least: string; behind?: unknown[]; unreadable?: unknown[] }
type Zeroed = Stated & {
  suite: string
  run: string | null
  age_s: number | null
  measures: { name: string; value: unknown }[]
  state: string
  code?: string
}

/** `, N unreadable` when a measured rule could not read N of its files. */
function unread(list: unknown[] | undefined): string {
  return Array.isArray(list) && list.length > 0 ? `, ${list.length} unreadable` : ''
}

/** How a version rule's files stand against its floor. */
function pins(v: Pinned): string {
  const behind = Array.isArray(v.behind) ? v.behind.length : 0
  return behind > 0 ? `${behind} older than ${v.at_least}` : `every pin at least ${v.at_least}`
}

/** A run's age in whole minutes, hours or days. */
function age(seconds: number): string {
  if (seconds < 3600) return `${Math.floor(seconds / 60)} min old`
  if (seconds < 172800) return `${Math.floor(seconds / 3600)} h old`
  return `${Math.floor(seconds / 86400)} d old`
}

/** What a zero rule's suite reports, such as `remaining 3, unknown 0 in /suites/s/runs/1, 20 h old`. */
function zeroes(z: Zeroed): string {
  if (z.state === 'unreadable') return `${z.suite} unreadable: ${z.code ?? 'no code'}`
  if (z.run === null) return `no qualifying run of ${z.suite}`
  const values = (Array.isArray(z.measures) ? z.measures : []).map(m => `${m.name} ${m.value === null || m.value === undefined ? 'missing' : String(m.value)}`)
  const stale = z.state === 'stale' ? ', stale' : ''
  return `${values.join(', ')} in ${z.run}${z.age_s === null ? '' : `, ${age(z.age_s)}`}${stale}`
}

/** An obligation's `close` line, as the rules plugin gives it; nothing for a standing rule. */
function closing(c: Stated): { close?: string } {
  return typeof c.close === 'string' ? { close: c.close } : {}
}

/** A rule's line in a list, with an obligation's close line indented beneath it. */
function line(c: Rule): string {
  const head = `- ${c.rule}: ${c.requirement}${c.usage ? ` (now ${c.usage})` : ''}${c.described ? ' (described)' : ''}`
  return c.close ? `${head}\n  ${c.close}` : head
}

/**
 * Reads `board --json call rules.check --path .` run at `root`. `null`
 * when no board holds the repository, which is no failure: it has no rules.
 * An answer in the former names (`control` for `rule`) is a failure.
 */
export function parseCheck(root: string, exitCode: number, stdout: string, stderr: string): Checked | null {
  const found = exitCode === 0 ? output(stdout) : null
  if (found !== null && Array.isArray(found.stated) && Array.isArray(found.measured)) {
    const versionList = Array.isArray(found.versions) ? (found.versions as Pinned[]) : []
    const zeroList = Array.isArray(found.zeros) ? (found.zeros as Zeroed[]) : []
    const all = [...(found.measured as Measured[]), ...versionList, ...zeroList, ...(found.stated as Stated[])]
    if (!all.every(item => typeof item.rule === 'string')) {
      return { kind: 'failed', root, code: UNREADABLE, message: 'rules.check named a rule in a way this mod does not read; update the mod and Board together' }
    }
    const measured = (found.measured as Measured[]).map(m => ({
      kind: 'budget' as const,
      rule: m.rule,
      requirement: m.requirement,
      usage: `${m.used} of ${m.max} ${m.budget}${m.used > m.max ? ', over budget' : ''}${unread(m.unreadable)}`,
      ...closing(m),
    }))
    const versions = versionList.map(v => ({
      kind: 'standing' as const,
      rule: v.rule,
      requirement: v.requirement,
      usage: `${pins(v)}${unread(v.unreadable)}`,
      ...closing(v),
    }))
    const zeros = zeroList.map(z => ({
      kind: (z.kind === 'obligation' ? 'obligation' : 'standing') as Kind,
      rule: z.rule,
      requirement: z.requirement,
      usage: zeroes(z),
      ...closing(z),
    }))
    const stated = (found.stated as Stated[]).map(s => ({
      kind: (s.kind === 'obligation' ? 'obligation' : 'standing') as Kind,
      rule: s.rule,
      requirement: s.requirement,
      ...(s.state === 'described' ? { described: true } : {}),
      ...closing(s),
    }))
    const warnings = (Array.isArray(found.warnings) ? (found.warnings as Warning[]) : []).filter(
      w => typeof w?.code === 'string' && typeof w?.detail === 'string',
    )
    const rules = [...measured, ...versions, ...zeros, ...stated]
    return { kind: 'read', root, rules, ...(warnings.length > 0 ? { warnings } : {}) }
  }
  const failed = failure(exitCode, stdout, stderr)
  if (failed.code === NO_BOARD) return null
  return { kind: 'failed', root, ...failed }
}

/** Reads `board --json call rules.for-path`; `null` when no board holds the file or it answers in the former names. */
export function parseForPath(exitCode: number, stdout: string): Rule[] | null {
  const found = exitCode === 0 ? output(stdout) : null
  if (found === null || !Array.isArray(found.rules)) return null
  const listed = found.rules as Stated[]
  if (!listed.every(c => typeof c.rule === 'string')) return null
  return listed.map(c => ({
    kind: (c.kind === 'obligation' ? 'obligation' : 'standing') as Kind,
    rule: c.rule,
    requirement: c.requirement,
    ...closing(c),
  }))
}

/** A remembered check's time, such as `2026-11-25 01:56 UTC`. */
export const checkedAt = (at: number) => `${new Date(at).toISOString().slice(0, 16).replace('T', ' ')} UTC`

/** Each warning `rules.check` gave, one line each after a blank line, or nothing. */
function warnings(checked: { warnings?: Warning[] }): string {
  const warned = (checked.warnings ?? []).map(w => `- ${w.code}: ${w.detail}`)
  return warned.length === 0 ? '' : `\n\nBoard warns about these rules:\n${warned.join('\n')}`
}

/** The context block's text, or `null` when there is nothing to say. */
export function contextText(checked: Checked): string | null {
  if (checked.kind === 'pending') {
    return (
      `\`board call rules.check\` is still running for ${checked.root}. The rules on its files will follow in a message when it answers. ` +
      'Until then, run `board call rules.for-path --path <file>` in its directory before you edit a file.'
    )
  }
  if (checked.kind === 'failed') {
    return (
      `\`board call rules.check\` failed for ${checked.root}, so the rules on its files are unknown:\n\n` +
      `${checked.code}: ${checked.message}\n\n` +
      'Before you edit a file, run `board call rules.for-path --path <file>` in its directory.'
    )
  }
  const warns = warnings(checked)
  if (checked.rules.length === 0 && warns === '') return null
  const lines = checked.rules.map(line)
  const from =
    checked.at === undefined
      ? ''
      : checked.stale !== undefined
        ? ` ${staleText(checked.root, checked.at, checked.stale)}`
        : ` These are from the check at ${checkedAt(checked.at)}. A fresh check runs now, and this session is told if it finds other rules.`
  return (
    `The board's rules state requirements on files in ${checked.root}. Follow each one that applies to a ` +
    `file you change. \`board get <rule>\` shows which files a rule names.${from}\n\n${lines.join('\n')}${warns}`
  )
}

/** What the session is told when the fresh check after a remembered start fails. */
export function staleText(root: string, at: number, failed: { code: string; message: string }): string {
  return (
    `The fresh rules check for ${root} failed, so the rules from the check at ${checkedAt(at)} may be out of date: ` +
    `${failed.code}: ${failed.message}. Before you edit a file, run \`board call rules.for-path --path <file>\` in its directory.`
  )
}

/** Whether two answers tell a session the same rules, whenever each was checked. */
export function sameRules(a: Checked | null, b: Checked | null): boolean {
  const text = (c: Checked | null) => (c === null ? null : contextText(c.kind === 'read' ? { kind: 'read', root: c.root, rules: c.rules, warnings: c.warnings } : c))
  return text(a) === text(b)
}

/** The hidden row that gives a check's answer once it arrives after the session's first message. */
export function freshText(checked: Checked | null, root: string): string {
  if (checked === null) return `The rules check for ${root} has answered: no board holds it, so it has no rules.`
  if (checked.kind === 'pending') return contextText(checked) ?? ''
  if (checked.kind === 'failed') {
    return (
      `The rules check for ${root} failed after this session started, so the rules on its files are unknown:\n\n` +
      `${checked.code}: ${checked.message}\n\n` +
      'Before you edit a file, run `board call rules.for-path --path <file>` in its directory.'
    )
  }
  if (checked.rules.length === 0) {
    return `The rules check for ${root} has answered since this session started: no rules apply to its files.${warnings(checked)}`
  }
  return (
    `The rules check for ${root} has answered since this session started, and these rules replace any listed before. ` +
    `Follow each one that applies to a file you change. \`board get <rule>\` shows which files a rule names.\n\n` +
    checked.rules.map(line).join('\n') +
    warnings(checked)
  )
}

/** The `$.store` key under which the mod remembers each root's last good check. */
export const REMEMBERED = 'answers'

/** How many repository roots the mod remembers; the least recently checked go first. */
export const REMEMBER_ROOTS = 16

/** A root's last good check: when it ran, the rules it named and the Board badge's name. */
export type Remembered = { at: number; rules: Rule[]; board: string | null }

const KINDS: readonly string[] = ['standing', 'obligation', 'budget']

function isRule(value: unknown): value is Rule {
  const r = (typeof value === 'object' && value !== null ? value : {}) as Record<string, unknown>
  const optional = (k: string, t: string) => r[k] === undefined || typeof r[k] === t
  return (
    typeof r.rule === 'string' &&
    typeof r.requirement === 'string' &&
    KINDS.includes(String(r.kind)) &&
    optional('usage', 'string') &&
    optional('close', 'string') &&
    optional('described', 'boolean')
  )
}

function entries(stored: unknown): Record<string, unknown> {
  return typeof stored === 'object' && stored !== null && !Array.isArray(stored) ? (stored as Record<string, unknown>) : {}
}

/** The root's remembered check from the store's value, or null when there is none or it has another shape. */
export function recalled(stored: unknown, root: string): Remembered | null {
  const entry = entries(stored)[root] as Record<string, unknown> | undefined
  if (typeof entry !== 'object' || entry === null) return null
  const { at, rules, board } = entry
  if (typeof at !== 'number' || !Array.isArray(rules) || !rules.every(isRule)) return null
  if (board !== null && typeof board !== 'string') return null
  return { at, rules, board }
}

/** The store's value with the root's entry set, or removed for null, keeping the REMEMBER_ROOTS newest. */
export function remembering(stored: unknown, root: string, entry: Remembered | null): Record<string, Remembered> {
  const kept: [string, Remembered][] = []
  for (const key of Object.keys(entries(stored))) {
    const other = key === root ? null : recalled(stored, key)
    if (other !== null) kept.push([key, other])
  }
  if (entry !== null) kept.push([root, entry])
  kept.sort((a, b) => b[1].at - a[1].at)
  return Object.fromEntries(kept.slice(0, REMEMBER_ROOTS))
}

/** The last segment of a path, such as `crumbkit` for `/work/crumbkit`. */
const lastSegment = (path: string) => path.split('/').filter(Boolean).at(-1) ?? path

/** The Rules button's label, and whether it is drawn in red; null when there are no rules. */
export function rulesBadge(checked: Checked): { label: string; problem: boolean } | null {
  if (checked.kind === 'pending') return { label: 'Rules …', problem: false }
  if (checked.kind === 'failed') return { label: `Rules: ${checked.code}`, problem: true }
  return checked.rules.length === 0 ? null : { label: `Rules ${checked.rules.length}`, problem: false }
}

/** The pane's heading. */
export const paneTitle = (checked: Checked) => `Rules on ${lastSegment(checked.root)}`

/** The pane's sections, in order, each with the rules of its kind. */
export function sections(rules: readonly Rule[]): { heading: string; rules: Rule[] }[] {
  const of = (kind: Kind) => rules.filter(c => c.kind === kind)
  return [
    { heading: 'Standing rules', rules: of('standing') },
    { heading: 'Obligations owed', rules: of('obligation') },
    { heading: 'Budgets', rules: of('budget') },
  ]
}

/** What the pane says under its sections. */
export const REMINDER_NOTE =
  'Editing a file adds a reminder for each rule on that file that this session was not told about.'

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

/** The hidden row that names rules on `file` the conversation was not told about. */
export function reminder(file: string, rules: readonly Rule[]): string {
  const lines = rules.map(line)
  const these = rules.length === 1 ? 'This board rule applies' : 'These board rules apply'
  return `${these} to ${file}, which you are editing now. Follow each one in this edit and any that follows.\n\n${lines.join('\n')}`
}
