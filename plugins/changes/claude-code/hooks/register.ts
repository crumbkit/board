import type { EngineInterface, Register } from 'claude-code'

import {
  MOUNTS_SQL,
  briefOf,
  changeRecord,
  configRecord,
  isChangeRecord,
  mountRecord,
  judge,
  localOrigin,
  noBrief,
  originOf,
  parseBoards,
  parseChange,
  parseHealth,
  parseKind,
  parseMounts,
  parseOrigins,
  repositoryOf,
  sameDirectory,
  unknown,
} from './guard'
import type { Mount } from './guard'

// The write guard. Before an Edit, Write or NotebookEdit of a file in a
// repository that a known board mounts, it reads the acting agent's first
// message and admits the call only when that message starts with
// `board-brief: /changes/<name>` for a change in build to that repository,
// and, for a prototype, only on its branch `proto/<name>`.
// Every other file is admitted untouched, except the board config and each
// known board's change and mount records, which change only through board
// commands and are refused to every agent. A repository whose directory is
// listed in `boards` of the board config is not guarded as a mounted
// repository, even when another known board mounts it, since a board holds
// records and no code. The guard learns this from the config. It does not
// test for a board root marker, because a code repository can commit a board
// of its own: Board's own source holds `.board/board.json`. A lookup that
// fails refuses: a guard that admits what it cannot check would be optional.
// There is no bypass.
//
// Soft limit: the guard sees only these three tools. A file written through
// Bash (`sed -i`, a heredoc, a script) passes it. The review gate covers that
// gap: the reviewer checks that every commit of a change came from the
// change's builder sessions. A Bash guard would need a command parser.
//
// Soft limit: the guard checks the brief line, not that `changes.brief`
// produced it, so a parent agent can type the line itself. The review gate
// covers that gap too.

const PATH = '"$HOME/.local/bin:/opt/homebrew/bin:/usr/local/bin:$PATH"'
// Run in a directory, it prints the repository's common directory, the
// checkout's top level (an empty line when it has none, as in a bare
// repository), the checkout's branch (an empty line when HEAD is detached),
// then its origin URL, if any.
const LOCATE =
  'cd "$1" && git rev-parse --path-format=absolute --git-common-dir && ' +
  '{ git rev-parse --path-format=absolute --show-toplevel 2>/dev/null || echo; } && ' +
  '{ git symbolic-ref --short -q HEAD || echo; } && { git config --get remote.origin.url || true; }'
// Each mounted repository's origin URL, `<repo>\t<url>` per line; empty when it has none.
const ORIGINS = 'for r; do printf \'%s\\t%s\\n\' "$r" "$(git -C "$r" config --get remote.origin.url 2>/dev/null)"; done'
const MOUNTS = `PATH=${PATH} exec board -C "$1" --json query --all "$2"`
const GET = `PATH=${PATH} exec board -C "$1" --json get "$2"`
const CHECK = `PATH=${PATH} exec board -C "$1" --json check --healthy "$2"`

// A slow or hung git or board holds the edit no longer than this, then refuses it.
const TIMEOUT_MS = 5000
// `$.session.messages` returns at most this many entries, the newest.
const WINDOW = 4096

// The change each agent's brief named, by session id and agent id ('' for the
// main conversation), so `/clear` and `/resume` start over. The message window
// holds the newest 4096 entries, and a compaction replaces the first message,
// so a brief found once is kept. Soft limit: an agent whose first guarded edit
// comes after its first message left that window is refused; the cache starts
// over when the module reloads. Reading the transcript file would replace it.
const briefs = new Map<string, string>()

type Ran = { exitCode: number; stdout: string; stderr: string }
const sh = ($: EngineInterface, script: string, args: string[]): Promise<Ran> =>
  $.process.run(['/bin/sh', '-c', script, 'guard', ...args], { timeoutMs: TIMEOUT_MS })

const said = (ran: Ran) => (ran.stderr.trim() || ran.stdout.trim()).slice(0, 500) || `exit ${ran.exitCode}`

class Unknown extends Error {}

/** The parent of a path spelled with `/`: `/` for a top-level path, `.` for a bare name. */
const parentOf = (path: string) => {
  const cut = path.replace(/\/+$/, '').lastIndexOf('/')
  return cut < 0 ? '.' : cut === 0 ? '/' : path.slice(0, cut)
}

/**
 * Where `file` lands, every symbolic link followed: its real path and the real
 * directory that holds it. A file that does not exist yet is placed by its
 * nearest directory that does, and a `~/` path is taken from the home
 * directory. Throws when the path cannot be placed: a `~user` path, or a
 * symbolic link that leads nowhere.
 */
async function place($: EngineInterface, file: string): Promise<{ real: string; dir: string }> {
  let path = file
  if (path === '~' || path.startsWith('~/')) {
    const home = await $.env.get('HOME')
    if (home === undefined) throw new Unknown(`it cannot place ${file}, since HOME is unset`)
    path = home + path.slice(1)
  } else if (path.startsWith('~')) {
    throw new Unknown(`it cannot place ${file}`)
  }
  let tail = ''
  for (let at = path; ; ) {
    const stat = await $.fs.stat(at, { resolve: true }).catch(() => undefined)
    if (stat !== undefined) {
      if (stat.realPath === undefined) throw new Unknown(`it cannot place ${file}, since ${at} leads nowhere`)
      if (tail !== '') return { real: `${stat.realPath.replace(/\/$/, '')}/${tail}`, dir: stat.realPath }
      return { real: stat.realPath, dir: stat.kind === 'dir' ? stat.realPath : parentOf(stat.realPath) }
    }
    if (at === '/' || at === '.') throw new Unknown(`it cannot place ${file}`)
    const name = at.replace(/\/+$/, '').slice(parentOf(at).length).replace(/^\//, '')
    tail = tail === '' ? name : `${name}/${tail}`
    at = parentOf(at)
  }
}

/** The real path of `path`, or `path` itself when it leads nowhere. */
const realOf = async ($: EngineInterface, path: string) =>
  (await $.fs.stat(path, { resolve: true }).catch(() => undefined))?.realPath ?? path

type Located = { repo: string; top: string; branch: string; url: string }

/** The repository's own directory, checkout top level, branch and origin URL, from `dir`; null outside a repository. */
async function locate($: EngineInterface, dir: string): Promise<Located | null> {
  const ran = await sh($, LOCATE, [dir])
  if (ran.exitCode !== 0) {
    if (/not a git repository/i.test(ran.stderr)) return null
    throw new Unknown(`git could not find the repository in ${dir}: ${said(ran)}`)
  }
  const [commonDir = '', top = '', branch = '', url = ''] = ran.stdout.split('\n')
  return { repo: repositoryOf(commonDir), top, branch, url }
}

/** The board config's path. */
const configPath = async ($: EngineInterface) =>
  `${(await $.env.get('XDG_CONFIG_HOME')) || `${await $.env.get('HOME')}/.config`}/board/config.json`

/** The known boards' directories; none when there is no board config. */
async function knownBoards($: EngineInterface): Promise<string[]> {
  const path = await configPath($)
  if (!(await $.fs.exists(path))) return []
  const boards = parseBoards(await $.fs.read(path))
  if (!Array.isArray(boards)) throw new Unknown(boards.why)
  return boards
}

/** A board's own mounts, from `board -C <dir> query`; throws when it lists none. */
async function mountsIn($: EngineInterface, dir: string): Promise<Mount[]> {
  const ran = await sh($, MOUNTS, [dir, MOUNTS_SQL])
  const mounts = ran.exitCode === 0 ? parseMounts(dir, ran.stdout) : null
  if (mounts === null) throw new Unknown(`\`board -C ${dir} query\` did not list its mounts: ${said(ran)}`)
  return mounts
}

/**
 * The refusal of a board record in a checkout of `own`, a known board's
 * repository, or null. The checkout may be the board's directory or a
 * linked worktree of it, so the path is taken from the checkout's top level
 * that git prints, and compared without regard to letter case, since a
 * file system that ignores case can place one file under either spelling.
 * A linked worktree's branch can hold mount records the board's directory
 * does not, so its mount records come from a query in the worktree, and the
 * refusal names the worktree, where `board mount update` changes them.
 */
async function ownRecord(
  $: EngineInterface,
  file: string,
  real: string,
  found: Located,
  own: { board: string; real: string; mounts: Mount[] },
): Promise<string | null> {
  if (found.top === '') {
    throw new Unknown(`${file} is inside the git directory of the board at ${own.board}, which agents do not edit`)
  }
  const checkout = await realOf($, found.top)
  const top = `${checkout.replace(/\/$/, '')}/`.toLowerCase()
  const lower = real.toLowerCase()
  if (!lower.startsWith(top)) {
    throw new Unknown(`${file} is not inside its checkout ${found.top} of the board at ${own.board}`)
  }
  const relative = lower.slice(top.length)
  if (isChangeRecord(relative)) return changeRecord(file, own.board)
  const mounts = sameDirectory(checkout, own.real) ? own.mounts : await mountsIn($, found.top)
  const record = mounts.find(m => m.record.toLowerCase() === relative)
  return record === undefined ? null : mountRecord(file, record)
}

/**
 * The known boards, and their mounts whose repository is the file's: those it
 * is a worktree of, and `byOrigin`, read only when asked, those its origin
 * names.
 */
type Held = { boards: string[]; worktree: Mount[]; byOrigin: () => Promise<Mount[]> }

/**
 * The mounts whose repository is `file`'s, across every known board, the
 * refusal of a board record, or null for a file outside the guard. A file in
 * a checkout of a known board's own repository, the board's directory or a
 * linked worktree of it, is outside it once its board records are checked. A
 * mount's repository is the file's when the file's repository is a worktree
 * of the mount's `repo`, when its origin is a local path or `file://` URL of
 * a worktree of it, or when both repositories' origins name one repository.
 * So a separate clone is guarded too. A repository with no origin that is no
 * worktree of a mount stays outside the guard.
 */
async function held($: EngineInterface, file: string): Promise<Held | string | null> {
  const placed = await place($, file)
  if (placed.real === (await realOf($, await configPath($)))) return configRecord(file)
  // Every board is a git repository, so a file outside one is no board record either.
  const found = await locate($, placed.dir)
  if (found === null) return null
  const all: Mount[] = []
  const boards: { board: string; real: string; mounts: Mount[] }[] = []
  for (const board of await knownBoards($)) {
    const mounts = await mountsIn($, board)
    const real = await realOf($, board)
    boards.push({ board, real, mounts })
    const root = `${real.replace(/\/$/, '')}/`
    if (placed.real.startsWith(root)) {
      const relative = placed.real.slice(root.length)
      if (isChangeRecord(relative)) return changeRecord(file, board)
      const record = mounts.find(m => m.record === relative)
      if (record !== undefined) return mountRecord(file, record)
    }
    all.push(...mounts)
  }
  // A known board's repository holds records, which ownRecord guards, and no code.
  const repoReal = await realOf($, found.repo)
  const own = boards.find(b => sameDirectory(repoReal, b.real))
  if (own !== undefined) return ownRecord($, file, placed.real, found, own)
  const worktreeOf = (repo: string) => all.filter(m => m.repo === repo)
  const byOrigin = async () => {
    if (all.length === 0) return []
    const local = localOrigin(found.url, found.repo)
    if (local !== null) {
      // An origin directory that is gone, or holds no repository, names no mount.
      const origin = await locate($, local).catch(() => null)
      return origin === null ? [] : worktreeOf(origin.repo)
    }
    const origin = originOf(found.url)
    if (origin === null) return []
    const repos = [...new Set(all.flatMap(m => (m.repo === null ? [] : [m.repo])))]
    const ran = await sh($, ORIGINS, repos)
    if (ran.exitCode !== 0) throw new Unknown(`git could not read the mounted repositories' origins: ${said(ran)}`)
    const origins = parseOrigins(ran.stdout)
    return all.filter(m => m.repo !== null && m.repo !== found.repo && origins.get(m.repo) === origin)
  }
  return { boards: boards.map(b => b.board), worktree: worktreeOf(found.repo), byOrigin }
}

/** The branch the checkout that holds `file` is on; '' when HEAD is detached. */
const branchOf = async ($: EngineInterface, file: string) =>
  (await locate($, (await place($, file)).dir))?.branch ?? ''

/** The change the acting agent's first message names, or null. */
async function briefNamed($: EngineInterface, agentId: string | undefined): Promise<string | null> {
  const key = `${await $.session.id()}\n${agentId ?? ''}`
  const kept = briefs.get(key)
  if (kept !== undefined) return kept
  const messages = await $.session.messages(agentId === undefined ? {} : { agentId })
  if (!Array.isArray(messages)) throw new Unknown(`Claude Code read no conversation for this agent: ${messages.deny}`)
  // A full window may have lost the first message, and its oldest row may be one another agent wrote.
  if (messages.length >= WINDOW) {
    throw new Unknown(
      'the conversation holds more than the newest 4,096 entries Claude Code returns, so its first message ' +
        'cannot be read; start a builder from the brief',
    )
  }
  const first = messages.find(m => m.role === 'user')
  const named = first === undefined ? null : briefOf(first.text)
  if (named !== null) briefs.set(key, named)
  return named
}

/**
 * The refusal of an edit of `file` by `agentId`, or null to admit it. The
 * guard chooses no mount: the brief's change names one, in the board that
 * holds the change, and the edit is admitted when that mount's repository is
 * the file's. The mounts the file's origin names are read only when its
 * worktree mounts do not settle that.
 */
async function refusal($: EngineInterface, file: string, agentId: string | undefined): Promise<string | null> {
  try {
    const found = await held($, file)
    if (found === null || typeof found === 'string') return found
    let mounts = found.worktree
    let widened = false
    const widen = async () => {
      if (!widened) mounts = [...mounts, ...(await found.byOrigin())]
      widened = true
    }
    if (mounts.length === 0) await widen()
    if (mounts.length === 0) return null
    const named = await briefNamed($, agentId)
    if (named === null) return noBrief(file, mounts[0]!, null)
    for (const board of found.boards) {
      const ran = await sh($, GET, [board, named])
      const change = parseChange(ran.stdout)
      if (change.kind === 'missing') continue
      if (change.kind === 'failed') {
        const why = ran.exitCode !== 0 && ran.stdout.trim() === '' ? said(ran) : change.why
        return unknown(file, `\`board -C ${board} get ${named}\` failed with ${why}`)
      }
      const names = (m: Mount) => m.board === board && m.mount === change.mount
      if (!mounts.some(names)) await widen()
      // With no mount the change names, the refusal names another of its board's mounts.
      const at = mounts.find(names) ?? mounts.find(m => m.board === board)
      // A board that mounts the file's repository nowhere holds another change of this name.
      if (at === undefined) continue
      const got = await sh($, GET, [board, `${named}/prototype`])
      const kind = parseKind(got.stdout)
      if (kind.kind === 'failed') {
        const why = got.exitCode !== 0 && got.stdout.trim() === '' ? said(got) : kind.why
        return unknown(file, `\`board -C ${board} get ${named}/prototype\` failed with ${why}`)
      }
      if (kind.kind === 'prototype') {
        const checked = await sh($, CHECK, [board, `${named}/prototype`])
        const health = parseHealth(checked.stdout)
        if (health.kind === 'failed') {
          const why = checked.stdout.trim() === '' ? said(checked) : health.why
          return unknown(file, `\`board -C ${board} check --healthy ${named}/prototype\` failed with ${why}`)
        }
      }
      const prototype = kind.kind === 'prototype'
      return judge(file, at, named, { board, ...change }, prototype, prototype ? await branchOf($, file) : '')
    }
    return noBrief(file, mounts[0]!, named)
  } catch (error) {
    return unknown(file, error instanceof Unknown ? error.message : `the lookup threw ${String(error)}`)
  }
}

// A hook that throws is skipped and the call runs, so each `.catch` refuses.
// The engine requires `on('<event>', hook)` spelled at each call, so the three
// registrations cannot share one helper.
export const register: Register = on => {
  const failed = (file: string) => ({ deny: unknown(file, 'the guard failed') })
  on('tool.call', { tool: 'Edit' }, async ($, e, next) => {
    const deny = await refusal($, e.file_path, e.agentId)
    return deny === null ? next(e) : { deny }
  }).catch(($, e) => failed(e.file_path))
  on('tool.call', { tool: 'Write' }, async ($, e, next) => {
    const deny = await refusal($, e.file_path, e.agentId)
    return deny === null ? next(e) : { deny }
  }).catch(($, e) => failed(e.file_path))
  on('tool.call', { tool: 'NotebookEdit' }, async ($, e, next) => {
    const deny = await refusal($, e.notebook_path, e.agentId)
    return deny === null ? next(e) : { deny }
  }).catch(($, e) => failed(e.notebook_path))
}
