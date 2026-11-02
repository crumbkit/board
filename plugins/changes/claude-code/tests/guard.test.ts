import { expect, test } from 'claude-code/testing'
import type { On } from 'claude-code'
import type { Engine } from 'claude-code/testing'

import { briefOf, localOrigin, originOf } from '../hooks/guard'

// These prove which edits the write guard lets through and the exact refusal
// for each that it stops. git and `board` are stood in by a process.run hook,
// the file system and the board config by fs hooks, and each agent's
// conversation by a session.messages hook. They do not run board:
// tests/test_brief.py proves the line that `changes.brief` starts with, and
// the plugin's other tests what `get` answers for a change.

const HOME = '/home/me'
const BOARD = '/home/me/board'
const BOARD2 = '/home/me/team-board'
const REPO = '/work/widgets'
const FILE = `${REPO}/src/lib.rs`
const BRIEF = 'board-brief: /changes/fix-it\n\nYou are the builder for /changes/fix-it.'
const report = (schema: string, data: unknown) => JSON.stringify({ schema, data })
const MOUNTS = report('board/query-report/v2', [
  ['/repos/widgets', REPO, 'repos/widgets.json'],
  ['/repos/gadgets', '/work/gadgets', 'repos/gadgets.json'],
  ['/repos/broken', null, 'repos/broken.json'],
])
const CONFIG = `${HOME}/.config/board/config.json`
const change = (record: object) =>
  report('board/get-report/v2', [{ record: { $type: 'changes/change/v1', title: 't', ...record } }])
const BUILDING = change({ mount: '/repos/widgets', phase: 'build' })
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

// The files that exist, and every directory above one of them.
const FILES = new Set([
  FILE,
  `${REPO}/README.md`,
  `${REPO}/notes/explore.ipynb`,
  '/home/me/board/tasks/a.md',
  '/work/loose/notes.md',
  '/work/clone-ssh/src/lib.rs',
  '/work/clone-https/src/lib.rs',
  '/work/clone-local/src/lib.rs',
  '/work/clone-file/src/lib.rs',
  '/work/other/src/lib.rs',

  '/work/widgets.worktrees/x/src/lib.rs',
  '/work/dubious/a.rs',
  '/work/throws/a.rs',
  '/tmp/x/.keep',
  CONFIG,
  `${HOME}/.config/board/notes.txt`,
  `${BOARD}/changes/fix-it.json`,
  `${BOARD}/changes/fix-it/plan.md`,
  `${BOARD}/repos/widgets.json`,
  `${BOARD}/repos/README.md`,
])
const DIRS = new Set([...FILES].flatMap(f => f.split('/').slice(1, -1).map((_, i, parts) => `/${parts.slice(0, i + 1).join('/')}`)))
DIRS.add('/')
// Symbolic links: a directory in the home directory, a file and one that leads nowhere.
const LINKS: Record<string, string> = {
  '/home/me/widgets': REPO,
  '/tmp/x/link.md': `${REPO}/README.md`,
  '/tmp/x/dangling.md': '/nowhere/x.md',
}
const resolve = (path: string) => {
  for (const [link, target] of Object.entries(LINKS)) {
    if (path === link || path.startsWith(`${link}/`)) return target + path.slice(link.length)
  }
  return path
}

// What `git rev-parse --git-common-dir` and the origin print in each repository's directory; the
// harness puts the branch line between them.
const REPOSITORIES: [string, string][] = [
  ['/work/widgets.worktrees/x', `${REPO}/.git\ngit@github.com:acme/widgets.git\n`],
  [REPO, `${REPO}/.git\ngit@github.com:acme/widgets.git\n`],
  ['/work/gadgets', '/work/gadgets/.git\n'],
  [BOARD, `${BOARD}/.git\n`],
  ['/work/loose', '/work/loose/.git\n'],
  ['/work/clone-ssh', '/work/clone-ssh/.git\ngit@github.com:acme/widgets.git\n'],
  ['/work/clone-https', '/work/clone-https/.git\nhttps://github.com/Acme/widgets\n'],
  ['/work/clone-local', `/work/clone-local/.git\n${REPO}\n`],
  ['/work/clone-file', `/work/clone-file/.git\nfile://${REPO}/.git\n`],
  ['/work/other', '/work/other/.git\nssh://git@github.com/acme/other.git\n'],
]

type Answers = {
  /** The first message of each conversation, by agent id; '' for the main one. */
  first?: Record<string, string>
  /** How many entries each conversation's window holds; 2 when left out. */
  entries?: number
  /** What `board get` prints for the change, by board; BUILDING when left out. */
  change?: string
  changes?: Record<string, string>
  /** The board config's text; null when there is none. */
  config?: string | null
  /** Whether `board query` fails. */
  queryFails?: boolean
  /** The session's id; s1 when left out. */
  session?: string
  /** The branch every checkout is on; '' for a detached HEAD, 'main' when left out. */
  branch?: string
}

/** Stands in for git, the file system, `board` and each conversation; `ran` lists the commands. */
function engine(on: On, answers: Answers) {
  const ran: string[] = []
  const edits: string[] = []
  const run = (exitCode: number, stdout: string, stderr = '') => ({
    value: { exitCode, stdout, stderr, isStdoutTruncated: false, isStderrTruncated: false },
  })
  const configText = () => (answers.config === undefined ? JSON.stringify({ boards: [BOARD] }) : answers.config)
  on('env.get', (_, e) => ({ value: e.name === 'HOME' ? HOME : undefined }))
  on('fs.exists', (_, e) => ({ value: e.path === `${HOME}/.config/board/config.json` && configText() !== null }))
  on('fs.read', (_, e) => {
    if (e.path !== `${HOME}/.config/board/config.json` || configText() === null) throw new Error(`ENOENT: ${e.path}`)
    ran.push('config')
    return { value: configText()! }
  })
  on('fs.stat', (_, e) => {
    const real = resolve(e.path)
    const isLink = LINKS[e.path] !== undefined
    const kind = FILES.has(real) ? 'file' : DIRS.has(real) ? 'dir' : null
    if (kind === null) {
      if (isLink) return { value: { kind: 'other', size: 0, mtimeMs: 0, isLink } }
      throw new Error(`ENOENT: ${e.path}`)
    }
    return { value: { kind, size: 0, mtimeMs: 0, isLink, ...(e.resolve ? { realPath: real } : {}) } }
  })
  on('process.run', (_, e) => {
    const script = e.argv[2] ?? ''
    const args = e.argv.slice(4)
    if (script.includes('rev-parse')) {
      ran.push('git')
      const dir = args[0] ?? ''
      if (dir.startsWith('/work/throws')) throw new Error('git crashed')
      if (dir.startsWith('/work/dubious')) return run(128, '', "fatal: detected dubious ownership in repository at '/work/dubious'\n")
      const found = REPOSITORIES.find(([root]) => dir === root || dir.startsWith(`${root}/`))
      return found === undefined
        ? run(128, '', 'fatal: not a git repository (or any of the parent directories): .git\n')
        : run(0, found[1].replace('\n', `\n${answers.branch ?? 'main'}\n`))
    }
    if (script.includes('for r')) {
      ran.push(`origins ${args.join(' ')}`)
      const origins: Record<string, string> = { [REPO]: 'git@github.com:acme/widgets.git', '/work/gadgets': 'https://github.com/acme/gadgets.git' }
      return run(0, args.map(repo => `${repo}\t${origins[repo] ?? ''}\n`).join(''))
    }
    if (script.includes('query')) {
      ran.push(`query ${args[0]}`)
      return answers.queryFails ? run(2, '', 'board: the index could not be built') : run(0, MOUNTS)
    }
    ran.push(`get ${args[0]} ${args[1]}`)
    const printed = answers.changes?.[args[0] ?? ''] ?? answers.change ?? BUILDING
    return run(printed === MISSING || printed === DEGRADED ? 1 : 0, printed)
  })
  on('session.id', () => ({ value: answers.session ?? 's1' }))
  on('session.messages', (_, e) => {
    const first = answers.first?.[e.agentId ?? '']
    if (first === undefined) return { value: { deny: `no conversation ${e.agentId}` } }
    const rest = Array.from({ length: (answers.entries ?? 2) - 1 }, () => ({ role: 'assistant' as const, text: 'ok', toolUses: [] }))
    return { value: [{ role: 'user', text: first, toolUses: [] }, ...rest] }
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

test('a main session that started from a brief edits a mounted repository', async ($, on) => {
  // Its own session id, since the guard keeps the brief it finds for each session and agent.
  const { edits } = engine(on, { first: { '': BRIEF }, session: 'started-from-brief' })
  expect((await edit($)).deny).toBeUndefined()
  expect(edits).toEqual(['Edit'])
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

test('a paused change is refused and says to resume it, and the same change in build is admitted', async ($, on) => {
  const answers: Answers = { first: { a1: BRIEF }, change: change({ mount: '/repos/widgets', phase: 'paused' }) }
  engine(on, answers)
  expect((await edit($, FILE, 'a1')).deny).toBe(
    `changes::guard::not_building: /changes/fix-it is paused, and its builder may edit ${FILE} only in build. ` +
      `Run \`board -C ${BOARD} call changes.resume --path /changes/fix-it\` when the cap allows.`,
  )
  answers.change = change({ mount: '/repos/widgets', phase: 'build' })
  expect((await edit($, FILE, 'a1')).deny).toBeUndefined()
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
  expect(ran).toEqual(['git', 'config', `query ${BOARD}`, 'git', 'config', `query ${BOARD}`, 'git'])
})

test('with no board config no repository is mounted, and the edit runs', async ($, on) => {
  const { edits } = engine(on, { config: null })
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
  const answers: Answers = { first: { a1: BRIEF }, config: 'boards = [' }
  engine(on, answers)
  expect((await edit($, FILE, 'a1')).deny).toMatch(/^changes::guard::unknown: .*board config/)
  answers.config = JSON.stringify({ boards: [BOARD] })
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

test('a clone whose origin is a local path or a file URL of the mounted repository is guarded like a worktree', async ($, on) => {
  const { ran, edits } = engine(on, { first: { '': 'Coordinate the work.', a1: BRIEF } })
  for (const file of ['/work/clone-local/src/lib.rs', '/work/clone-file/src/lib.rs']) {
    expect((await edit($, file)).deny).toMatch(/^changes::guard::no_brief: .* is in \/repos\/widgets, /)
    expect((await edit($, file, 'a1')).deny).toBeUndefined()
  }
  expect(edits).toEqual(['Edit', 'Edit'])
  expect(ran.slice(0, 4)).toEqual(['git', 'config', `query ${BOARD}`, 'git'])
})

test('a local origin is a path or a file URL, and an absolute path is kept as it is', () => {
  expect(localOrigin('/work/widgets', '/work/c')).toBe('/work/widgets')
  expect(localOrigin('file:///work/widgets.git', '/work/c')).toBe('/work/widgets.git')
  expect(localOrigin('file://localhost/work/w%20x', '/work/c')).toBe('/work/w x')
  expect(localOrigin('../widgets', '/work/c')).toBe('/work/c/../widgets')
  expect(localOrigin('git@github.com:acme/widgets.git', '/work/c')).toBeNull()
  expect(localOrigin('https://github.com/acme/widgets', '/work/c')).toBeNull()
})

test('a worktree in another directory is guarded as its repository', async ($, on) => {
  const file = '/work/widgets.worktrees/x/src/lib.rs'
  const { ran, edits } = engine(on, { first: { '': 'Coordinate the work.', a1: BRIEF } })
  expect((await edit($, file)).deny).toMatch(/^changes::guard::no_brief: .* is in \/repos\/widgets, /)
  expect((await edit($, file, 'a1')).deny).toBeUndefined()
  expect(edits).toEqual(['Edit'])
  expect(ran.slice(0, 3)).toEqual(['git', 'config', `query ${BOARD}`])
})

test('a ~ path to a mounted repository is guarded, and one outside it is admitted', async ($, on) => {
  const { edits } = engine(on, { first: { '': 'Coordinate the work.' } })
  expect((await edit($, '~/widgets/src/lib.rs')).deny).toMatch(
    /^changes::guard::no_brief: ~\/widgets\/src\/lib.rs is in \/repos\/widgets, /,
  )
  expect((await edit($, '~/board/tasks/a.md')).deny).toBeUndefined()
  expect(edits).toEqual(['Edit'])
})

test('a symbolic link into a mounted repository is guarded, and one that leads nowhere refuses with unknown', async ($, on) => {
  const { edits } = engine(on, { first: { '': 'Coordinate the work.' } })
  expect((await edit($, '/tmp/x/link.md')).deny).toMatch(/^changes::guard::no_brief: \/tmp\/x\/link.md is in \/repos\/widgets, /)
  expect((await edit($, '/tmp/x/dangling.md')).deny).toMatch(/^changes::guard::unknown: .*cannot place/)
  expect((await edit($, '/tmp/x/new.md')).deny).toBeUndefined()
  expect(edits).toEqual(['Edit'])
})

test('a ~user path cannot be placed and refuses with unknown', async ($, on) => {
  engine(on, {})
  expect((await edit($, '~other/widgets/src/lib.rs')).deny).toMatch(/^changes::guard::unknown: .*cannot place/)
})

test('a git failure other than no repository refuses with unknown, and git that throws does too', async ($, on) => {
  const { edits } = engine(on, {})
  expect((await edit($, '/work/dubious/a.rs')).deny).toMatch(/^changes::guard::unknown: .*dubious ownership/)
  expect((await edit($, '/work/throws/a.rs')).deny).toMatch(/^changes::guard::unknown: .*the lookup threw /)
  expect((await edit($, '/work/loose/notes.md')).deny).toBeUndefined()
  expect(edits).toEqual(['Edit'])
})

test('a board query that fails refuses with unknown, and one that lists mounts admits the builder', async ($, on) => {
  const answers: Answers = { first: { a1: BRIEF }, queryFails: true }
  engine(on, answers)
  expect((await edit($, FILE, 'a1')).deny).toMatch(/^changes::guard::unknown: .*did not list its mounts: board: the index/)
  answers.queryFails = false
  expect((await edit($, FILE, 'a1')).deny).toBeUndefined()
})

test('with two boards the change is read from the one that holds it', async ($, on) => {
  const config = JSON.stringify({ boards: [BOARD, BOARD2] })
  const { ran, edits } = engine(on, { first: { a1: BRIEF }, config, changes: { [BOARD]: MISSING, [BOARD2]: BUILDING } })
  expect((await edit($, FILE, 'a1')).deny).toBeUndefined()
  expect(edits).toEqual(['Edit'])
  expect(ran.filter(r => r.startsWith('get'))).toEqual([`get ${BOARD} /changes/fix-it`, `get ${BOARD2} /changes/fix-it`])
})

test('a brief kept for the main conversation does not outlive /clear or /resume', async ($, on) => {
  const answers: Answers = { first: { '': BRIEF }, session: 'before-clear' }
  engine(on, answers)
  expect((await edit($)).deny).toBeUndefined()
  answers.session = 'after-clear'
  answers.first = { '': 'A new conversation.' }
  expect((await edit($)).deny).toMatch(/^changes::guard::no_brief: /)
})

test('a full window with no kept brief refuses with unknown, and a window that holds the first message admits', async ($, on) => {
  const answers: Answers = { first: { w1: BRIEF }, entries: 4096 }
  engine(on, answers)
  expect((await edit($, FILE, 'w1')).deny).toMatch(/^changes::guard::unknown: .*4,096/)
  answers.entries = 4095
  expect((await edit($, FILE, 'w1')).deny).toBeUndefined()
})

test('the board config is refused to every agent, and another file beside it is admitted', async ($, on) => {
  const { edits } = engine(on, { first: { a1: BRIEF } })
  const write = (file: string, agentId?: string) =>
    $.tool.call({ tool: 'Write', file_path: file, content: '{"boards": []}', ...(agentId ? { agentId } : {}) })
  for (const agent of [undefined, 'a1']) {
    expect((await write(CONFIG, agent)).deny).toBe(
      `changes::guard::board_record: ${CONFIG} is the board config, which changes only outside an agent's ` +
        'Edit, Write and NotebookEdit. Ask the owner to change it.',
    )
  }
  expect((await write('~/.config/board/config.json')).deny).toMatch(/^changes::guard::board_record: /)
  expect((await write(`${HOME}/.config/board/notes.txt`)).deny).toBeUndefined()
  expect(edits).toEqual(['Write'])
})

test('a change record is refused to every agent, and its plan is admitted', async ($, on) => {
  const { edits } = engine(on, { first: { a1: BRIEF } })
  for (const agent of [undefined, 'a1']) {
    expect((await edit($, `${BOARD}/changes/fix-it.json`, agent)).deny).toBe(
      `changes::guard::board_record: ${BOARD}/changes/fix-it.json is a change record of the board at ${BOARD}, ` +
        `which changes only through board commands. Use \`board -C ${BOARD} call changes.start\`, \`changes.attach\` ` +
        'or `changes.advance`, or `board put` to drop a change.',
    )
  }
  expect((await edit($, `${BOARD}/changes/new.json`)).deny).toMatch(/^changes::guard::board_record: /)
  expect((await edit($, `${BOARD}/changes/fix-it/plan.md`)).deny).toBeUndefined()
  expect(edits).toEqual(['Edit'])
})

test('a mount record is refused to every agent, and another file beside it is admitted', async ($, on) => {
  const { edits } = engine(on, { first: { a1: BRIEF } })
  for (const agent of [undefined, 'a1']) {
    expect((await edit($, `${BOARD}/repos/widgets.json`, agent)).deny).toBe(
      `changes::guard::board_record: ${BOARD}/repos/widgets.json is the mount record of /repos/widgets in the ` +
        `board at ${BOARD}, which changes only through board commands. Use \`board -C ${BOARD} mount update ` +
        '/repos/widgets` to follow its repository, or `board put` to change the mount.',
    )
  }
  expect((await edit($, `${BOARD}/repos/broken.json`)).deny).toMatch(/^changes::guard::board_record: .*\/repos\/broken /)
  expect((await edit($, `${BOARD}/repos/README.md`)).deny).toBeUndefined()
  expect(edits).toEqual(['Edit'])
})
