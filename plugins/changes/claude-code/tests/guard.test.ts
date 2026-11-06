import { expect, test } from 'claude-code/testing'
import type { On } from 'claude-code'
import type { Engine } from 'claude-code/testing'

import { briefOf, localOrigin, originOf, sameDirectory } from '../hooks/guard'

// These prove which edits the write guard lets through and the exact refusal
// for each that it stops. git and `board` are stood in by a process.run hook,
// the file system and the board config by fs hooks, and each agent's
// conversation by a session.messages hook. They do not run board:
// tests/test_brief.py proves the line that `changes.brief` starts with, and
// the plugin's other tests what `get` answers for a change.

const HOME = '/home/me'
const BOARD = '/home/me/board'
const BOARD2 = '/home/me/team-board'
// A second board nested in the personal board's directory, as its own git repository.
const SECOND = '/home/me/board/second'
// A worktree of the personal board's repository.
const BOARD_WORKTREE = '/home/me/board.worktrees/x'
const HANDOFF = `${BOARD}/projects/handoff.md`
const REPO = '/work/widgets'
const FILE = `${REPO}/src/lib.rs`
const BRIEF = 'board-brief: /changes/fix-it\n\nYou are the builder for /changes/fix-it.'
const report = (schema: string, data: unknown) => JSON.stringify({ schema, data })
const MOUNTS = report('board/query-report/v2', [
  ['/repos/widgets', REPO, 'repos/widgets.json'],
  ['/repos/gadgets', '/work/gadgets', 'repos/gadgets.json'],
  ['/repos/broken', null, 'repos/broken.json'],
])
// The second board's mounts: it mounts the personal board.
const SECOND_MOUNTS = report('board/query-report/v2', [['/personal', BOARD, 'personal.json']])
// The personal board's mounts when it also mounts the second board.
const MOUNTS_WITH_SECOND = report('board/query-report/v2', [
  ['/repos/widgets', REPO, 'repos/widgets.json'],
  ['/repos/second', SECOND, 'repos/second.json'],
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
const notFound = (key: string) =>
  JSON.stringify({
    schema: 'board/get-report/v2',
    data: [],
    failures: [{ key, code: 'board::path::not_found', message: `nothing is at ${key}` }],
  })
const PROTOTYPE = report('board/get-report/v2', [
  { record: { $type: 'changes/prototype/v1', question: 'Does a step list read better?' } },
])
// What Board prints for a prototype record that fails its schema: `get`
// prints it as it prints a valid one and exits 0, and `check --healthy`
// reports it degraded and exits 1.
const DEGRADED_PROTOTYPE = report('board/get-report/v2', [
  { record: { $type: 'changes/prototype/v1', question: 'Does a step list read better?', unknown: true } },
])
const checked = (entry: object) =>
  JSON.stringify({
    schema: 'board/check-report/v2',
    data: [{ kind: 'record', logical: '/changes/fix-it/prototype', path: 'changes/fix-it/prototype.json', source: 'worktree', ...entry }],
    total: 1,
    truncated: false,
  })
const HEALTHY = checked({ status: 'ok' })
const DEGRADED_CHECK = checked({
  status: 'degraded',
  code: 'board::record::invalid',
  reason: "fails its schema at its root: Additional properties are not allowed ('unknown' was unexpected)",
})
// A record of another type at <change>/prototype.
const OTHER_TYPE = report('board/get-report/v2', [{ record: { $type: 'tasks/task/v2', title: 'x' } }])
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
  `${BOARD}/changes/new.json`,
  `${BOARD}/changes/fix-it/prototype.json`,
  `${BOARD}/changes/fix-it/plans/20261105T120000Z.json`,
  `${BOARD}/changes/fix-it/plans/20261105T120000Z.md`,
  `${BOARD}/repos/widgets.json`,
  `${BOARD}/repos/README.md`,
  HANDOFF,
  `${SECOND}/notes.md`,
  `${BOARD_WORKTREE}/projects/handoff.md`,
  `${BOARD_WORKTREE}/changes/fix-it.json`,
  `${BOARD_WORKTREE}/changes/fix-it/plans/20261105T120000Z.json`,
  `${BOARD_WORKTREE}/repos/widgets.json`,
  `${BOARD_WORKTREE}/repos/extra.json`,
  `${BOARD}/repos/extra.json`,
  `${BOARD}/.git/description`,
])
const DIRS = new Set([...FILES].flatMap(f => f.split('/').slice(1, -1).map((_, i, parts) => `/${parts.slice(0, i + 1).join('/')}`)))
DIRS.add('/')
// Symbolic links: a directory in the home directory, a file and one that leads nowhere.
const LINKS: Record<string, string> = {
  '/home/me/widgets': REPO,
  '/tmp/x/link.md': `${REPO}/README.md`,
  '/tmp/x/dangling.md': '/nowhere/x.md',
  '/home/me/board-link': BOARD,
}
const resolve = (path: string) => {
  for (const [link, target] of Object.entries(LINKS)) {
    if (path === link || path.startsWith(`${link}/`)) return target + path.slice(link.length)
  }
  return path
}

// What `git rev-parse --git-common-dir` and the origin print in each repository's directory; the
// harness puts the checkout's top level (the entry's directory) and the branch line between them.
const REPOSITORIES: [string, string][] = [
  // Inside a repository's git directory, git prints no top level.
  [`${BOARD}/.git`, `${BOARD}/.git\n`],
  ['/work/widgets.worktrees/x', `${REPO}/.git\ngit@github.com:acme/widgets.git\n`],
  [REPO, `${REPO}/.git\ngit@github.com:acme/widgets.git\n`],
  ['/work/gadgets', '/work/gadgets/.git\n'],
  [SECOND, `${SECOND}/.git\n`],
  [BOARD_WORKTREE, `${BOARD}/.git\n`],
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
  /** What `board get` prints for the change's `<change>/prototype`; nothing there when left out. */
  prototype?: string
  /** What `board check --healthy` prints for `<change>/prototype`; HEALTHY when left out. */
  checked?: string
  /** What `board get` prints for the change, by board; BUILDING when left out. */
  change?: string
  changes?: Record<string, string>
  /** The board config's text; null when there is none. */
  config?: string | null
  /** What `board query` prints for each board's mounts, by board; MOUNTS when left out. */
  mounts?: Record<string, string>
  /** Whether `board query` fails. */
  queryFails?: boolean
  /** The one directory where `board query` fails. */
  queryFailsIn?: string
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
        : run(0, found[1].replace('\n', `\n${found[0].endsWith('/.git') ? '' : found[0]}\n${answers.branch ?? 'main'}\n`))
    }
    if (script.includes('for r')) {
      ran.push(`origins ${args.join(' ')}`)
      const origins: Record<string, string> = { [REPO]: 'git@github.com:acme/widgets.git', '/work/gadgets': 'https://github.com/acme/gadgets.git' }
      return run(0, args.map(repo => `${repo}\t${origins[repo] ?? ''}\n`).join(''))
    }
    if (script.includes('query')) {
      ran.push(`query ${args[0]}`)
      if (answers.queryFails || answers.queryFailsIn === args[0]) return run(2, '', 'board: the index could not be built')
      return run(0, answers.mounts?.[args[0] ?? ''] ?? MOUNTS)
    }
    if (script.includes('check --healthy')) {
      ran.push(`check ${args[0]} ${args[1]}`)
      const health = answers.checked ?? HEALTHY
      return run(health === HEALTHY ? 0 : 1, health)
    }
    ran.push(`get ${args[0]} ${args[1]}`)
    if (args[1]?.endsWith('/prototype')) {
      const held = answers.prototype ?? notFound(args[1])
      return run(answers.prototype === undefined ? 1 : 0, held)
    }
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
  expect(ran).toEqual([
    'git',
    'config',
    `query ${BOARD}`,
    `get ${BOARD} /changes/fix-it`,
    `get ${BOARD} /changes/fix-it/prototype`,
  ])
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
  // A change is a prototype when a prototype record sits at <change>/prototype.
  const answers: Answers = { first: { a1: BRIEF }, prototype: PROTOTYPE, branch: 'main' }
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

test('a prototype record Board does not read refuses the edit, and with none there the change builds', async ($, on) => {
  const answers: Answers = {
    first: { a1: BRIEF },
    prototype: DEGRADED_PROTOTYPE,
    checked: DEGRADED_CHECK,
    branch: 'proto/fix-it',
  }
  const { ran, edits } = engine(on, answers)
  expect((await edit($, FILE, 'a1')).deny).toMatch(
    /`board -C \/home\/me\/board check --healthy \/changes\/fix-it\/prototype` failed with board::record::invalid: fails its schema/,
  )
  expect(ran).toContain(`check ${BOARD} /changes/fix-it/prototype`)
  // The same record when Board reads it admits the builder on its branch.
  delete answers.checked
  answers.prototype = PROTOTYPE
  expect((await edit($, FILE, 'a1')).deny).toBeUndefined()
  // A record of another type there is refused too.
  answers.prototype = OTHER_TYPE
  expect((await edit($, FILE, 'a1')).deny).toMatch(/failed with it holds a tasks\/task\/v2, not a changes\/prototype\/v1/)
  answers.branch = 'proto/other'
  delete answers.prototype
  expect((await edit($, FILE, 'a1')).deny).toBeUndefined()
  expect(edits).toEqual(['Edit', 'Edit'])
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
  expect(ran.filter(r => r.startsWith('get'))).toEqual([
    `get ${BOARD} /changes/fix-it`,
    `get ${BOARD2} /changes/fix-it`,
    `get ${BOARD2} /changes/fix-it/prototype`,
  ])
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

test("a change's records are refused to every agent, and its plan's text is admitted", async ($, on) => {
  const { edits } = engine(on, { first: { a1: BRIEF } })
  for (const agent of [undefined, 'a1']) {
    expect((await edit($, `${BOARD}/changes/fix-it.json`, agent)).deny).toBe(
      `changes::guard::board_record: ${BOARD}/changes/fix-it.json is a change record of the board at ${BOARD}, ` +
        `which changes only through board commands. Use \`board -C ${BOARD} call changes.start\`, \`changes.attach\` ` +
        'or `changes.advance`, or `board put` to drop a change.',
    )
  }
  for (const record of ['new.json', 'fix-it/prototype.json', 'fix-it/plans/20261105T120000Z.json']) {
    expect((await edit($, `${BOARD}/changes/${record}`)).deny).toMatch(/^changes::guard::board_record: /)
  }
  expect((await edit($, `${BOARD}/changes/fix-it/plans/20261105T120000Z.md`)).deny).toBeUndefined()
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

test('two directories are the same when they are equal whole, ignoring trailing slashes', () => {
  expect(sameDirectory(BOARD, BOARD)).toBe(true)
  expect(sameDirectory(BOARD, `${BOARD}/`)).toBe(true)
  expect(sameDirectory(`${BOARD}//`, BOARD)).toBe(true)
  expect(sameDirectory('/', '/')).toBe(true)
  // A board inside a repository does not exempt the repository, nor a board the repository is inside.
  expect(sameDirectory(BOARD, SECOND)).toBe(false)
  expect(sameDirectory(SECOND, BOARD)).toBe(false)
  expect(sameDirectory(REPO, BOARD)).toBe(false)
})

test('a known board that another known board mounts is admitted without a brief, and guarded when not listed', async ($, on) => {
  const answers: Answers = {
    first: { '': 'Coordinate the work.' },
    config: JSON.stringify({ boards: [BOARD, SECOND] }),
    mounts: { [SECOND]: SECOND_MOUNTS },
  }
  const { ran, edits } = engine(on, answers)
  expect((await edit($, HANDOFF)).deny).toBeUndefined()
  expect(ran).toEqual(['git', 'config', `query ${BOARD}`, `query ${SECOND}`])
  answers.config = JSON.stringify({ boards: [SECOND] })
  expect((await edit($, HANDOFF)).deny).toBe(
    `changes::guard::no_brief: ${HANDOFF} is in /personal, which the board at ${SECOND} mounts. Only an agent ` +
      'whose first message starts with `board-brief: /changes/<name>` may edit it, and this agent\'s first message ' +
      `does not. Start a builder whose first message is the output of \`board -C ${SECOND} call changes.brief ` +
      '--path /changes/<name>`.',
  )
  expect(edits).toEqual(['Edit'])
})

test('a mounted known board still refuses its change, plan and mount records', async ($, on) => {
  const { edits } = engine(on, {
    first: { '': 'Coordinate the work.' },
    config: JSON.stringify({ boards: [BOARD, SECOND] }),
    mounts: { [SECOND]: SECOND_MOUNTS },
  })
  expect((await edit($, `${BOARD}/changes/fix-it.json`)).deny).toBe(
    `changes::guard::board_record: ${BOARD}/changes/fix-it.json is a change record of the board at ${BOARD}, ` +
      `which changes only through board commands. Use \`board -C ${BOARD} call changes.start\`, \`changes.attach\` ` +
      'or `changes.advance`, or `board put` to drop a change.',
  )
  expect((await edit($, `${BOARD}/changes/fix-it/plans/20261105T120000Z.json`)).deny).toMatch(
    /^changes::guard::board_record: .* is a change record of the board at \/home\/me\/board,/,
  )
  expect((await edit($, `${BOARD}/repos/widgets.json`)).deny).toMatch(
    /^changes::guard::board_record: .* is the mount record of \/repos\/widgets in the board at \/home\/me\/board,/,
  )
  expect((await edit($, HANDOFF)).deny).toBeUndefined()
  expect(edits).toEqual(['Edit'])
})

test('a nested known board is its own repository, admitted when listed and guarded when only mounted', async ($, on) => {
  const file = `${SECOND}/notes.md`
  const answers: Answers = {
    first: { '': 'Coordinate the work.' },
    config: JSON.stringify({ boards: [BOARD, SECOND] }),
    mounts: { [BOARD]: MOUNTS_WITH_SECOND, [SECOND]: SECOND_MOUNTS },
  }
  const { edits } = engine(on, answers)
  expect((await edit($, file)).deny).toBeUndefined()
  answers.config = JSON.stringify({ boards: [BOARD] })
  expect((await edit($, file)).deny).toMatch(/^changes::guard::no_brief: \/home\/me\/board\/second\/notes.md is in \/repos\/second, /)
  expect(edits).toEqual(['Edit'])
})

test('a code repository a board mounts stays guarded when a known board is mounted too', async ($, on) => {
  const answers: Answers = {
    first: { '': 'Coordinate the work.', a1: BRIEF },
    config: JSON.stringify({ boards: [BOARD, SECOND] }),
    mounts: { [SECOND]: SECOND_MOUNTS },
  }
  const { edits } = engine(on, answers)
  expect((await edit($)).deny).toMatch(/^changes::guard::no_brief: \/work\/widgets\/src\/lib.rs is in \/repos\/widgets, /)
  expect((await edit($, FILE, 'a1')).deny).toBeUndefined()
  expect(edits).toEqual(['Edit'])
})

test('a worktree of a known board is admitted like the board, and guarded when the board is not listed', async ($, on) => {
  const file = `${BOARD_WORKTREE}/projects/handoff.md`
  const answers: Answers = {
    first: { '': 'Coordinate the work.' },
    config: JSON.stringify({ boards: [BOARD, SECOND] }),
    mounts: { [SECOND]: SECOND_MOUNTS },
  }
  const { edits } = engine(on, answers)
  expect((await edit($, file)).deny).toBeUndefined()
  answers.config = JSON.stringify({ boards: [SECOND] })
  expect((await edit($, file)).deny).toMatch(/^changes::guard::no_brief: .* is in \/personal, which the board at \/home\/me\/board\/second mounts\./)
  expect(edits).toEqual(['Edit'])
})

test("a linked worktree of a known board still refuses its change and mount records, and admits its other files", async ($, on) => {
  const { edits } = engine(on, {
    first: { '': 'Coordinate the work.', a1: BRIEF },
    config: JSON.stringify({ boards: [BOARD, SECOND] }),
    mounts: { [SECOND]: SECOND_MOUNTS },
  })
  for (const agent of [undefined, 'a1']) {
    expect((await edit($, `${BOARD_WORKTREE}/changes/fix-it.json`, agent)).deny).toBe(
      `changes::guard::board_record: ${BOARD_WORKTREE}/changes/fix-it.json is a change record of the board at ` +
        `${BOARD}, which changes only through board commands. Use \`board -C ${BOARD} call changes.start\`, ` +
        '`changes.attach` or `changes.advance`, or `board put` to drop a change.',
    )
    expect((await edit($, `${BOARD_WORKTREE}/repos/widgets.json`, agent)).deny).toBe(
      `changes::guard::board_record: ${BOARD_WORKTREE}/repos/widgets.json is the mount record of /repos/widgets ` +
        `in the board at ${BOARD_WORKTREE}, which changes only through board commands. Use \`board -C ` +
        `${BOARD_WORKTREE} mount update /repos/widgets\` to follow its repository, or \`board put\` to change the mount.`,
    )
  }
  expect((await edit($, `${BOARD_WORKTREE}/changes/fix-it/plans/20261105T120000Z.json`)).deny).toMatch(
    /^changes::guard::board_record: .* is a change record of the board at \/home\/me\/board,/,
  )
  expect((await edit($, `${BOARD_WORKTREE}/projects/handoff.md`)).deny).toBeUndefined()
  expect(edits).toEqual(['Edit'])
})

test('a known board listed through a symbolic link is exempt, and its records are still refused', async ($, on) => {
  const link = '/home/me/board-link'
  const answers: Answers = {
    first: { '': 'Coordinate the work.' },
    config: JSON.stringify({ boards: [link, SECOND] }),
    mounts: { [SECOND]: SECOND_MOUNTS },
  }
  const { edits } = engine(on, answers)
  expect((await edit($, HANDOFF)).deny).toBeUndefined()
  expect((await edit($, `${link}/projects/handoff.md`)).deny).toBeUndefined()
  expect((await edit($, `${link}/changes/fix-it.json`)).deny).toBe(
    `changes::guard::board_record: ${link}/changes/fix-it.json is a change record of the board at ${link}, ` +
      `which changes only through board commands. Use \`board -C ${link} call changes.start\`, \`changes.attach\` ` +
      'or `changes.advance`, or `board put` to drop a change.',
  )
  answers.config = JSON.stringify({ boards: [SECOND] })
  expect((await edit($, `${link}/projects/handoff.md`)).deny).toMatch(/^changes::guard::no_brief: .* is in \/personal, /)
  expect(edits).toEqual(['Edit', 'Edit'])
})

// The worktree's branch holds a mount record the main checkout does not.
const WORKTREE_MOUNTS = report('board/query-report/v2', [
  ['/repos/widgets', REPO, 'repos/widgets.json'],
  ['/repos/extra', '/work/extra', 'repos/extra.json'],
])

test("a mount record only a linked worktree's branch holds is refused there, and the same path in the main checkout is admitted", async ($, on) => {
  const { ran, edits } = engine(on, {
    first: { '': 'Coordinate the work.' },
    config: JSON.stringify({ boards: [BOARD, SECOND] }),
    mounts: { [SECOND]: SECOND_MOUNTS, [BOARD_WORKTREE]: WORKTREE_MOUNTS },
  })
  expect((await edit($, `${BOARD_WORKTREE}/repos/extra.json`)).deny).toBe(
    `changes::guard::board_record: ${BOARD_WORKTREE}/repos/extra.json is the mount record of /repos/extra in the ` +
      `board at ${BOARD_WORKTREE}, which changes only through board commands. Use \`board -C ${BOARD_WORKTREE} mount ` +
      'update /repos/extra` to follow its repository, or `board put` to change the mount.',
  )
  expect(ran).toContain(`query ${BOARD_WORKTREE}`)
  expect((await edit($, `${BOARD}/repos/extra.json`)).deny).toBeUndefined()
  expect(edits).toEqual(['Edit'])
})

test("a linked worktree's mount query that fails refuses with unknown", async ($, on) => {
  const answers: Answers = { first: { '': 'Coordinate the work.' }, config: JSON.stringify({ boards: [BOARD] }) }
  engine(on, answers)
  answers.queryFailsIn = BOARD_WORKTREE
  expect((await edit($, `${BOARD_WORKTREE}/projects/handoff.md`)).deny).toMatch(
    /^changes::guard::unknown: .*`board -C \/home\/me\/board.worktrees\/x query` did not list its mounts/,
  )
  delete answers.queryFailsIn
  expect((await edit($, `${BOARD_WORKTREE}/projects/handoff.md`)).deny).toBeUndefined()
})

test("a file in a known board's git directory is refused and says so, and a file in its checkout is admitted", async ($, on) => {
  const { edits } = engine(on, { first: { '': 'Coordinate the work.' }, config: JSON.stringify({ boards: [BOARD] }) })
  expect((await edit($, `${BOARD}/.git/description`)).deny).toBe(
    `changes::guard::unknown: the guard could not tell whether this agent may edit ${BOARD}/.git/description: ` +
      `${BOARD}/.git/description is inside the git directory of the board at ${BOARD}, which agents do not edit. ` +
      'The guard refuses every edit of a mounted repository it cannot check; fix what failed and try again.',
  )
  expect((await edit($, HANDOFF)).deny).toBeUndefined()
  expect(edits).toEqual(['Edit'])
})

test('records in a known board are matched without regard to letter case', async ($, on) => {
  const { edits } = engine(on, { first: { '': 'Coordinate the work.' }, config: JSON.stringify({ boards: [BOARD] }) })
  expect((await edit($, `${BOARD_WORKTREE}/Changes/fix-it.json`)).deny).toMatch(
    /^changes::guard::board_record: .*\/Changes\/fix-it.json is a change record of the board at \/home\/me\/board,/,
  )
  expect((await edit($, `${BOARD_WORKTREE}/Repos/Widgets.json`)).deny).toMatch(
    /^changes::guard::board_record: .* is the mount record of \/repos\/widgets /,
  )
  expect((await edit($, `${BOARD_WORKTREE}/Projects/handoff.md`)).deny).toBeUndefined()
  expect(edits).toEqual(['Edit'])
})

test('a listed directory that is a linked worktree exempts nothing, and listing the main checkout does', async ($, on) => {
  const file = `${BOARD_WORKTREE}/projects/handoff.md`
  const answers: Answers = {
    first: { '': 'Coordinate the work.' },
    config: JSON.stringify({ boards: [BOARD_WORKTREE, SECOND] }),
    mounts: { [SECOND]: SECOND_MOUNTS },
  }
  const { edits } = engine(on, answers)
  expect((await edit($, file)).deny).toMatch(/^changes::guard::no_brief: .* is in \/personal, which the board at \/home\/me\/board\/second mounts\./)
  answers.config = JSON.stringify({ boards: [BOARD, SECOND] })
  expect((await edit($, file)).deny).toBeUndefined()
  expect(edits).toEqual(['Edit'])
})
