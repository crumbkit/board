import { expect, test } from 'claude-code/testing'

import type { ReadyChange } from '../types'
import { failingLabel, failingMode, failingPart, keptLines, KEPT_MS, kindLabel, parseReady, pullRef, readyBadge, readyFor, stateText, transitions, waitingLess, waitsOnFailing } from '../hooks/ready'

// These prove the words the Ready to land button and pane draw from what
// changes.ready answers, and the lines the silent week records. Which
// changes are ready, and why one is failing, the changes plugin's own tests
// prove.

const NOW = Date.UTC(2026, 10, 25, 9, 0, 0)

/** A ready change at `/changes/<name>` with `fields`. */
const change = (name: string, fields: Partial<ReadyChange> = {}): ReadyChange => ({
  path: `/changes/${name}`,
  title: `The ${name} change`,
  pull: `/gh/crumbkit/board/pulls/${name.length}`,
  head: 'a3f9c0de1234567890abcdef1234567890abcdef',
  kind: 'land-choice',
  check: null,
  waits_on: `Land ${name} now?`,
  questions: [`/projects/p/questions/${name}`],
  days: 1,
  failing: null,
  ...fields,
})

/** What changes.ready prints for `changes`. */
const readyReport = (changes: ReadyChange[]) =>
  JSON.stringify({ schema: 'board/call-report/v2', data: { plugin: 'changes', capability: 'ready', output: { changes, total: changes.length, truncated: false, unreadable: 0 } } })

test('changes.ready is read as its rows, and a board without the capability or the plugin is told apart from a failure', () => {
  const rows = [change('t')]
  expect(parseReady(0, readyReport(rows), '')).toEqual({ kind: 'read', ready: { changes: rows, total: 1 } })
  const noCapability = JSON.stringify({ schema: 'board/failure/v1', code: 'board::plugin::no_capability', message: 'no ready', fields: { capability: 'ready', plugin: 'changes' } })
  expect(parseReady(1, noCapability, '')).toEqual({ kind: 'absent' })
  const noPlugin = JSON.stringify({ schema: 'board/failure/v1', code: 'board::plugin::not_installed', message: 'plugin changes is not installed' })
  expect(parseReady(1, noPlugin, '')).toEqual({ kind: 'absent' })
  const other = JSON.stringify({ schema: 'board/failure/v1', code: 'board::plugin::no_capability', message: 'no open', fields: { capability: 'open' } })
  expect(parseReady(1, other, '').kind).toBe('failed')
  const timeout = JSON.stringify({ schema: 'board/failure/v1', code: 'board::query::timeout', message: 'the query ran longer than 1s' })
  expect(parseReady(1, timeout, '')).toEqual({ kind: 'failed', why: 'board::query::timeout the query ran longer than 1s' })
})

test('the button counts the ready changes, draws nothing for none, and counts failing ones in N while the red part is silent', () => {
  const ready = { changes: [change('t'), change('u', { failing: 'conflict' })], total: 2 }
  expect(readyBadge(ready, null, 'silent')).toEqual({ label: 'Ready to land 2', failing: null, problem: false })
  expect(readyBadge(ready, null, 'shown')).toEqual({ label: 'Ready to land 1', failing: '· 1 failing', problem: false })
  expect(readyBadge({ changes: [], total: 0 }, null, 'silent')).toBeNull()
  expect(readyBadge(null, null, 'silent')).toBeNull()
})

test('a failed read keeps the last count with the Waiting wording, and a board with no personal board says nothing', () => {
  const ready = { changes: [change('t'), change('u'), change('v')], total: 3 }
  const why = 'board::query::timeout the query ran longer than 1s'
  expect(readyBadge(ready, why, 'silent')).toEqual({ label: 'Ready to land 3: the last read failed', failing: null, problem: true })
  // With no read that worked, the count is 0, in the same words.
  expect(readyBadge(null, why, 'silent')).toEqual({ label: 'Ready to land 0: the last read failed', failing: null, problem: true })
  expect(readyBadge(null, 'board::config::not_found no personal board', 'silent')).toBeNull()
})

test('the red part says stale when each failing change only has a stale review, and failing otherwise', () => {
  expect(failingPart([])).toBeNull()
  expect(failingPart([change('t', { failing: 'stale' })])).toBe('· 1 stale')
  expect(failingPart([change('t', { failing: 'stale' }), change('u', { failing: 'ci_failed' })])).toBe('· 2 failing')
  const ready = { changes: [change('t'), change('u', { failing: 'stale' })], total: 2 }
  expect(readyBadge(ready, null, 'shown')).toEqual({ label: 'Ready to land 1', failing: '· 1 stale', problem: false })
})

test('a failing row says why after the check it waits on, and that nothing is asked until it is fixed', () => {
  expect(waitsOnFailing('your review', 'stale')).toBe(
    'your review. Its review no longer matches its head commit, and the orchestrator has it reviewed again first. Nothing is asked of you until the review covers the head.',
  )
  expect(waitsOnFailing('your data sign-off.', 'ci_failed')).toBe(
    'your data sign-off. Its CI failed, and the orchestrator is fixing it first. Nothing is asked of you until CI passes.',
  )
  expect(waitsOnFailing('Land it?', 'conflict')).toBe(
    'Land it? Its branch no longer merges cleanly into main, and the orchestrator is fixing it first. Nothing is asked of you until it merges cleanly.',
  )
})

test('Waiting leaves out the questions of each change Ready to land counts, and the blocking ones of a change still going', () => {
  const waiting = { total: 5, blocking: 3 }
  const ready = {
    changes: [
      change('t', { questions: ['/q/a', '/q/b'] }),
      change('u', { questions: ['/q/c'], done: true }),
      change('v', { questions: ['/q/d'], failing: 'conflict' }),
    ],
    total: 3,
  }
  expect(waitingLess(waiting, ready, null, 'silent')).toEqual({ total: 1, blocking: 0 })
  // Shown, the failing change is left out of Ready to land, so Waiting keeps its question.
  expect(waitingLess(waiting, ready, null, 'shown')).toEqual({ total: 2, blocking: 1 })
  expect(waitingLess(waiting, null, null, 'silent')).toBe(waiting)
  expect(waitingLess(null, ready, null, 'silent')).toBeNull()
})

/** plans.open's answer listing the questions at `paths`, of `total` open in all. */
const openOf = (paths: string[], total = paths.length) => ({
  questions: paths.map(path => ({ path, question: path, asked: '2026-11-25T09:00:00Z', options: [], recommendation: null, blocks: null, blocked_work: null })),
  total,
})

test('Waiting leaves out only the ready questions plans.open still lists, so an answered one is not left out twice', () => {
  const ready = { changes: [change('t', { questions: ['/q/a', '/q/b'] })], total: 1 }
  // The owner answered /q/a: plans.waiting no longer counts it, and the last ready read still names it.
  expect(waitingLess({ total: 4, blocking: 3 }, ready, openOf(['/q/b', '/q/x', '/q/y', '/q/z']), 'silent')).toEqual({ total: 3, blocking: 2 })
  // Both answered: nothing is left out.
  expect(waitingLess({ total: 3, blocking: 3 }, ready, openOf(['/q/x', '/q/y', '/q/z']), 'silent')).toEqual({ total: 3, blocking: 3 })
  // When plans.open listed only some of the open questions, an unlisted one may still be open, so it is left out.
  expect(waitingLess({ total: 5, blocking: 3 }, ready, openOf(['/q/b', '/q/x'], 5), 'silent')).toEqual({ total: 3, blocking: 1 })
})

test('only the option shown draws the red part', () => {
  expect(failingMode('shown')).toBe('shown')
  for (const option of ['silent', undefined, '', 'yes']) expect(failingMode(option)).toBe('silent')
})

test('a row names its pull request, how long it has been ready, its kind and why it fails', () => {
  expect(pullRef('/gh/crumbkit/board/pulls/288')).toBe('crumbkit/board#288')
  expect(pullRef('/repos/team/gh/o/r/pulls/7')).toBe('o/r#7')
  expect(pullRef('/elsewhere/7')).toBe('/elsewhere/7')
  expect(pullRef(null)).toBeNull()
  expect([readyFor(0), readyFor(2), readyFor(null)]).toEqual(['ready today', 'ready 2d', null])
  expect(['recheck', 'data-sign-off', 'deviation', 'land-choice'].map(k => kindLabel(k as ReadyChange['kind']))).toEqual(['Recheck', 'Data sign-off', 'Deviation', 'Land choice'])
  expect(['ci_failed', 'conflict', 'stale'].map(f => failingLabel(f as 'stale'))).toEqual(['CI failed', 'Conflicts with main', 'Review is stale'])
})

test('a change that starts failing, changes its reason or stops failing gets one line, and an unchanged one gets none', () => {
  const at = new Date(NOW).toISOString()
  const started = transitions([], [change('t'), change('u', { failing: 'conflict' })], at, 'local_cd34')
  expect(started).toEqual([
    { at, change: '/changes/u', pull: '/gh/crumbkit/board/pulls/1', head: change('u').head, reason: 'conflict', session: 'local_cd34' },
  ])
  expect(transitions(started, [change('t'), change('u', { failing: 'conflict' })], at, 'local_cd34')).toEqual([])
  const changed = transitions(started, [change('u', { failing: 'stale' })], at, 'local_ef56')
  expect(changed.map(l => [l.change, l.reason, l.session])).toEqual([['/changes/u', 'stale', 'local_ef56']])
  expect(transitions(started, [change('u')], at, 's').map(l => [l.change, l.reason])).toEqual([['/changes/u', null]])
  // A change that leaves the list while failing stops failing too.
  expect(transitions(started, [], at, 's').map(l => [l.change, l.reason, l.pull])).toEqual([['/changes/u', null, '/gh/crumbkit/board/pulls/1']])
})

test('the state file keeps lines newer than 30 days, one JSON object per line, and skips a line it cannot read', () => {
  const line = (days: number) => ({ at: new Date(NOW - days * 24 * 3_600_000).toISOString(), change: `/changes/d${days}`, pull: null, head: null, reason: 'stale' as const, session: 's' })
  const text = `${JSON.stringify(line(31))}\nnot json\n${JSON.stringify(line(29))}\n`
  expect(keptLines(text, NOW)).toEqual([line(29)])
  expect(KEPT_MS).toBe(30 * 24 * 3_600_000)
  expect(stateText([line(29)], [line(0)])).toBe(`${JSON.stringify(line(29))}\n${JSON.stringify(line(0))}\n`)
})
