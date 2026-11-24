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

type Shares = { tokens?: number; usage?: number } | 'refused' | 'throws'

/** Stands in for `board`, the disk under `STATE` and the session's prompt submission. A write to a path in `failing` throws. */
function host(on: On, who: string | { id: string }, shares: Shares = { tokens: 280_000, usage: 85 }, disk = new Map<string, string>(), failing = new Set<string>()) {
  const me = typeof who === 'string' ? { id: who } : who
  const submitted: string[] = []
  const logs: string[] = []
  const calls: string[] = []
  on('process.run', (_, e) => {
    if (shares === 'throws') throw new Error('board missing')
    const name = e.argv[e.argv.indexOf('--name') + 1]!
    calls.push(name)
    const value = shares === 'refused' ? undefined : name === 'handoff-context-tokens' ? shares.tokens : name === 'handoff-usage-share' ? shares.usage : undefined
    if (value === undefined) return { value: { exitCode: 1, stdout: '{"code":"params::heuristic::not_found"}', ...whole } }
    return { value: { exitCode: 0, stdout: JSON.stringify({ data: { output: { value, source: 'value' } } }), ...whole } }
  })
  on('fs.write', (_, e) => {
    if (failing.has(e.path)) throw new Error('disk full')
    disk.set(e.path, e.text)
    return { value: undefined }
  })
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
  return { disk, submitted, logs, calls }
}

/** A measurement with `tokens` in the context window and the two usage windows at `five` and `seven` percent. */
const usage = (tokens: number, five = 10, seven = 10) => ({
  context: { window: 1_000_000, tokens, percent: Math.round(tokens / 10_000) },
  rateLimits: [
    { kind: 'five_hour', percentUsed: five, resetsAt: RESET },
    { kind: 'seven_day', percentUsed: seven, resetsAt: '2026-11-24T00:00:00Z' },
  ],
  changed: ['context' as const],
})

async function start($: any, cwd = '/work/a') {
  await $.session.start({ cwd, surface: 'terminal', isInteractive: true })
}

test('crossing names the context limit first, then the first usage window at or over its share', () => {
  const limits = { contextTokens: 280_000, usage: 85 }
  expect(crossing(usage(279_999), limits)).toBeUndefined()
  expect(crossing(usage(280_000), limits)).toEqual({ key: 'context-tokens', why: 'its context holds 280,000 tokens, at or over its limit of 280,000' })
  expect(crossing(usage(10_000, 85), limits)).toEqual({ key: `five_hour@${RESET}`, why: 'the five_hour usage limit is 85% used', window: { kind: 'five_hour', resetsAt: RESET } })
  expect(crossing(usage(10_000, 84.9, 90), limits)?.key).toBe('seven_day@2026-11-24T00:00:00Z')
})

test('279,999 tokens do not prompt; 280,000 tokens prompt once, name the count and the limit, and tell no other session', async ($, on) => {
  mock.clock(on, { now: NOW })
  const { disk, submitted } = host(on, 's1', 'refused')
  on('session.start', (_, e) => ({ cwd: e.cwd }))
  on('session.measure', (_, e) => ({ changed: e.changed }))
  await start($)

  await $.session.measure(usage(279_999))
  expect(submitted).toEqual([])
  await $.session.measure(usage(280_000))
  await $.session.measure(usage(281_000))

  expect(submitted).toHaveLength(1)
  expect(submitted[0]).toMatch(/its context holds 280,000 tokens, at or over its limit of 280,000/)
  expect(submitted[0]).toMatch(/handoff skill/)
  expect(disk.has(`${STATE}/trigger.json`)).toBe(false)
  expect(JSON.parse(disk.get(`${STATE}/sessions/s1.json`)!)).toMatchObject({ cwd: '/work/a', asked: ['context-tokens'], status: 'asked' })
})

test('a session below both shares is left alone', async ($, on) => {
  mock.clock(on, { now: NOW })
  const { submitted, disk } = host(on, 's1')
  on('session.start', (_, e) => ({ cwd: e.cwd }))
  on('session.measure', (_, e) => ({ changed: e.changed }))
  await start($)
  await $.session.measure(usage(279_999, 84.9, 5))
  expect(submitted).toEqual([])
  expect(disk.size).toBe(0)
})

test('a usage window at its share makes the session hand off once and leaves a trigger for the other sessions', async ($, on) => {
  mock.clock(on, { now: NOW })
  const { disk, submitted } = host(on, 's1')
  on('session.start', (_, e) => ({ cwd: e.cwd }))
  on('session.measure', (_, e) => ({ changed: e.changed }))
  await start($)

  await $.session.measure(usage(10_000, 86))
  await $.session.measure(usage(10_000, 87))

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
  await $.session.measure(usage(300_000))

  await $.turn.complete({ answer: 'Pushed.\nhandoff: /changes/a — resume: board changes next --path /changes/a\nREADY TO ARCHIVE', durationMs: 1, isAborted: false, turnId: 't' } as never)

  const line = 'cd /work/a && claude --resume s1  # handoff: /changes/a — resume: board changes next --path /changes/a'
  expect(logs).toEqual([`handoff: ${line}`])
  expect(JSON.parse(disk.get(`${STATE}/sessions/s1.json`)!)).toMatchObject({ status: 'handed-off', line })

  const out = await $.command.run({ command: 'handoff-lines', args: '', origin: { kind: 'composer' } } as never)
  expect(out.text).toBe(`cd /work/z && claude --resume s0  # handoff: /changes/z — resume: board changes next --path /changes/z\n${line}`)
})

test('the built-in defaults apply when the board gives no value, and the mod says so once', async ($, on) => {
  const clock = mock.clock(on, { now: NOW })
  const { submitted, logs } = host(on, 's1', 'refused')
  on('session.start', (_, e) => ({ cwd: e.cwd }))
  on('session.measure', (_, e) => ({ changed: e.changed }))
  await start($)
  await $.session.measure(usage(279_999, 84.9))
  expect(submitted).toEqual([])
  expect(logs).toEqual(['handoff: params unreadable, using 280,000 context tokens and 85% usage'])
  await clock.advance(10 * 60_000)
  await $.session.measure(usage(280_000))
  expect(submitted).toHaveLength(1)
  expect(logs).toHaveLength(1)
})

test('a session that reloads does not get the prompt again for a crossing it was already asked', async ($, on) => {
  mock.clock(on, { now: NOW })
  const disk = new Map([[`${STATE}/sessions/s1.json`, JSON.stringify({ cwd: '/work/a', asked: ['context-tokens'], status: 'asked' })]])
  const { submitted } = host(on, 's1', undefined, disk)
  on('session.start', (_, e) => ({ cwd: e.cwd }))
  on('session.measure', (_, e) => ({ changed: e.changed }))
  await start($)
  await $.session.measure(usage(320_000))
  expect(submitted).toEqual([])
})

test('a session file from 0.1.1 that holds the old context key is still prompted at the token limit', async ($, on) => {
  mock.clock(on, { now: NOW })
  const disk = new Map([[`${STATE}/sessions/s1.json`, JSON.stringify({ cwd: '/work/a', asked: ['context'], status: 'asked' })]])
  const { submitted } = host(on, 's1', undefined, disk)
  on('session.start', (_, e) => ({ cwd: e.cwd }))
  on('session.measure', (_, e) => ({ changed: e.changed }))
  await start($)
  await $.session.measure(usage(300_000))
  expect(submitted).toHaveLength(1)
})

test('a usage window that crosses after the context limit did still leaves its trigger, and the session is not asked twice', async ($, on) => {
  mock.clock(on, { now: NOW })
  const { disk, submitted } = host(on, 's1')
  on('session.start', (_, e) => ({ cwd: e.cwd }))
  on('session.measure', (_, e) => ({ changed: e.changed }))
  await start($)
  await $.session.measure(usage(300_000))
  expect(disk.has(`${STATE}/trigger.json`)).toBe(false)

  await $.session.measure(usage(300_000, 90))

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
  await $.session.measure(usage(300_000))

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
  await $.session.measure(usage(300_000))
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
  await $.session.measure(usage(300_000))
  await $.turn.complete({ answer: 'handoff: /a', durationMs: 1, isAborted: false, turnId: 't' } as never)
  await $.session.measure(usage(300_000, 90))
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
  await $.session.measure(usage(300_000))
  who.id = 's2'
  await $.session.measure(usage(300_000))
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
  const { submitted } = host(on, 's1', { tokens: 280_000, usage: 50 })
  on('session.start', (_, e) => ({ cwd: e.cwd }))
  on('session.measure', (_, e) => ({ changed: e.changed }))
  await start($)
  await $.session.measure(usage(10_000, 55))
  expect(submitted).toHaveLength(1)
})

test('a session id that is not a plain name is never written as a file', async ($, on) => {
  mock.clock(on, { now: NOW })
  const { disk } = host(on, '../x')
  on('session.start', (_, e) => ({ cwd: e.cwd }))
  on('session.measure', (_, e) => ({ changed: e.changed }))
  await start($)
  await $.session.measure(usage(300_000))
  expect([...disk.keys()].filter(k => k.includes('sessions'))).toEqual([])
})

test('a count back under the limit, as after /compact, re-arms the context prompt', async ($, on) => {
  mock.clock(on, { now: NOW })
  const { submitted } = host(on, 's1')
  on('session.start', (_, e) => ({ cwd: e.cwd }))
  on('session.measure', (_, e) => ({ changed: e.changed }))
  on('turn.complete', (_, e) => ({ text: e.answer }))
  await start($)
  await $.session.measure(usage(300_000))
  await $.session.measure(usage(310_000))
  expect(submitted).toHaveLength(1)
  await $.session.measure(usage(20_000))
  await $.session.measure(usage(300_000))
  expect(submitted).toHaveLength(2)
})

test('a payload without context.tokens makes no context check and does not re-arm the context prompt', async ($, on) => {
  mock.clock(on, { now: NOW })
  const { submitted } = host(on, 's1')
  on('session.start', (_, e) => ({ cwd: e.cwd }))
  on('session.measure', (_, e) => ({ changed: e.changed }))
  await start($)
  await $.session.measure(usage(300_000))
  await $.session.measure({ ...usage(300_000), context: { window: 1_000_000 }, changed: ['rateLimits' as never] })
  await $.session.measure(usage(300_000))
  expect(submitted).toHaveLength(1)
})

const SEVEN = '2026-11-24T00:00:00Z'
const trigger = (disk: Map<string, string>) => JSON.parse(disk.get(`${STATE}/trigger.json`)!)

test('a usage reset that puts the window back under its share stops telling new sessions', async ($, on) => {
  const clock = mock.clock(on, { now: NOW })
  const who = { id: 's1' }
  const { disk, submitted } = host(on, who)
  on('session.start', (_, e) => ({ cwd: e.cwd }))
  on('session.measure', (_, e) => ({ changed: e.changed }))
  await start($)
  await $.session.measure(usage(10_000, 10, 86))
  expect(submitted).toHaveLength(1)

  await $.session.measure(usage(10_000, 10, 41))
  who.id = 's2'
  await clock.advance(30_000)

  expect(submitted).toHaveLength(1)
  expect(Date.parse(trigger(disk).resetsAt)).toBeLessThanOrEqual(NOW)
  expect(trigger(disk).key).toBe(`seven_day@${SEVEN}`)
})

test('a session whose window fell back under its share is told again when it crosses again', async ($, on) => {
  const clock = mock.clock(on, { now: NOW })
  const { disk, submitted } = host(on, 's1')
  on('session.start', (_, e) => ({ cwd: e.cwd }))
  on('session.measure', (_, e) => ({ changed: e.changed }))
  await start($)
  await $.session.measure(usage(10_000, 10, 86))
  await $.session.measure(usage(10_000, 10, 41))
  expect(JSON.parse(disk.get(`${STATE}/sessions/s1.json`)!).asked).toEqual([])

  await clock.advance(60_000)
  await $.session.measure(usage(10_000, 10, 86))

  expect(submitted).toHaveLength(2)
  expect(trigger(disk)).toEqual({ key: `seven_day@${SEVEN}`, why: 'the seven_day usage limit is 86% used', resetsAt: SEVEN, at: '2026-11-19T09:01:00Z' })
})

test('a reading under the share leaves a trigger for another window alone, and writes nothing when there is no trigger', async ($, on) => {
  mock.clock(on, { now: NOW })
  const five = JSON.stringify({ key: `five_hour@${RESET}`, why: 'the five_hour usage limit is 86% used', resetsAt: RESET, at: '2026-11-19T08:00:00Z' })
  const disk = new Map([[`${STATE}/trigger.json`, five]])
  const { submitted } = host(on, 's1', undefined, disk)
  on('session.start', (_, e) => ({ cwd: e.cwd }))
  on('session.measure', (_, e) => ({ changed: e.changed }))
  await start($)

  await $.session.measure({ ...usage(10_000), rateLimits: [{ kind: 'seven_day', percentUsed: 41, resetsAt: SEVEN }] })

  expect(disk.get(`${STATE}/trigger.json`)).toBe(five)
  expect(submitted).toEqual([])
  disk.delete(`${STATE}/trigger.json`)
  await $.session.measure(usage(10_000, 10, 41))
  expect(disk.size).toBe(0)
})

test('a trigger left before the fix is cleared by a reading of its window under the share, whatever that reading\'s reset time', async ($, on) => {
  const now = Date.UTC(2026, 10, 24, 12, 20, 0)
  const clock = mock.clock(on, { now })
  const old = { key: 'seven_day@2026-11-26T00:00:00.000Z', why: 'the seven_day usage limit is 85% used', resetsAt: '2026-11-26T00:00:00.000Z', at: '2026-11-22T03:13:00Z' }
  const who = { id: 's1' }
  const disk = new Map([[`${STATE}/trigger.json`, JSON.stringify(old)]])
  const { submitted } = host(on, who, undefined, disk)
  on('session.start', (_, e) => ({ cwd: e.cwd }))
  on('session.measure', (_, e) => ({ changed: e.changed }))
  await start($)

  await $.session.measure({ ...usage(10_000), rateLimits: [{ kind: 'seven_day', percentUsed: 41, resetsAt: '2026-11-25T00:00:00Z' }] })
  who.id = 's2'
  await clock.advance(30_000)

  expect(submitted).toEqual([])
  expect(trigger(disk)).toEqual({ key: old.key, why: 'the seven_day usage limit fell to 41% used', resetsAt: '2026-11-24T12:20:00Z', at: '2026-11-24T12:20:00Z' })
})

test('a trigger that cannot be cleared is logged and does not stop the measure', async ($, on) => {
  mock.clock(on, { now: NOW })
  const live = JSON.stringify({ key: `seven_day@${SEVEN}`, why: 'the seven_day usage limit is 86% used', resetsAt: SEVEN, at: '2026-11-19T08:00:00Z' })
  const disk = new Map([[`${STATE}/trigger.json`, live]])
  const { logs } = host(on, 's1', undefined, disk, new Set([`${STATE}/trigger.json`]))
  on('session.start', (_, e) => ({ cwd: e.cwd }))
  on('session.measure', (_, e) => ({ changed: e.changed }))
  await start($)

  const out = await $.session.measure(usage(10_000, 10, 41))

  expect(out).toEqual({ changed: ['context'] })
  expect(disk.get(`${STATE}/trigger.json`)).toBe(live)
  expect(logs.join('\n')).toMatch(/handoff: could not clear the trigger: /)
})

test('a reading just under the share, from a session whose usage lags, does not clear a real crossing', async ($, on) => {
  const clock = mock.clock(on, { now: NOW })
  const who = { id: 's1' }
  const { disk, submitted } = host(on, who)
  on('session.start', (_, e) => ({ cwd: e.cwd }))
  on('session.measure', (_, e) => ({ changed: e.changed }))
  await start($)
  await $.session.measure(usage(10_000, 10, 86))
  const written = disk.get(`${STATE}/trigger.json`)

  who.id = 's2'
  await $.session.measure(usage(10_000, 10, 84))
  who.id = 's3'
  await clock.advance(30_000)

  expect(disk.get(`${STATE}/trigger.json`)).toBe(written)
  expect(submitted).toHaveLength(2)
  expect(submitted[1]).toMatch(/seven_day usage limit is 86% used/)
})

test('a reading well under the share from another session clears the crossing for every idle session', async ($, on) => {
  const clock = mock.clock(on, { now: NOW })
  const who = { id: 's1' }
  const { submitted } = host(on, who)
  on('session.start', (_, e) => ({ cwd: e.cwd }))
  on('session.measure', (_, e) => ({ changed: e.changed }))
  await start($)
  await $.session.measure(usage(10_000, 10, 86))

  who.id = 's2'
  await $.session.measure(usage(10_000, 10, 41))
  who.id = 's3'
  await clock.advance(30_000)

  expect(submitted).toHaveLength(1)
})

test('an idle session told before a reset is told again when the window crosses again', async ($, on) => {
  const clock = mock.clock(on, { now: NOW })
  const who = { id: 's1' }
  const { disk, submitted } = host(on, who)
  on('session.start', (_, e) => ({ cwd: e.cwd }))
  on('session.measure', (_, e) => ({ changed: e.changed }))
  await start($)
  await $.session.measure(usage(10_000, 10, 86))
  who.id = 's3'
  await clock.advance(30_000)
  expect(submitted).toHaveLength(2)

  who.id = 's1'
  await $.session.measure(usage(10_000, 10, 41))
  who.id = 's3'
  await clock.advance(30_000)
  expect(JSON.parse(disk.get(`${STATE}/sessions/s3.json`)!).asked).toEqual([])

  who.id = 's1'
  await $.session.measure(usage(10_000, 10, 86))
  who.id = 's3'
  await clock.advance(30_000)
  expect(submitted).toHaveLength(4)
})

test('a second measurement under the share after a clear leaves the trigger as it is', async ($, on) => {
  const clock = mock.clock(on, { now: NOW })
  const { disk } = host(on, 's1')
  on('session.start', (_, e) => ({ cwd: e.cwd }))
  on('session.measure', (_, e) => ({ changed: e.changed }))
  await start($)
  await $.session.measure(usage(10_000, 10, 86))
  await $.session.measure(usage(10_000, 10, 41))
  const cleared = disk.get(`${STATE}/trigger.json`)

  await clock.advance(60_000)
  await $.session.measure(usage(10_000, 10, 41))

  expect(disk.get(`${STATE}/trigger.json`)).toBe(cleared)
})

test('a handoff-context-tokens value from params overrides the default', async ($, on) => {
  mock.clock(on, { now: NOW })
  const { submitted } = host(on, 's1', { tokens: 150_000, usage: 85 })
  on('session.start', (_, e) => ({ cwd: e.cwd }))
  on('session.measure', (_, e) => ({ changed: e.changed }))
  await start($)
  await $.session.measure(usage(149_999))
  expect(submitted).toEqual([])
  await $.session.measure(usage(150_000))
  expect(submitted).toHaveLength(1)
  expect(submitted[0]).toMatch(/150,000 tokens, at or over its limit of 150,000/)
})

test('a changed limit applies once the copy is over 5 minutes old, without a restart, and not before', async ($, on) => {
  const clock = mock.clock(on, { now: NOW })
  const shares = { tokens: 300_000, usage: 85 }
  const { submitted, calls } = host(on, 's1', shares)
  on('session.start', (_, e) => ({ cwd: e.cwd }))
  on('session.measure', (_, e) => ({ changed: e.changed }))
  await start($)
  await $.session.measure(usage(250_000))
  shares.tokens = 200_000

  await clock.set(NOW + 300_000)
  await $.session.measure(usage(250_000))
  expect(submitted).toEqual([])
  expect(calls).toEqual(['handoff-context-tokens', 'handoff-usage-share'])

  await clock.set(NOW + 300_001)
  await $.session.measure(usage(250_000))
  expect(submitted).toHaveLength(1)
  expect(submitted[0]).toMatch(/limit of 200,000/)
  expect(calls).toHaveLength(4)
})

test('a session told about seven_day usage is still told when it reaches the context limit', async ($, on) => {
  mock.clock(on, { now: NOW })
  const { submitted, disk } = host(on, 's1')
  on('session.start', (_, e) => ({ cwd: e.cwd }))
  on('session.measure', (_, e) => ({ changed: e.changed }))
  await start($)
  await $.session.measure(usage(10_000, 10, 86))
  expect(submitted).toHaveLength(1)

  await $.session.measure(usage(280_000, 10, 86))

  expect(submitted).toHaveLength(2)
  expect(submitted[1]).toMatch(/its context holds 280,000 tokens/)
  expect(JSON.parse(disk.get(`${STATE}/sessions/s1.json`)!).asked).toEqual([`seven_day@${SEVEN}`, 'context-tokens'])
})

test('a live seven_day trigger does not tell a session whose own seven_day reading is 41%, and tells a session with no reading', async ($, on) => {
  const clock = mock.clock(on, { now: NOW })
  const live = JSON.stringify({ key: `seven_day@${SEVEN}`, why: 'the seven_day usage limit is 86% used', resetsAt: SEVEN, at: '2026-11-19T08:00:00Z' })
  const who = { id: 's1' }
  // The trigger cannot be cleared, so it stays live and only the session's own reading can keep it from telling s1.
  const disk = new Map([[`${STATE}/trigger.json`, live]])
  const { submitted } = host(on, who, undefined, disk, new Set([`${STATE}/trigger.json`]))
  on('session.start', (_, e) => ({ cwd: e.cwd }))
  on('session.measure', (_, e) => ({ changed: e.changed }))
  await start($)
  await $.session.measure(usage(10_000, 10, 41))

  await clock.advance(30_000)
  expect(submitted).toEqual([])

  who.id = 's2'
  await clock.advance(30_000)
  expect(submitted).toHaveLength(1)
  expect(submitted[0]).toMatch(/seven_day usage limit is 86% used/)
})

test('a session whose own reading under the share is older than a fresh trigger is told', async ($, on) => {
  const clock = mock.clock(on, { now: NOW })
  const disk = new Map<string, string>()
  const { submitted } = host(on, 's1', undefined, disk)
  on('session.start', (_, e) => ({ cwd: e.cwd }))
  on('session.measure', (_, e) => ({ changed: e.changed }))
  await start($)
  await $.session.measure(usage(10_000, 10, 41))

  // Another session crosses 86% after this session's 41% reading, so its trigger is newer than the reading.
  await clock.advance(10_000)
  disk.set(`${STATE}/trigger.json`, JSON.stringify({ key: `seven_day@${SEVEN}`, why: 'the seven_day usage limit is 86% used', resetsAt: SEVEN, at: '2026-11-19T09:00:10Z' }))
  await clock.advance(20_000)

  expect(submitted).toHaveLength(1)
  expect(submitted[0]).toMatch(/seven_day usage limit is 86% used/)
})

test('a failed params refresh keeps the last good value, so the session is not re-armed and told twice', async ($, on) => {
  const clock = mock.clock(on, { now: NOW })
  const shares: { tokens?: number; usage?: number } = { tokens: 150_000, usage: 85 }
  const { submitted } = host(on, 's1', shares)
  on('session.start', (_, e) => ({ cwd: e.cwd }))
  on('session.measure', (_, e) => ({ changed: e.changed }))
  await start($)
  await $.session.measure(usage(200_000))
  expect(submitted).toHaveLength(1)

  shares.tokens = undefined
  await clock.advance(300_001)
  await $.session.measure(usage(200_000))
  shares.tokens = 150_000
  await clock.advance(300_001)
  await $.session.measure(usage(200_000))

  expect(submitted).toHaveLength(1)
})
