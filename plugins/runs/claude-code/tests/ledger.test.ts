import { expect, mock, test } from 'claude-code/testing'
import type { On } from 'claude-code'

import { finalLine, pullRequest } from '../hooks/register'

// These prove which `board put` and `gh` commands the hooks run, what records
// they hand `board`, and what the agent and the person are told. `board` and
// `gh` are stood in by a process.run hook, so the records are not checked
// against the shipped schemas here; tests/test_schema.py does that for the
// `written_by` field.

const NOW = Date.UTC(2026, 9, 25, 9, 30, 0)
const URL = 'https://github.com/acme/widgets/pull/42'

type Put = { argv: readonly string[]; record: Record<string, unknown> | undefined }

/** Stands in for `board` and `gh`: each `board put` answers `answer(path)`, and `gh pr view` prints a title. */
function host(on: On, answer: (path: string) => { exitCode: number; stdout: string } = () => ({ exitCode: 0, stdout: '{}' })) {
  const puts: Put[] = []
  const logs: { text: string; to: string }[] = []
  const statuses: (string | undefined)[] = []
  const whole = { stderr: '', isStdoutTruncated: false, isStderrTruncated: false }
  on('process.run', (_, e) => {
    if (e.argv[0] === 'gh') return { value: { exitCode: 0, stdout: 'Widgets ship in blue\n', ...whole } }
    const stdin = e.init?.stdin
    puts.push({ argv: e.argv, record: stdin === undefined ? undefined : JSON.parse(stdin) })
    const path = e.argv[e.argv.indexOf('put') + 1] ?? ''
    return { value: { ...answer(path), ...whole } }
  })
  on('ui.log', (_, e) => {
    logs.push({ text: e.text, to: e.to })
    return { value: undefined }
  })
  on('ui.status', (_, e) => {
    statuses.push(e.text)
    return { value: undefined }
  })
  on('session.id', () => ({ value: 'session-1' }))
  const session = mock.session(on)
  const appended = () => session.appended().map(row => row.message.content)
  return { puts, logs, statuses, appended }
}

test('a subagent that stops is recorded as a run with its duration and final line', async ($, on) => {
  const clock = mock.clock(on, { now: NOW })
  const { puts, logs, statuses } = host(on)
  on('classic.SubagentStart', () => ({}))
  on('classic.SubagentStop', () => ({}))

  await $.classic.SubagentStart({ agent_id: 'a1b2', agent_type: 'Explore' })
  await clock.advance(90_000)
  await $.classic.SubagentStop({
    agent_id: 'a1b2',
    agent_type: 'Explore',
    agent_transcript_path: '/t',
    stop_hook_active: false,
    last_assistant_message: 'Searched the crates.\n\nThe parser lives in src/parse.rs.\n',
  })

  expect(puts.map(p => p.argv)).toEqual([
    ['board', '--json', 'put', '/suites/claude-code', '--expect', 'none'],
    ['board', '--json', 'put', '/suites/claude-code/runs/20261025T093000Z-a1b2'],
  ])
  expect(puts[0]?.record?.$type).toBe('runs/suite/v1')
  expect(puts[1]?.record).toEqual({
    $type: 'runs/run/v1',
    suite: '/suites/claude-code',
    by: 'Explore',
    started: '2026-10-25T09:30:00Z',
    finished: '2026-10-25T09:31:30Z',
    scope: 'full',
    results: [],
    measures: { duration_s: 90 },
    summary: 'The parser lives in src/parse.rs.',
    written_by: 'runs/subagent-stop',
  })
  expect(logs).toEqual([{ text: 'board: recorded the Explore subagent as /suites/claude-code/runs/20261025T093000Z-a1b2', to: 'transcript' }])
  expect(statuses).toEqual(['board: 1 record this session'])
})

test('a subagent that a stop hook keeps running is not recorded until it stops', async ($, on) => {
  mock.clock(on, { now: NOW })
  const { puts } = host(on)
  on('classic.SubagentStop', () => ({ block: 'Run the tests first.' }))
  await $.classic.SubagentStop({ agent_id: 'a1', agent_type: 'Explore', agent_transcript_path: '/t', stop_hook_active: false })
  expect(puts).toEqual([])
})

test('a completed task is recorded under the configured board and suite, with its subject as the summary', { options: { board: '/boards/work', suite: '/suites/agents' } }, async ($, on) => {
  const clock = mock.clock(on, { now: NOW })
  const { puts } = host(on)
  on('classic.TaskCreated', () => ({}))
  on('classic.TaskCompleted', () => ({}))
  await $.classic.TaskCreated({ task_id: '7', task_subject: 'Write the parser' })
  await clock.advance(5_000)
  await $.classic.TaskCompleted({ task_id: '7', task_subject: 'Write the parser', teammate_name: 'builder' })
  expect(puts[1]?.argv).toEqual(['board', '-C', '/boards/work', '--json', 'put', '/suites/agents/runs/20261025T093000Z-7'])
  expect(puts[1]?.record).toMatchObject({
    by: 'builder',
    measures: { duration_s: 5 },
    summary: 'Write the parser',
    written_by: 'runs/task-completed',
  })
})

test('gh pr create writes a decision stub and asks the agent for its why and alternatives', async ($, on) => {
  mock.clock(on, { now: NOW })
  const { puts, statuses, appended } = host(on)
  on('tool.call', () => ({ result: { stdout: `Creating pull request\n${URL}\n`, stderr: '', interrupted: false } }))

  await $.tool.call({ tool: 'Bash', command: 'gh pr create --fill' })

  expect(puts.map(p => p.argv)).toEqual([['board', '--json', 'put', '/decisions/20261025T093000Z-widgets-42']])
  expect(puts[0]?.record).toEqual({
    $type: 'plans/decision/v1',
    decision: 'Widgets ship in blue',
    why: '',
    session: 'session-1',
    decided: '2026-10-25T09:30:00Z',
    refs: [URL],
    review: 'wanted',
    written_by: 'runs/pr-create',
  })
  expect(appended()).toEqual([[
    {
      type: 'text',
      text:
        `The runs mod recorded ${URL} as a decision stub at /decisions/20261025T093000Z-widgets-42. ` +
        'Its why is empty. Fill in why and alternatives: read it with board get, then board put it back.',
    },
  ]])
  expect(statuses).toEqual(['board: 1 record this session'])
})

test('a board without the plans type gets no stub, and only the debug log says so', async ($, on) => {
  mock.clock(on, { now: NOW })
  const unknown = JSON.stringify({ code: 'board::record::unknown_type', message: 'no schema for plans/decision/v1' })
  const { logs, statuses, appended } = host(on, () => ({ exitCode: 1, stdout: unknown }))
  on('tool.call', () => ({ result: { stdout: `${URL}\n`, stderr: '', interrupted: false } }))
  await $.tool.call({ tool: 'Bash', command: 'gh pr create -t x -b y' })
  expect(appended()).toEqual([])
  expect(statuses).toEqual([])
  expect(logs).toEqual([{ text: 'board: no decision stub for /decisions/20261025T093000Z-widgets-42: board::record::unknown_type no schema for plans/decision/v1', to: 'debug' }])
})

test('output with no pull request URL is logged and nothing is written; a refused put is logged where the person sees it', async ($, on) => {
  mock.clock(on, { now: NOW })
  const invalid = JSON.stringify({ code: 'board::record::invalid', message: 'written_by is not allowed' })
  const { puts, logs } = host(on, path => (path.startsWith('/decisions/') ? { exitCode: 1, stdout: invalid } : { exitCode: 0, stdout: '{}' }))
  let stdout = 'pull request create failed\n'
  on('tool.call', () => ({ result: { stdout, stderr: '', interrupted: false } }))

  await $.tool.call({ tool: 'Bash', command: 'gh pr create --fill' })
  expect(puts).toEqual([])
  stdout = `${URL}\n`
  await $.tool.call({ tool: 'Bash', command: 'gh pr list' })
  expect(puts).toEqual([])
  await $.tool.call({ tool: 'Bash', command: 'git push && gh pr create --fill' })
  expect(logs).toEqual([
    { text: 'board: gh pr create printed no pull request URL, so no decision stub was written', to: 'debug' },
    { text: 'board: no decision stub for /decisions/20261025T093000Z-widgets-42: board::record::invalid written_by is not allowed', to: 'transcript' },
  ])
})

test('the final line is the last line with text, and a pull request URL names its repository and number', () => {
  expect(finalLine('a\n\n  b  \n\n')).toBe('b')
  expect(finalLine('\n')).toBeUndefined()
  expect(finalLine(undefined)).toBeUndefined()
  expect(pullRequest(`Warning: 1 uncommitted change\n${URL}\n`)).toEqual({ url: URL, name: 'widgets-42' })
  expect(pullRequest('https://github.com/acme/widgets/issues/42')).toBeUndefined()
})
