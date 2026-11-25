// What `changes.ready` answered and the words the Ready to land button and
// pane draw from it, and the lines the mod appends to its state file while
// the red failing part is silent. Pure functions, so the tests can pin every
// word.

import type { Failing, Open, Ready, ReadyChange, ReadyKind, Waiting } from '../types'
import { failure } from './plans'

/** Whether the red failing part is drawn: `silent` until a later change sets the option to `shown`. */
export type FailingMode = 'silent' | 'shown'

/** The mode the mod's `failing` option names; anything but `shown` is silent. */
export const failingMode = (option: unknown): FailingMode => (option === 'shown' ? 'shown' : 'silent')

/** How long a line stays in the state file. */
export const KEPT_MS = 30 * 24 * 3_600_000

/**
 * Reads `board --json call changes.ready`. `absent` when the board has no
 * changes plugin or the plugin has no `ready` capability, so the mod stops
 * asking; `failed` with Board's words otherwise.
 */
export function parseReady(exitCode: number, stdout: string, stderr: string): { kind: 'read'; ready: Ready } | { kind: 'absent' } | { kind: 'failed'; why: string } {
  let report: Record<string, unknown> | null = null
  try {
    const value: unknown = JSON.parse(stdout)
    report = typeof value === 'object' && value !== null ? (value as Record<string, unknown>) : null
  } catch {
    report = null
  }
  const output = (report?.data as { output?: { changes?: unknown; total?: unknown } } | undefined)?.output
  if (exitCode === 0 && report?.schema === 'board/call-report/v2' && Array.isArray(output?.changes)) {
    const changes = output.changes as ReadyChange[]
    return { kind: 'read', ready: { changes, total: typeof output.total === 'number' ? output.total : changes.length } }
  }
  const fields = report?.fields as { capability?: unknown; plugin?: unknown } | undefined
  const noCapability = report?.code === 'board::plugin::no_capability' && fields?.capability === 'ready'
  if (noCapability || report?.code === 'board::plugin::not_installed') return { kind: 'absent' }
  return { kind: 'failed', why: failure(exitCode, stdout, stderr) }
}

/** Failure codes the button does not report: no personal board. */
const QUIET = ['board::config::not_found']

/** The changes the red part counts: none while it is silent, else the failing ones. */
const failingOf = (ready: Ready, mode: FailingMode) => (mode === 'shown' ? ready.changes.filter(c => c.failing !== null) : [])

/**
 * The red part for `failing` changes: `· N stale` when every one is failing
 * only because its review no longer matches its head, else `· N failing`.
 * Null when none is.
 */
export function failingPart(failing: readonly ReadyChange[]): string | null {
  if (failing.length === 0) return null
  const word = failing.every(c => c.failing === 'stale') ? 'stale' : 'failing'
  return `· ${failing.length} ${word}`
}

/**
 * The Ready to land button: its label, the red part that says how many are
 * failing (only when `mode` is `shown`), and whether the last read failed.
 * While the red part is silent, failing changes count in N. A failed read
 * keeps the last count, 0 when none was read. Null when nothing is ready and
 * the last read did not fail loudly.
 */
export function readyBadge(ready: Ready | null, why: string | null, mode: FailingMode): { label: string; failing: string | null; problem: boolean } | null {
  const loud = why !== null && !QUIET.some(code => why.startsWith(code))
  const failing = ready === null ? [] : failingOf(ready, mode)
  const count = (ready?.total ?? 0) - failing.length
  if (loud) return { label: `Ready to land ${count}: the last read failed`, failing: null, problem: true }
  if (ready === null || ready.total === 0) return null
  return { label: `Ready to land ${count}`, failing: failingPart(failing), problem: false }
}

/**
 * What Waiting counts once the questions Ready to land counts are left out,
 * so a check is counted once: each question of a change the button counts
 * leaves `total`, and also `blocking` when its change is still going. The
 * ready read can be older than the count, so a question is left out only
 * while `open`, the last plans.open answer, still lists it, or lists only
 * some of the open questions; an answered question is not left out twice.
 */
export function waitingLess(waiting: Waiting | null, ready: Ready | null, open: Open | null, mode: FailingMode): Waiting | null {
  if (waiting === null || ready === null) return waiting
  const listed = open === null || open.questions.length < open.total ? null : new Set(open.questions.map(q => q.path))
  const stillOpen = (path: string) => listed === null || listed.has(path)
  const counted = ready.changes.filter(c => mode === 'silent' || c.failing === null)
  const all = new Set(counted.flatMap(c => c.questions).filter(stillOpen))
  const going = new Set(counted.filter(c => !c.done).flatMap(c => c.questions).filter(stillOpen))
  return { total: Math.max(0, waiting.total - all.size), blocking: Math.max(0, waiting.blocking - going.size) }
}

const KINDS: Record<ReadyKind, string> = {
  recheck: 'Recheck',
  'data-sign-off': 'Data sign-off',
  deviation: 'Deviation',
  'land-choice': 'Land choice',
}

/** The tag a row's kind draws. */
export const kindLabel = (kind: ReadyKind) => KINDS[kind] ?? kind

const FAILINGS: Record<Failing, string> = { ci_failed: 'CI failed', conflict: 'Conflicts with main', stale: 'Review is stale' }

/** The red tag a failing row draws. */
export const failingLabel = (failing: Failing) => FAILINGS[failing] ?? failing

const WHY: Record<Failing, string> = {
  ci_failed: 'Its CI failed, and the orchestrator is fixing it first. Nothing is asked of you until CI passes.',
  conflict: 'Its branch no longer merges cleanly into main, and the orchestrator is fixing it first. Nothing is asked of you until it merges cleanly.',
  stale: 'Its review no longer matches its head commit, and the orchestrator has it reviewed again first. Nothing is asked of you until the review covers the head.',
}

/** What a failing row's `Waits on:` says after the check: why it fails and that nothing is asked until it is fixed. */
export const failingWhy = (failing: Failing) => WHY[failing] ?? ''

/** A failing row's `Waits on:` text: the check, then why it fails. */
export function waitsOnFailing(waitsOn: string, failing: Failing): string {
  const said = waitsOn.trim().replace(/[;:,]+$/, '')
  return `${said}${/[.?!]$/.test(said) ? '' : '.'} ${failingWhy(failing)}`
}

/** A pull request record's path as `owner/repo#number`; the path itself when it is not a GitHub pull record. */
export function pullRef(path: string | null): string | null {
  if (path === null) return null
  const found = /\/gh\/([^/]+)\/([^/]+)\/pulls\/(\d+)$/.exec(path)
  return found === null ? path : `${found[1]}/${found[2]}#${found[3]}`
}

/** How long a row has been ready, from the days since its change was touched. */
export const readyFor = (days: number | null) => (days === null ? null : days === 0 ? 'ready today' : `ready ${days}d`)

/** One line of the state file. */
export type FailingLine = { at: string; change: string; pull: string | null; head: string | null; reason: Failing | null; session: string }

/** The lines of `text` that parse and are newer than `now - KEPT_MS`. */
export function keptLines(text: string, now: number): FailingLine[] {
  return text
    .split('\n')
    .filter(line => line.trim() !== '')
    .flatMap(line => {
      try {
        const parsed = JSON.parse(line) as FailingLine
        return typeof parsed.change === 'string' && now - Date.parse(parsed.at) < KEPT_MS ? [parsed] : []
      } catch {
        return []
      }
    })
}

/**
 * The lines to append for `changes`, given the lines already kept: one for
 * each change that starts failing or changes its reason, and one with a null
 * reason for each change whose last line names a reason and that is no
 * longer failing or no longer listed.
 */
export function transitions(kept: readonly FailingLine[], changes: readonly ReadyChange[], at: string, session: string): FailingLine[] {
  const last = new Map<string, FailingLine>()
  for (const line of kept) last.set(line.change, line)
  const added: FailingLine[] = []
  const listed = new Set<string>()
  for (const change of changes) {
    listed.add(change.path)
    const before = last.get(change.path)?.reason ?? null
    if (change.failing !== before) added.push({ at, change: change.path, pull: change.pull, head: change.head, reason: change.failing, session })
  }
  for (const [change, line] of last) {
    if (!listed.has(change) && line.reason !== null) added.push({ at, change, pull: line.pull, head: line.head, reason: null, session })
  }
  return added
}

/** The state file's text after `added`: the kept lines, then the new ones, one JSON object per line. */
export const stateText = (kept: readonly FailingLine[], added: readonly FailingLine[]) =>
  [...kept, ...added].map(line => JSON.stringify(line)).join('\n') + '\n'
