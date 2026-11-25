// What `changes.ready` answered and the words the Ready to land button and
// pane draw from it, and the lines the mod appends to its state file while
// the red failing part is silent. Pure functions, so the tests can pin every
// word.

import type { Failing, Ready, ReadyChange, ReadyKind } from '../types'
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

/**
 * The Ready to land button: its label, the red part that says how many are
 * failing (only when `mode` is `shown`), and whether the last read failed.
 * While the red part is silent, failing changes count in N. Null when
 * nothing is ready and the last read did not fail loudly.
 */
export function readyBadge(ready: Ready | null, why: string | null, mode: FailingMode): { label: string; failing: string | null; problem: boolean } | null {
  const loud = why !== null && !QUIET.some(code => why.startsWith(code))
  if (ready === null) return loud ? { label: 'Ready to land could not be read', failing: null, problem: true } : null
  const failing = mode === 'shown' ? ready.changes.filter(c => c.failing !== null).length : 0
  const count = ready.total - failing
  if (loud) return { label: `Ready to land ${count}: the last read failed`, failing: null, problem: true }
  if (ready.total === 0) return null
  return { label: `Ready to land ${count}`, failing: failing === 0 ? null : `· ${failing} failing`, problem: false }
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
