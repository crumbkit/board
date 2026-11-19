import { expect, mock, test } from 'claude-code/testing'
import type { On } from 'claude-code'

import { crossing, resumeLine } from '../hooks/register'

// These prove when a session is told to hand off, what the other sessions on
// the account are told, and which resume lines are shown. `board` is stood in
// by a process.run hook that answers `params.get`, and the disk by a map, so
// the tests prove the mod's logic and its files' contents, not that Claude
// Code runs `session.measure` at those moments or that the model obeys the
// row; /evals/session-usage-for-handoff says what was checked in a real session.

const NOW = Date.UTC(2026, 10, 19, 9, 0, 0)
const STATE = '/home/user/.claude/board-handoff'
const RESET = '2026-11-19T12:00:00Z'
const whole = { stderr: '', isStdoutTruncated: false, isStderrTruncated: false }

type Shares = { context?: number; usage?: number } | 'refused' | 'throws'

/** Stands in for `board`, the disk under `STATE` and the session's prompt submission. */
function host(on: On, who: string | { id: string }, shares: Shares = { context: 70, usage: 85 }, disk = new Map<string, string>()) {
  const me = typeof who === 'string' ? { id: who } : who
  const submitted: string[] = []
  const logs: string[] = []
  on('process.run', (_, e) => {
    if (shares === 'throws') throw new Error('board missing')
    const name = e.argv[e.argv.indexOf('--name') + 1]
    const value = shares === 'refused' ? undefined : name === 'handoff-context-share' ? shares.context : shares.usage
    if (value === undefined) return { value: { exitCode: 1, stdout: '{"code":"params::heuristic::not_found"}', ...whole } }
    return { value: { exitCode: 0, stdout: JSON.stringify({ data: { output: { value, source: 'value' } } }), ...whole } }
  })
  on('fs.write', (_, e) => (disk.set(e.path, e.text), { value: undefined }))
  on('fs.read', (_, e) => {
    const text = disk.get(e.path)
    if (text === undefined) throw new Error('missing')
    return { value: text }
  })
  on('fs.list', (_, e) => ({
    value: [...disk.keys()].filter(p => p.startsWith(`${e.path}/`)).map(p => ({ name: p.slice(e.path.length + 1), kind: 'file' as const, size: 0, mtimeMs: 0, isLink: false })),
  }))
  on('prompt.submit', (_, e) => (submitted.push(e.text), { text: e.text }))
  on('ui.log', (_, e) => (logs.push(e.text), { value: undefined }))
  on('command.register', () => ({ value: { command: 'handoff-lines' } }))
  on('session.id', () => ({ value: me.id }))
  mock.env(on, { HOME: '/home/user' })
  return { disk, submitted, logs }
}

const usage = (percent: number, five = 10, seven = 10) => ({
  context: { window: 200_000, percent },
  rateLimits: [
    { kind: 'five_hour', percentUsed: five, resetsAt: RESET },
    { kind: 'seven_day', percentUsed: seven, resetsAt: '2026-11-24T00:00:00Z' },
  ],
  changed: ['context' as const],
})

async function start($: any, cwd = '/work/a') {
  await $.session.start({ cwd, surface: 'terminal', isInteractive: true })
}

test('crossing names the context share first, then the first usage window at or over its share', () => {
  const shares = { context: 70, usage: 85 }
  expect(crossing(usage(69), shares)).toBeUndefined()
  expect(crossing(usage(70), shares)).toEqual({ key: 'context', why: 'its context window is 70% full' })
  expect(crossing(usage(10, 85), shares)).toEqual({ key: `five_hour@${RESET}`, why: 'the five_hour usage limit is 85% used', window: { kind: 'five_hour', resetsAt: RESET } })
  expect(crossing(usage(10, 84.9, 90), shares)?.key).toBe('seven_day@2026-11-24T00:00:00Z')
})

test('a context window at its share makes the session hand off now, and tells no other session', async ($, on) => {
  mock.clock(on, { now: NOW })
  const { disk, submitted } = host(on, 's1')
  on('session.start', (_, e) => ({ cwd: e.cwd }))
  on('session.measure', (_, e) => ({ changed: e.changed }))
  await start($)

  await $.session.measure(usage(70))

  expect(submitted).toHaveLength(1)
  expect(submitted[0]).toMatch(/its context window is 70% full/)
  expect(submitted[0]).toMatch(/handoff skill/)
  expect(disk.has(`${STATE}/trigger.json`)).toBe(false)
  expect(JSON.parse(disk.get(`${STATE}/sessions/s1.json`)!)).toMatchObject({ cwd: '/work/a', asked: ['context'], status: 'asked' })
})

test('a session below both shares is left alone', async ($, on) => {
  mock.clock(on, { now: NOW })
  const { submitted, disk } = host(on, 's1')
  on('session.start', (_, e) => ({ cwd: e.cwd }))
  on('session.measure', (_, e) => ({ changed: e.changed }))
  await start($)
  await $.session.measure(usage(69, 84.9, 5))
  expect(submitted).toEqual([])
  expect(disk.size).toBe(0)
})

test('a usage window at its share makes the session hand off once and leaves a trigger for the other sessions', async ($, on) => {
  mock.clock(on, { now: NOW })
  const { disk, submitted } = host(on, 's1')
  on('session.start', (_, e) => ({ cwd: e.cwd }))
  on('session.measure', (_, e) => ({ changed: e.changed }))
  await start($)

  await $.session.measure(usage(10, 86))
  await $.session.measure(usage(10, 87))

  expect(submitted).toHaveLength(1)
  expect(JSON.parse(disk.get(`${STATE}/trigger.json`)!)).toEqual({ key: `five_hour@${RESET}`, why: 'the five_hour usage limit is 86% used', resetsAt: RESET, at: '2026-11-19T09:00:00Z' })
})

test('every other running session on the account is told within one poll of the trigger', async ($, on) => {
  const clock = mock.clock(on, { now: NOW })
  const disk = new Map([[`${STATE}/trigger.json`, JSON.stringify({ key: `five_hour@${RESET}`, why: 'the five_hour usage limit is 86% used', resetsAt: RESET, at: '2026-11-19T09:00:00Z' })]])
  const { submitted } = host(on, 's2', undefined, disk)
  on('session.start', (_, e) => ({ cwd: e.cwd }))
  await start($, '/work/b')

  await clock.advance(30_000)

  expect(submitted).toHaveLength(1)
  expect(submitted[0]).toMatch(/five_hour usage limit is 86% used/)
  await clock.advance(30_000)
  expect(submitted).toHaveLength(1)
  expect(JSON.parse(disk.get(`${STATE}/sessions/s2.json`)!)).toMatchObject({ cwd: '/work/b', asked: [`five_hour@${RESET}`] })
})

test('a trigger whose window has reset tells nobody', async ($, on) => {
  const clock = mock.clock(on, { now: Date.UTC(2026, 10, 19, 12, 0, 1) })
  const disk = new Map([[`${STATE}/trigger.json`, JSON.stringify({ key: `five_hour@${RESET}`, why: 'x', resetsAt: RESET, at: '2026-11-19T09:00:00Z' })]])
  const { submitted } = host(on, 's2', undefined, disk)
  on('session.start', (_, e) => ({ cwd: e.cwd }))
  await start($)
  await clock.advance(30_000)
  expect(submitted).toEqual([])
})

test('when the asked turn ends the session shows one resume line, and /handoff-lines prints every session\'s', async ($, on) => {
  mock.clock(on, { now: NOW })
  const disk = new Map([[`${STATE}/sessions/s0.json`, JSON.stringify({ cwd: '/work/z', asked: ['context'], status: 'handed-off', line: resumeLine('s0', '/work/z', 'handoff: /changes/z — resume: board changes next --path /changes/z') })]])
  const { logs } = host(on, 's1', undefined, disk)
  on('session.start', (_, e) => ({ cwd: e.cwd }))
  on('session.measure', (_, e) => ({ changed: e.changed }))
  on('turn.complete', (_, e) => ({ text: e.answer }))
  on('command.run', () => ({}))
  await start($)
  await $.session.measure(usage(80))

  await $.turn.complete({ answer: 'Pushed.\nhandoff: /changes/a — resume: board changes next --path /changes/a\nREADY TO ARCHIVE', durationMs: 1, isAborted: false, turnId: 't' } as never)

  const line = 'cd /work/a && claude --resume s1  # handoff: /changes/a — resume: board changes next --path /changes/a'
  expect(logs).toEqual([`handoff: ${line}`])
  expect(JSON.parse(disk.get(`${STATE}/sessions/s1.json`)!)).toMatchObject({ status: 'handed-off', line })

  const out = await $.command.run({ command: 'handoff-lines', args: '', origin: { kind: 'composer' } } as never)
  expect(out.text).toBe(`cd /work/z && claude --resume s0  # handoff: /changes/z — resume: board changes next --path /changes/z\n${line}`)
})

test('thresholds come from params, and the built-in defaults apply when the board gives none', async ($, on) => {
  mock.clock(on, { now: NOW })
  const { submitted, logs } = host(on, 's1', 'refused')
  on('session.start', (_, e) => ({ cwd: e.cwd }))
  on('session.measure', (_, e) => ({ changed: e.changed }))
  await start($)
  await $.session.measure(usage(74))
  expect(submitted).toEqual([])
  expect(logs.join('\n')).toMatch(/handoff: params unreadable, using 75% context and 85% usage/)
  await $.session.measure(usage(75))
  expect(submitted).toHaveLength(1)
})

test('a session that reloads does not get the prompt again for a crossing it was already asked', async ($, on) => {
  mock.clock(on, { now: NOW })
  const disk = new Map([[`${STATE}/sessions/s1.json`, JSON.stringify({ cwd: '/work/a', asked: ['context'], status: 'asked' })]])
  const { submitted } = host(on, 's1', undefined, disk)
  on('session.start', (_, e) => ({ cwd: e.cwd }))
  on('session.measure', (_, e) => ({ changed: e.changed }))
  await start($)
  await $.session.measure(usage(90))
  expect(submitted).toEqual([])
})

test('a usage window that crosses after the context share did still leaves its trigger, and the session is not asked twice', async ($, on) => {
  mock.clock(on, { now: NOW })
  const { disk, submitted } = host(on, 's1')
  on('session.start', (_, e) => ({ cwd: e.cwd }))
  on('session.measure', (_, e) => ({ changed: e.changed }))
  await start($)
  await $.session.measure(usage(80))
  expect(disk.has(`${STATE}/trigger.json`)).toBe(false)

  await $.session.measure(usage(80, 90))

  expect(submitted).toHaveLength(1)
  expect(JSON.parse(disk.get(`${STATE}/trigger.json`)!).key).toBe(`five_hour@${RESET}`)
})

test('a turn that ends with no handoff line, or was aborted, is not recorded as handed off', async ($, on) => {
  mock.clock(on, { now: NOW })
  const { logs, disk } = host(on, 's1')
  on('session.start', (_, e) => ({ cwd: e.cwd }))
  on('session.measure', (_, e) => ({ changed: e.changed }))
  on('turn.complete', (_, e) => ({ text: e.answer }))
  await start($)
  await $.session.measure(usage(80))

  await $.turn.complete({ answer: 'Still working on the parser.', durationMs: 1, isAborted: false, turnId: 't1' } as never)
  await $.turn.complete({ answer: 'handoff: /x — resume: y', durationMs: 1, isAborted: true, turnId: 't2' } as never)

  expect(logs).toEqual([])
  expect(JSON.parse(disk.get(`${STATE}/sessions/s1.json`)!).status).toBe('asked')
})

test('a trigger with no reset time stops telling sessions five hours after it was written', async ($, on) => {
  const clock = mock.clock(on, { now: NOW + 5 * 3_600_000 + 1 })
  const disk = new Map([[`${STATE}/trigger.json`, JSON.stringify({ key: 'five_hour@undefined', why: 'x', at: '2026-11-19T09:00:00Z' })]])
  const { submitted } = host(on, 's2', undefined, disk)
  on('session.start', (_, e) => ({ cwd: e.cwd }))
  await start($)
  await clock.advance(30_000)
  expect(submitted).toEqual([])
})

test('a failing params call leaves the defaults and does not stop the measure', async ($, on) => {
  mock.clock(on, { now: NOW })
  const { submitted } = host(on, 's1', 'throws')
  on('session.start', (_, e) => ({ cwd: e.cwd }))
  on('session.measure', (_, e) => ({ changed: e.changed }))
  await start($)
  await $.session.measure(usage(80))
  expect(submitted).toHaveLength(1)
})

test('a directory with a space is quoted in the resume line', () => {
  expect(resumeLine('s1', "/work/it's here")).toBe("cd '/work/it'\\''s here' && claude --resume s1")
  expect(resumeLine('s1', '/work/a')).toBe('cd /work/a && claude --resume s1')
})

test('a session is told again for a later window after it handed off, and its new line replaces the old one', async ($, on) => {
  mock.clock(on, { now: NOW })
  const { disk, submitted } = host(on, 's1')
  on('session.start', (_, e) => ({ cwd: e.cwd }))
  on('session.measure', (_, e) => ({ changed: e.changed }))
  on('turn.complete', (_, e) => ({ text: e.answer }))
  await start($)
  await $.session.measure(usage(80))
  await $.turn.complete({ answer: 'handoff: /a', durationMs: 1, isAborted: false, turnId: 't' } as never)
  await $.session.measure(usage(80, 90))
  expect(submitted).toHaveLength(2)
  await $.turn.complete({ answer: 'handoff: /b', durationMs: 1, isAborted: false, turnId: 't' } as never)
  expect(JSON.parse(disk.get(`${STATE}/sessions/s1.json`)!).line).toContain('# handoff: /b')
})

test('after /clear the new session id starts with no state of the old one', async ($, on) => {
  mock.clock(on, { now: NOW })
  const who = { id: 's1' }
  const { disk, submitted } = host(on, who)
  on('session.start', (_, e) => ({ cwd: e.cwd }))
  on('session.measure', (_, e) => ({ changed: e.changed }))
  await start($)
  await $.session.measure(usage(80))
  who.id = 's2'
  await $.session.measure(usage(80))
  expect(submitted).toHaveLength(2)
  expect(disk.has(`${STATE}/sessions/s2.json`)).toBe(true)
})

test('a trigger whose reset time cannot be read tells nobody', async ($, on) => {
  const clock = mock.clock(on, { now: NOW })
  const disk = new Map([[`${STATE}/trigger.json`, JSON.stringify({ key: 'k', why: 'x', resetsAt: 'soon', at: '2026-11-19T09:00:00Z' })]])
  const { submitted } = host(on, 's2', undefined, disk)
  on('session.start', (_, e) => ({ cwd: e.cwd }))
  await start($)
  await clock.advance(30_000)
  expect(submitted).toEqual([])
})

test('a trigger with no reset time still tells sessions while it is under five hours old', async ($, on) => {
  const clock = mock.clock(on, { now: NOW })
  const disk = new Map([[`${STATE}/trigger.json`, JSON.stringify({ key: 'k', why: 'x', at: '2026-11-19T09:00:00Z' })]])
  const { submitted } = host(on, 's2', undefined, disk)
  on('session.start', (_, e) => ({ cwd: e.cwd }))
  await start($)
  await clock.advance(30_000)
  expect(submitted).toHaveLength(1)
})

test('the usage share comes from params', async ($, on) => {
  mock.clock(on, { now: NOW })
  const { submitted } = host(on, 's1', { context: 70, usage: 50 })
  on('session.start', (_, e) => ({ cwd: e.cwd }))
  on('session.measure', (_, e) => ({ changed: e.changed }))
  await start($)
  await $.session.measure(usage(10, 55))
  expect(submitted).toHaveLength(1)
})

test('a session id that is not a plain name is never written as a file', async ($, on) => {
  mock.clock(on, { now: NOW })
  const { disk } = host(on, '../x')
  on('session.start', (_, e) => ({ cwd: e.cwd }))
  on('session.measure', (_, e) => ({ changed: e.changed }))
  await start($)
  await $.session.measure(usage(80))
  expect([...disk.keys()].filter(k => k.includes('sessions'))).toEqual([])
})

test('the context share can be crossed and asked about again after the session falls back under it', async ($, on) => {
  mock.clock(on, { now: NOW })
  const { submitted } = host(on, 's1')
  on('session.start', (_, e) => ({ cwd: e.cwd }))
  on('session.measure', (_, e) => ({ changed: e.changed }))
  on('turn.complete', (_, e) => ({ text: e.answer }))
  await start($)
  await $.session.measure(usage(80))
  await $.session.measure(usage(85))
  expect(submitted).toHaveLength(1)
  await $.session.measure(usage(20))
  await $.session.measure(usage(80))
  expect(submitted).toHaveLength(2)
})

test('a measurement without a context percent does not re-arm the context crossing', async ($, on) => {
  mock.clock(on, { now: NOW })
  const { submitted } = host(on, 's1')
  on('session.start', (_, e) => ({ cwd: e.cwd }))
  on('session.measure', (_, e) => ({ changed: e.changed }))
  await start($)
  await $.session.measure(usage(80))
  await $.session.measure({ ...usage(80), context: { window: 200_000 }, changed: ['rateLimits' as never] })
  await $.session.measure(usage(80))
  expect(submitted).toHaveLength(1)
})
