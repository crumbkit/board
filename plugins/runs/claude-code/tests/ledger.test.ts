import { expect, mock, test } from 'claude-code/testing'
import type { On } from 'claude-code'

import { boardLine, commands, finalLine, opensPullRequest, pullRequest, remoteOf, target } from '../hooks/register'

// These prove which `board`, `git` and `gh` commands the hooks run, what
// records they hand `board`, what a subagent is started with, and what the
// agent and the person are told. `board`, `git` and `gh` are stood in by a
// process.run hook, so the records are not checked against the shipped
// schemas here; tests/test_schema.py does that.

const NOW = Date.UTC(2026, 9, 25, 9, 30, 0)
const URL = 'https://github.com/acme/widgets/pull/42'
const SESSION = 'session-1'
const whole = { stderr: '', isStdoutTruncated: false, isStderrTruncated: false }

type Put = { argv: readonly string[]; stdin: string | undefined }
type Answer = { exitCode: number; stdout: string }
type Changes = { path: string }[]

/**
 * Stands in for `board`, `git` and `gh`. `open(args)` gives the changes
 * `changes.open` answers for its flags, `brief` what `changes.brief` prints,
 * and `answer(path)` what a put answers. git reports the origin and branch
 * of `/work/widgets` only.
 */
function host(
  on: On,
  {
    open = () => [] as Changes,
    brief = { plan: '/changes/w/plans/1.md', next: 'build' } as object | null,
    answer = () => ({ exitCode: 0, stdout: '{"data":{}}' }) as Answer,
    env = {} as Record<string, string>,
  }: { open?: (args: readonly string[]) => Changes; brief?: object | null; answer?: (path: string) => Answer; env?: Record<string, string> } = {},
) {
  const puts: Put[] = []
  const calls: (readonly string[])[] = []
  const gits: (readonly string[])[] = []
  const logs: { text: string; to: string }[] = []
  const statuses: (string | undefined)[] = []
  on('process.run', (_, e) => {
    const [program, ...rest] = e.argv
    if (program === 'git') {
      gits.push(e.argv)
      const known = rest[1] === '/work/widgets'
      const out = rest.includes('get-url') ? 'git@github.com:acme/widgets.git\n' : 'blue\n'
      return { value: { exitCode: known ? 0 : 128, stdout: known ? out : '', ...whole } }
    }
    if (rest.includes('call')) {
      calls.push(e.argv)
      const capability = rest[rest.indexOf('call') + 1]
      if (capability === 'changes.brief' && brief === null) {
        return { value: { exitCode: 1, stdout: JSON.stringify({ code: 'changes::change::not_found', message: 'no change' }), ...whole } }
      }
      const output = capability === 'changes.brief' ? brief : { changes: open(rest), total: open(rest).length }
      return { value: { exitCode: 0, stdout: JSON.stringify({ data: { output } }), ...whole } }
    }
    puts.push({ argv: e.argv, stdin: e.init?.stdin })
    const path = rest[rest.indexOf('put') + 1] ?? ''
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
  on('session.id', () => ({ value: SESSION }))
  on('session.cwd', () => ({ value: '/work/widgets' }))
  mock.env(on, { HOME: '/home/owner', ...env })
  return { puts, calls, gits, logs, statuses }
}

const record = (put: Put | undefined) => (put?.stdin === undefined ? undefined : JSON.parse(put.stdin))
const lines = (put: Put | undefined) => put?.stdin?.trimEnd().split('\n').map(line => JSON.parse(line))

/** An engine that starts every subagent as `agent-7`, keeping the prompt it was started with. */
function engine(on: On) {
  const prompts: string[] = []
  on('agent.spawn', (_, e) => {
    prompts.push(e.prompt)
    return { model: 'm', agentId: 'agent-7' }
  })
  return prompts
}

test('a subagent that starts is recorded with its prompt, and the run at its stop names that record', async ($, on) => {
  const clock = mock.clock(on, { now: NOW })
  const { puts, logs } = host(on)
  const prompts = engine(on)
  on('classic.SubagentStart', () => ({}))
  on('classic.SubagentStop', () => ({}))

  await $.agent.spawn({ prompt: 'Find the parser.', cwd: '/work/widgets-wt' })
  await $.classic.SubagentStart({ agent_id: 'agent-7', agent_type: 'Explore' })
  await clock.advance(90_000)
  await $.classic.SubagentStop({
    agent_id: 'agent-7',
    agent_type: 'Explore',
    agent_transcript_path: '/t',
    stop_hook_active: false,
    last_assistant_message: 'Searched the crates.\n\nThe parser lives in src/parse.rs.\n',
  })

  expect(prompts).toEqual(['Find the parser.'])
  expect(puts.map(p => p.argv)).toEqual([
    ['board', '--personal', '--json', 'put', '--batch'],
    ['board', '--personal', '--json', 'put', '/suites/claude-code', '--expect', 'none'],
    ['board', '--personal', '--json', 'put', '/suites/claude-code/runs/20261025T093000Z-agent-7'],
  ])
  expect(lines(puts[0])).toEqual([
    { path: '/agents/agent-7.md', expect: 'none', text: 'Find the parser.' },
    {
      path: '/agents/agent-7',
      expect: 'none',
      record: {
        $type: 'runs/agent/v1',
        session: SESSION,
        cwd: '/work/widgets-wt',
        started: '2026-10-25T09:30:00Z',
        written_by: 'runs/agent-spawn',
      },
    },
  ])
  expect(record(puts[2])).toEqual({
    $type: 'runs/run/v1',
    suite: '/suites/claude-code',
    by: 'Explore',
    started: '2026-10-25T09:30:00Z',
    finished: '2026-10-25T09:31:30Z',
    scope: 'full',
    results: [],
    measures: { duration_s: 90 },
    summary: 'The parser lives in src/parse.rs.',
    agent: '/agents/agent-7',
    written_by: 'runs/subagent-stop',
  })
  expect(logs.map(log => log.text)).toEqual([
    'board: recorded the subagent agent-7 as /agents/agent-7',
    'board: recorded the Explore subagent as /suites/claude-code/runs/20261025T093000Z-agent-7',
  ])
})

test('a subagent the mod did not see start is recorded at its stop without an agent', async ($, on) => {
  mock.clock(on, { now: NOW })
  const { puts } = host(on)
  on('classic.SubagentStop', () => ({}))
  await $.classic.SubagentStop({ agent_id: 'a1', agent_type: 'Explore', agent_transcript_path: '/t', stop_hook_active: false })
  expect(record(puts[1])?.agent).toBeUndefined()
})

test('a prompt that ends with a Board line works on that change and starts with its brief', async ($, on) => {
  mock.clock(on, { now: NOW })
  const { puts, calls } = host(on)
  const prompts = engine(on)
  const prompt = 'Build the parser.\n\nBoard: /changes/parser\n'
  await $.agent.spawn({ prompt })
  expect(calls).toEqual([['board', '--personal', '--json', 'call', 'changes.brief', '--path', '/changes/parser']])
  expect(prompts[0]).toBe(
    'The runs mod ran `board --personal call changes.brief --path /changes/parser` before you started. It printed:\n\n' +
      '{\n  "plan": "/changes/w/plans/1.md",\n  "next": "build"\n}\n\n' +
      prompt,
  )
  const [prose, agent] = lines(puts[0])!
  expect(prose.text).toBe(prompt)
  expect(agent.record.works_on).toBe('/changes/parser')
  expect(agent.record.cwd).toBe('/work/widgets')
})

test('without a Board line the session’s one held change is the one worked on, and two held changes give none', async ($, on) => {
  mock.clock(on, { now: NOW })
  let held: Changes = [{ path: '/changes/held' }]
  const { puts, calls } = host(on, { open: args => (args.includes('--holder') ? held : []) })
  const prompts = engine(on)
  await $.agent.spawn({ prompt: 'Build the parser.' })
  expect(calls[0]).toEqual(['board', '--personal', '--json', 'call', 'changes.open', '--holder', SESSION])
  expect(lines(puts[0])![1].record.works_on).toBe('/changes/held')
  expect(prompts[0]).toContain('changes.brief --path /changes/held')

  held = [{ path: '/changes/a' }, { path: '/changes/b' }]
  await $.agent.spawn({ prompt: 'Build the lexer.' })
  expect(lines(puts[1])![1].record.works_on).toBeUndefined()
  expect(prompts[1]).toBe('Build the lexer.')
})

test('a resumed or refused spawn writes no agent record', async ($, on) => {
  mock.clock(on, { now: NOW })
  const { puts } = host(on)
  let deny: string | undefined
  on('agent.spawn', () => (deny === undefined ? { model: 'm', agentId: 'agent-7' } : { deny }))
  await $.agent.spawn({ prompt: 'Go on.', resume: { agentId: 'agent-7' } } as never)
  deny = 'not now'
  await $.agent.spawn({ prompt: 'Start.' })
  expect(puts).toEqual([])
  deny = undefined
  await $.agent.spawn({ prompt: 'Start.' })
  expect(puts.length).toBe(1)
})

test('a completed task is recorded under the configured suite, with its subject as the summary', { options: { suite: '/suites/agents' } }, async ($, on) => {
  const clock = mock.clock(on, { now: NOW })
  const { puts } = host(on)
  on('classic.TaskCreated', () => ({}))
  on('classic.TaskCompleted', () => ({}))
  await $.classic.TaskCreated({ task_id: '7', task_subject: 'Write the parser' })
  await clock.advance(5_000)
  await $.classic.TaskCompleted({ task_id: '7', task_subject: 'Write the parser', teammate_name: 'builder' })
  expect(puts[1]?.argv).toEqual(['board', '--personal', '--json', 'put', '/suites/agents/runs/20261025T093000Z-7'])
  expect(record(puts[1])).toMatchObject({ by: 'builder', measures: { duration_s: 5 }, summary: 'Write the parser', written_by: 'runs/task-completed' })
})

/** A `gh pr create` that prints `stdout`, counting the times it ran. */
function gh(on: On, stdout = `Creating pull request\n${URL}\n`) {
  const ran: string[] = []
  on('tool.call', (_, e) => {
    ran.push(e.tool === 'Bash' ? e.command : '')
    return { result: { stdout, stderr: '', interrupted: false } }
  })
  return ran
}

test('gh pr create on a branch an open change names runs, and its URL becomes that change’s opened record', async ($, on) => {
  mock.clock(on, { now: NOW })
  const { puts, calls, gits, statuses } = host(on, { open: args => (args.includes('blue') ? [{ path: '/changes/blue' }] : []) })
  const ran = gh(on)
  const result = await $.tool.call({ tool: 'Bash', command: 'gh pr create --fill' })
  expect(result.deny).toBeUndefined()
  expect(ran).toEqual(['gh pr create --fill'])
  expect(gits).toEqual([
    ['git', '-C', '/work/widgets', 'remote', 'get-url', 'origin'],
    ['git', '-C', '/work/widgets', 'symbolic-ref', '--quiet', '--short', 'HEAD'],
  ])
  expect(calls).toEqual([['board', '--personal', '--json', 'call', 'changes.open', '--remote', 'git@github.com:acme/widgets.git', '--branch', 'blue']])
  expect(puts.map(p => p.argv)).toEqual([['board', '--personal', '--json', 'put', '/changes/blue/opened', '--expect', 'none']])
  expect(record(puts[0])).toEqual({ $type: 'changes/opened/v1', pull_request: URL })
  expect(statuses).toEqual(['board: 1 record this session'])
})

test('gh pr create on a branch no open change names is refused before it runs, and says how to start one', async ($, on) => {
  mock.clock(on, { now: NOW })
  const { puts } = host(on)
  const ran = gh(on)
  const result = await $.tool.call({ tool: 'Bash', command: 'git push && gh pr create --fill' })
  expect(ran).toEqual([])
  expect(puts).toEqual([])
  expect(result.deny).toBe(
    'board: refused: no open change names git@github.com:acme/widgets.git on blue, so the pull request would have no record. ' +
      "From the clone, on blue, run `board --personal call changes.start --title '<title>' --session <session>`, " +
      'which records the repository too when the board has no record of it, then run gh pr create again.',
  )
})

test('-R and --head name the target, and a leading cd names the directory for what they leave out', async ($, on) => {
  mock.clock(on, { now: NOW })
  const { calls, gits } = host(on, { open: () => [{ path: '/changes/x' }] })
  gh(on)
  await $.tool.call({ tool: 'Bash', command: "gh pr create -R github.com/acme/widgets --head 'acme:green' --fill" })
  expect(gits).toEqual([])
  expect(calls.at(-1)?.slice(-4)).toEqual(['--remote', 'https://github.com/acme/widgets', '--branch', 'green'])
  await $.tool.call({ tool: 'Bash', command: 'cd ~/elsewhere && gh pr create --head green' })
  expect(gits.at(-1)).toEqual(['git', '-C', '/home/owner/elsewhere', 'remote', 'get-url', 'origin'])
})

test('gh pr create after another cd, or in a directory with no origin, is refused and says to give -R and --head', async ($, on) => {
  mock.clock(on, { now: NOW })
  host(on, { open: () => [{ path: '/changes/x' }] })
  const ran = gh(on)
  const after = await $.tool.call({ tool: 'Bash', command: 'cd /work/widgets && cd sub && gh pr create --fill' })
  expect(after.deny).toBe('board: refused: the command changes directory before gh pr create, so its repository and branch are unknown; run gh pr create with -R <owner/name> and --head <branch>')
  const bare = await $.tool.call({ tool: 'Bash', command: 'cd /tmp && gh pr create --fill' })
  expect(bare.deny).toBe("board: refused: /tmp has no origin, so the pull request's repository and branch are unknown; run gh pr create with -R <owner/name> and --head <branch>")
  const named = await $.tool.call({ tool: 'Bash', command: 'cd /work/widgets && cd sub && gh pr create -R acme/widgets -H blue' })
  expect(named.deny).toBeUndefined()
  expect(ran).toEqual(['cd /work/widgets && cd sub && gh pr create -R acme/widgets -H blue'])
})

test('a pull request that prints no URL, or whose branch two changes name, writes no opened record', async ($, on) => {
  mock.clock(on, { now: NOW })
  let open: Changes = [{ path: '/changes/x' }]
  const { puts, logs } = host(on, { open: () => open })
  let stdout = 'pull request create failed\n'
  on('tool.call', () => ({ result: { stdout, stderr: '', interrupted: false } }))
  await $.tool.call({ tool: 'Bash', command: 'gh pr create --fill' })
  open = [{ path: '/changes/x' }, { path: '/changes/y' }]
  stdout = `${URL}\n`
  await $.tool.call({ tool: 'Bash', command: 'gh pr create --fill' })
  expect(puts).toEqual([])
  expect(logs).toEqual([
    { text: 'board: gh pr create printed no pull request URL, so no opened record was written', to: 'debug' },
    { text: `board: 2 open changes name this branch (/changes/x, /changes/y), so no opened record was written for ${URL}`, to: 'transcript' },
  ])
})

test('a refused opened record is logged where the person sees it, and a missing plugin only in the debug log', async ($, on) => {
  mock.clock(on, { now: NOW })
  let code = 'board::put::stale'
  const { logs } = host(on, { open: () => [{ path: '/changes/x' }], answer: () => ({ exitCode: 1, stdout: JSON.stringify({ code, message: 'no' }) }) })
  gh(on)
  await $.tool.call({ tool: 'Bash', command: 'gh pr create --fill' })
  code = 'board::record::unknown_type'
  await $.tool.call({ tool: 'Bash', command: 'gh pr create --fill' })
  expect(logs).toEqual([
    { text: `board: no opened record for ${URL}: board::put::stale no`, to: 'transcript' },
    { text: `board: no opened record for ${URL}: board::record::unknown_type no`, to: 'debug' },
  ])
})

test('other Bash commands pass through the gate untouched', async ($, on) => {
  mock.clock(on, { now: NOW })
  const { calls } = host(on)
  const ran = gh(on)
  await $.tool.call({ tool: 'Bash', command: 'gh pr list' })
  expect(ran).toEqual(['gh pr list'])
  expect(calls).toEqual([])
})

test('a Board line whose brief cannot be read works on nothing and leaves the prompt as it was', async ($, on) => {
  mock.clock(on, { now: NOW })
  const { puts } = host(on, { brief: null })
  const prompts = engine(on)
  await $.agent.spawn({ prompt: 'Build it.\nBoard: /changes/gone' })
  expect(prompts).toEqual(['Build it.\nBoard: /changes/gone'])
  expect(lines(puts[0])![1].record.works_on).toBeUndefined()
})

test('the desktop session id names the agent and the holder, and the transcript id stands in without one', async ($, on) => {
  mock.clock(on, { now: NOW })
  const { puts, calls } = host(on, { env: { CLAUDE_CODE_HOST_SESSION_ID: 'local_0a1b' } })
  engine(on)
  await $.agent.spawn({ prompt: 'Build it.' })
  expect(calls[0]?.at(-1)).toBe('local_0a1b')
  expect(lines(puts[0])![1].record.session).toBe('local_0a1b')
})

test('a teammate or a workflow agent gets no agent record, and a subagent does', async ($, on) => {
  mock.clock(on, { now: NOW })
  const { puts } = host(on)
  let extra: object = { isTeammate: true }
  on('agent.spawn', () => ({ model: 'm', agentId: 'agent-7' }))
  await $.agent.spawn({ prompt: 'Help.', ...extra } as never)
  extra = { workflow: { runId: 'wf_1', agentIndex: 1 } }
  await $.agent.spawn({ prompt: 'Help.', ...extra } as never)
  expect(puts).toEqual([])
  await $.agent.spawn({ prompt: 'Help.' })
  expect(puts.length).toBe(1)
})

test('a gate that fails before gh runs refuses it, and one that answers lets it run', async ($, on) => {
  mock.clock(on, { now: NOW })
  let fail = true
  const whole2 = { stderr: '', isStdoutTruncated: false, isStderrTruncated: false }
  on('process.run', (_, e) => {
    if (fail) throw new Error('board is gone')
    if (e.argv[0] === 'git') return { value: { exitCode: 0, stdout: 'x\n', ...whole2 } }
    return { value: { exitCode: 0, stdout: JSON.stringify({ data: { output: { changes: [{ path: '/changes/x' }], total: 1 } } }), ...whole2 } }
  })
  on('session.cwd', () => ({ value: '/work/widgets' }))
  mock.env(on, { HOME: '/home/owner' })
  const ran = gh(on, 'done\n')
  const failed = await $.tool.call({ tool: 'Bash', command: 'gh pr create --fill' })
  expect(failed.deny).toBe('board: refused: the pull request gate failed, so it could not look for an open change; `claude --debug` logs why. Run gh pr create again.')
  expect(ran).toEqual([])
  fail = false
  const answered = await $.tool.call({ tool: 'Bash', command: 'gh pr create --fill' })
  expect(answered.deny).toBeUndefined()
  expect(ran).toEqual(['gh pr create --fill'])
})

test('gh pr new is gated as gh pr create is, and a commit message or heredoc that mentions it is not', async ($, on) => {
  mock.clock(on, { now: NOW })
  const { calls } = host(on)
  const ran = gh(on)
  const refused = await $.tool.call({ tool: 'Bash', command: 'GH_DEBUG=1 gh pr new --fill' })
  expect(refused.deny).toStartWith('board: refused: no open change names')
  await $.tool.call({ tool: 'Bash', command: 'git commit -m "Run gh pr create && gh pr new after this"' })
  await $.tool.call({ tool: 'Bash', command: "cat > body.md <<'EOF'\ngh pr create --fill\nEOF\ngit status" })
  expect(ran).toEqual(['git commit -m "Run gh pr create && gh pr new after this"', "cat > body.md <<'EOF'\ngh pr create --fill\nEOF\ngit status"])
  expect(calls.length).toBe(1)
})

test('the helpers read the final line, a Board line, a pull request URL, a remote and a target', () => {
  expect(finalLine('a\n\n  b  \n\n')).toBe('b')
  expect(finalLine(undefined)).toBeUndefined()
  expect(boardLine('Do it.\nBoard: /changes/parser\n')).toBe('/changes/parser')
  expect(boardLine('Board: /changes/parser\nDo it.')).toBeUndefined()
  expect(boardLine('Do it.\nBoard: /evals/e')).toBeUndefined()
  expect(pullRequest(`Warning: 1 uncommitted change\n${URL}\n`)).toEqual({ url: URL, repo: 'widgets', number: '42' })
  expect(pullRequest('https://github.com/acme/widgets/issues/42')).toBeUndefined()
  expect(remoteOf('github.com/acme/widgets')).toBe('https://github.com/acme/widgets')
  expect(remoteOf('acme/widgets')).toBe('acme/widgets')
  expect(target('gh pr create --repo=acme/w -Hblue')).toEqual({ repo: 'acme/w', head: 'blue', dir: undefined })
  expect(target('cd "/a b" && gh pr create --fill && cd /c')).toEqual({ repo: undefined, head: undefined, dir: '/a b' })
  expect(target('echo "cd x" && gh pr create')).toEqual({ repo: undefined, head: undefined, dir: undefined })
  expect(commands("A=1 B='2 3' gh pr view; (cd x && ls) | wc")).toEqual([
    { words: ['gh', 'pr', 'view'], then: ';' },
    { words: ['cd', 'x'], then: '&&' },
    { words: ['ls'], then: ')' },
    { words: ['wc'], then: '' },
  ])
  expect(opensPullRequest('git push && gh pr create --fill')).toBe(true)
  expect(opensPullRequest("echo 'gh pr create'")).toBe(false)
})
