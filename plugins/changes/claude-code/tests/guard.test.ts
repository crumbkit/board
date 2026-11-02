import { expect, test } from 'claude-code/testing'
import type { On } from 'claude-code'
import type { Engine } from 'claude-code/testing'

import { briefOf, originOf } from '../hooks/guard'

// These prove which edits the write guard lets through and the exact refusal
// for each that it stops. git, the board config and `board` are stood in by
// a process.run hook, and each agent's conversation by a session.messages
// hook. They do not run board: tests/test_brief.py proves the line that
// `changes.brief` starts with, and the plugin's other tests what `get`
// answers for a change.

const BOARD = '/home/me/board'
const REPO = '/work/widgets'
const FILE = `${REPO}/src/lib.rs`
const BRIEF = 'board-brief: /changes/fix-it\n\nYou are the builder for /changes/fix-it.'
const report = (schema: string, data: unknown) => JSON.stringify({ schema, data })
const MOUNTS = report('board/query-report/v2', [
  ['/repos/widgets', REPO],
  ['/repos/gadgets', '/work/gadgets'],
])
const change = (record: object) =>
  report('board/get-report/v2', [{ record: { $type: 'changes/change/v1', title: 't', ...record } }])
const MISSING = JSON.stringify({
  schema: 'board/get-report/v2',
  data: [],
  failures: [{ key: '/changes/fix-it', code: 'board::path::not_found', message: 'nothing is at /changes/fix-it' }],
})
const DEGRADED = JSON.stringify({
  schema: 'board/get-report/v2',
  data: [],
  failures: [{ key: '/changes/fix-it', code: 'board::record::invalid', message: '/changes/fix-it fails its schema' }],
})

type Answers = {
  /** The first message of each conversation, by agent id; '' for the main one. */
  first?: Record<string, string>
  /** What `board get` prints for the change. */
  change?: string
  /** What reading the board config answers. */
  config?: { exitCode: number; stdout: string }
  /** The branch every checkout is on; '' for a detached HEAD, 'main' when left out. */
  branch?: string
}

/** Stands in for git, the board config, `board` and each conversation; `ran` lists the commands. */
function engine(on: On, answers: Answers) {
  const ran: string[] = []
  const edits: string[] = []
  const run = (exitCode: number, stdout: string, stderr = '') => ({
    value: { exitCode, stdout, stderr, isStdoutTruncated: false, isStderrTruncated: false },
  })
  on('process.run', (_, e) => {
    const script = e.argv[2] ?? ''
    const args = e.argv.slice(4)
    if (script.includes('rev-parse')) {
      ran.push('git')
      const path = args[0] ?? ''
      const at = (dir: string, url = '') => run(0, `${dir}/.git\n${answers.branch ?? 'main'}\n${url}${url ? '\n' : ''}`)
      if (path.startsWith(`${REPO}/`)) return at(REPO)
      if (path.startsWith('/work/gadgets/')) return at('/work/gadgets')
      if (path.startsWith('/work/loose/')) return at('/work/loose')
      if (path.startsWith('/work/clone-ssh/')) return at('/work/clone-ssh', 'git@github.com:acme/widgets.git')
      if (path.startsWith('/work/clone-https/')) return at('/work/clone-https', 'https://github.com/Acme/widgets')
      if (path.startsWith('/work/other/')) return at('/work/other', 'ssh://git@github.com/acme/other.git')
      return run(128, '', 'fatal: not a git repository (or any of the parent directories): .git\n')
    }
    if (script.includes('for r')) {
      ran.push(`origins ${args.join(' ')}`)
      const origins: Record<string, string> = { [REPO]: 'git@github.com:acme/widgets.git', '/work/gadgets': 'https://github.com/acme/gadgets.git' }
      return run(0, args.map(repo => `${repo}\t${origins[repo] ?? ''}\n`).join(''))
    }
    if (script.includes('config.json')) {
      ran.push('config')
      const config = answers.config ?? { exitCode: 0, stdout: JSON.stringify({ boards: [BOARD] }) }
      return run(config.exitCode, config.stdout)
    }
    if (script.includes('query')) {
      ran.push(`query ${args[0]}`)
      return run(0, MOUNTS)
    }
    ran.push(`get ${args[0]} ${args[1]}`)
    const printed = answers.change ?? change({ mount: '/repos/widgets', phase: 'build' })
    return run(printed === MISSING || printed === DEGRADED ? 1 : 0, printed)
  })
  on('session.messages', (_, e) => {
    const first = answers.first?.[e.agentId ?? '']
    if (first === undefined) return { value: { deny: `no conversation ${e.agentId}` } }
    return { value: [{ role: 'user', text: first, toolUses: [] }, { role: 'assistant', text: 'ok', toolUses: [] }] }
  })
  on('tool.call', (_, e) => {
    edits.push(e.tool)
    return { result: {} }
  })
  return { ran, edits }
}

const edit = ($: Engine, file = FILE, agentId?: string) =>
  $.tool.call({ tool: 'Edit', file_path: file, old_string: 'a', new_string: 'b', replace_all: false, ...(agentId ? { agentId } : {}) })

test('a brief line is the first line of the first message, naming one change', () => {
  expect(briefOf(BRIEF)).toBe('/changes/fix-it')
  expect(briefOf('\n  board-brief: /changes/fix-it  \nmore')).toBe('/changes/fix-it')
  expect(briefOf('Please fix it.\nboard-brief: /changes/fix-it')).toBeNull()
  expect(briefOf('board-brief: /tasks/fix-it')).toBeNull()
  expect(briefOf('')).toBeNull()
})

test('a subagent started from the brief of a change in build edits its repository', async ($, on) => {
  const { ran, edits } = engine(on, { first: { '': 'Coordinate the work.', a1: BRIEF } })
  const done = await edit($, FILE, 'a1')
  expect(done.deny).toBeUndefined()
  expect(edits).toEqual(['Edit'])
  expect(ran).toEqual(['git', 'config', `query ${BOARD}`, `get ${BOARD} /changes/fix-it`])
})

test('a main session without a brief is refused an edit of a mounted repository', async ($, on) => {
  const { edits } = engine(on, { first: { '': 'Coordinate the work.', a1: BRIEF } })
  const done = await edit($)
  expect(done.deny).toBe(
    `changes::guard::no_brief: ${FILE} is in /repos/widgets, which the board at ${BOARD} mounts. Only an agent ` +
      'whose first message starts with `board-brief: /changes/<name>` may edit it, and this agent\'s first message ' +
      `does not. Start a builder whose first message is the output of \`board -C ${BOARD} call changes.brief --path ` +
      '/changes/<name>`.',
  )
  expect(edits).toEqual([])
})

test('a brief that names a change in plan is refused, and the same brief in build is admitted', async ($, on) => {
  const answers: Answers = { first: { a1: BRIEF }, change: change({ mount: '/repos/widgets', phase: 'plan' }) }
  const { edits } = engine(on, answers)
  const refused = await edit($, FILE, 'a1')
  expect(refused.deny).toBe(
    `changes::guard::not_building: /changes/fix-it is in phase plan, and its builder may edit ${FILE} only in ` +
      `build. Run \`board -C ${BOARD} call changes.advance --path /changes/fix-it\` once its plan gate passes.`,
  )
  answers.change = change({ mount: '/repos/widgets', phase: 'build' })
  expect((await edit($, FILE, 'a1')).deny).toBeUndefined()
  expect(edits).toEqual(['Edit'])
})

test('a brief for a change to another repository is refused, and one for this repository is admitted', async ($, on) => {
  const answers: Answers = { first: { a1: BRIEF }, change: change({ mount: '/repos/gadgets', phase: 'build' }) }
  const { edits } = engine(on, answers)
  const done = await edit($, FILE, 'a1')
  expect(done.deny).toBe(
    `changes::guard::wrong_mount: /changes/fix-it is a change to /repos/gadgets, and ${FILE} is in ` +
      '/repos/widgets. Start a builder from the brief of a change to /repos/widgets.',
  )
  answers.change = change({ mount: '/repos/widgets', phase: 'build' })
  expect((await edit($, FILE, 'a1')).deny).toBeUndefined()
  expect(edits).toEqual(['Edit'])
})

test('a prototype builder is refused off its proto branch and admitted on it', async ($, on) => {
  const prototype = change({ mount: '/repos/widgets', phase: 'build', kind: 'prototype' })
  const answers: Answers = { first: { a1: BRIEF }, change: prototype, branch: 'main' }
  const { edits } = engine(on, answers)
  expect((await edit($, FILE, 'a1')).deny).toBe(
    `changes::guard::wrong_branch: /changes/fix-it is a prototype, and its builder may edit ${FILE} only on ` +
      "branch proto/fix-it, but the file's checkout is on branch main. Run `git switch -c proto/fix-it` there, or " +
      '`git switch proto/fix-it` once it exists.',
  )
  answers.branch = ''
  expect((await edit($, FILE, 'a1')).deny).toMatch(/checkout is on a detached HEAD\./)
  answers.branch = 'proto/fix-it'
  expect((await edit($, FILE, 'a1')).deny).toBeUndefined()
  expect(edits).toEqual(['Edit'])
})

test('a build change is admitted on any branch', async ($, on) => {
  const { edits } = engine(on, { first: { a1: BRIEF }, branch: 'proto/other' })
  expect((await edit($, FILE, 'a1')).deny).toBeUndefined()
  expect(edits).toEqual(['Edit'])
})

test('a brief naming a change no board holds is refused as no brief', async ($, on) => {
  const answers: Answers = { first: { a1: BRIEF }, change: MISSING }
  engine(on, answers)
  const done = await edit($, FILE, 'a1')
  expect(done.deny).toMatch(/^changes::guard::no_brief: .*first message names \/changes\/fix-it, which the board at /)
  answers.change = change({ mount: '/repos/widgets', phase: 'build' })
  expect((await edit($, FILE, 'a1')).deny).toBeUndefined()
})

test('a path outside every mounted repository is admitted without reading the conversation', async ($, on) => {
  const { ran, edits } = engine(on, {})
  for (const file of ['/home/me/board/tasks/a.md', '/work/loose/notes.md', '/tmp/scratch/new/file.txt']) {
    const done = await $.tool.call({ tool: 'Write', file_path: file, content: 'x' })
    expect(done.deny).toBeUndefined()
  }
  expect(edits).toEqual(['Write', 'Write', 'Write'])
  expect(ran).toEqual(['git', 'git', 'config', `query ${BOARD}`, 'git'])
})

test('with no board config no repository is mounted, and the edit runs', async ($, on) => {
  const { edits } = engine(on, { config: { exitCode: 3, stdout: '' } })
  expect((await edit($)).deny).toBeUndefined()
  expect(edits).toEqual(['Edit'])
})

test('a NotebookEdit is guarded like an Edit', async ($, on) => {
  const notebook = `${REPO}/notes/explore.ipynb`
  const call = (agentId?: string) =>
    $.tool.call({ tool: 'NotebookEdit', notebook_path: notebook, new_source: 'x', ...(agentId ? { agentId } : {}) })
  const { edits } = engine(on, { first: { '': 'Coordinate the work.', a1: BRIEF } })
  expect((await call()).deny).toMatch(/^changes::guard::no_brief: /)
  expect((await call('a1')).deny).toBeUndefined()
  expect(edits).toEqual(['NotebookEdit'])
})

test('an unreadable change refuses with unknown and says what failed', async ($, on) => {
  const answers: Answers = { first: { a1: BRIEF }, change: DEGRADED }
  const { edits } = engine(on, answers)
  const done = await edit($, FILE, 'a1')
  expect(done.deny).toBe(
    `changes::guard::unknown: the guard could not tell whether this agent may edit ${FILE}: \`board -C ${BOARD} ` +
      'get /changes/fix-it` failed with board::record::invalid: /changes/fix-it fails its schema. The guard refuses ' +
      'every edit of a mounted repository it cannot check; fix what failed and try again.',
  )
  answers.change = change({ mount: '/repos/widgets', phase: 'build' })
  expect((await edit($, FILE, 'a1')).deny).toBeUndefined()
  expect(edits).toEqual(['Edit'])
})

test('a conversation the session cannot read refuses with unknown, and one it can read is admitted', async ($, on) => {
  const answers: Answers = { first: {} }
  engine(on, answers)
  const done = await edit($, FILE, 'gone')
  expect(done.deny).toMatch(/^changes::guard::unknown: .*no conversation gone/)
  answers.first = { gone: BRIEF }
  expect((await edit($, FILE, 'gone')).deny).toBeUndefined()
})

test('a board config that is not JSON refuses with unknown, and one that is admits the builder', async ($, on) => {
  const answers: Answers = { first: { a1: BRIEF }, config: { exitCode: 0, stdout: 'boards = [' } }
  engine(on, answers)
  expect((await edit($, FILE, 'a1')).deny).toMatch(/^changes::guard::unknown: .*board config/)
  answers.config = { exitCode: 0, stdout: JSON.stringify({ boards: [BOARD] }) }
  expect((await edit($, FILE, 'a1')).deny).toBeUndefined()
})

test('a brief found once is kept for the agent, so a long conversation keeps its builder', async ($, on) => {
  const answers: Answers = { first: { a1: BRIEF } }
  const { edits } = engine(on, answers)
  await edit($, FILE, 'a1')
  answers.first = { a1: 'a later message, once the first left the window' }
  expect((await edit($, FILE, 'a1')).deny).toBeUndefined()
  expect(edits).toEqual(['Edit', 'Edit'])
})

test('a clone whose ssh origin names the mounted repository is guarded like a worktree of it', async ($, on) => {
  const file = '/work/clone-ssh/src/lib.rs'
  const { ran, edits } = engine(on, { first: { '': 'Coordinate the work.', a1: BRIEF } })
  expect((await edit($, file)).deny).toMatch(/^changes::guard::no_brief: \/work\/clone-ssh\/src\/lib.rs is in \/repos\/widgets, /)
  expect((await edit($, file, 'a1')).deny).toBeUndefined()
  expect(edits).toEqual(['Edit'])
  expect(ran.slice(0, 4)).toEqual(['git', 'config', `query ${BOARD}`, `origins ${REPO} /work/gadgets`])
})

test('a clone whose https origin names the mounted repository is guarded like a worktree of it', async ($, on) => {
  const file = '/work/clone-https/src/lib.rs'
  const { edits } = engine(on, { first: { '': 'Coordinate the work.', a1: BRIEF } })
  expect((await edit($, file)).deny).toMatch(/^changes::guard::no_brief: .* is in \/repos\/widgets, /)
  expect((await edit($, file, 'a1')).deny).toBeUndefined()
  expect(edits).toEqual(['Edit'])
})

test('a repository whose origin names no mounted repository is admitted without reading the conversation', async ($, on) => {
  const { ran, edits } = engine(on, {})
  expect((await edit($, '/work/other/src/lib.rs')).deny).toBeUndefined()
  expect(edits).toEqual(['Edit'])
  expect(ran).toEqual(['git', 'config', `query ${BOARD}`, `origins ${REPO} /work/gadgets`])
})

test('origins match across ssh and https forms, case and a trailing .git', () => {
  const same = ['git@github.com:acme/widgets.git', 'ssh://git@github.com/acme/widgets', 'https://github.com/Acme/widgets.git/', 'http://GitHub.com/acme/widgets']
  expect(new Set(same.map(originOf))).toEqual(new Set(['github.com/acme/widgets']))
  expect(originOf('git@github.com:acme/other.git')).toBe('github.com/acme/other')
  expect(originOf('')).toBeNull()
})
