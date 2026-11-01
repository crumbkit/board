import type { EngineInterface, Register } from 'claude-code'

import {
  MOUNTS_SQL,
  briefOf,
  judge,
  localOrigin,
  noBrief,
  originOf,
  parseBoards,
  parseChange,
  parseMounts,
  parseOrigins,
  repositoryOf,
  unknown,
} from './guard'
import type { Mount } from './guard'

// The write guard. Before an Edit, Write or NotebookEdit of a file in a
// repository that a known board mounts, it reads the acting agent's first
// message and admits the call only when that message starts with
// `board-brief: /changes/<name>` for a change in build to that repository.
// Every other file is admitted untouched. A lookup that fails refuses: a guard
// that admits what it cannot check would be optional. There is no bypass.
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
// Run in a directory, it prints the repository's common directory, then its origin URL, if any.
const LOCATE =
  'cd "$1" && git rev-parse --path-format=absolute --git-common-dir && { git config --get remote.origin.url || true; }'
// Each mounted repository's origin URL, `<repo>\t<url>` per line; empty when it has none.
const ORIGINS = 'for r; do printf \'%s\\t%s\\n\' "$r" "$(git -C "$r" config --get remote.origin.url 2>/dev/null)"; done'
const MOUNTS = `PATH=${PATH} exec board -C "$1" --json query --all "$2"`
const GET = `PATH=${PATH} exec board -C "$1" --json get "$2"`

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
 * The real directory that holds `file`, every symbolic link followed. A file
 * that does not exist yet is placed by its nearest directory that does, and a
 * `~/` path is taken from the home directory. Throws when the path cannot be
 * placed: a `~user` path, or a symbolic link that leads nowhere.
 */
async function folderOf($: EngineInterface, file: string): Promise<string> {
  let path = file
  if (path === '~' || path.startsWith('~/')) {
    const home = await $.env.get('HOME')
    if (home === undefined) throw new Unknown(`it cannot place ${file}, since HOME is unset`)
    path = home + path.slice(1)
  } else if (path.startsWith('~')) {
    throw new Unknown(`it cannot place ${file}`)
  }
  for (let at = path; ; at = parentOf(at)) {
    const stat = await $.fs.stat(at, { resolve: true }).catch(() => undefined)
    if (stat !== undefined) {
      if (stat.realPath === undefined) throw new Unknown(`it cannot place ${file}, since ${at} leads nowhere`)
      return stat.kind === 'dir' ? stat.realPath : parentOf(stat.realPath)
    }
    if (at === '/' || at === '.') throw new Unknown(`it cannot place ${file}`)
  }
}

/** The repository's own directory and origin URL, from `dir`; null outside a repository. */
async function locate($: EngineInterface, dir: string): Promise<{ repo: string; url: string } | null> {
  const ran = await sh($, LOCATE, [dir])
  if (ran.exitCode !== 0) {
    if (/not a git repository/i.test(ran.stderr)) return null
    throw new Unknown(`git could not find the repository in ${dir}: ${said(ran)}`)
  }
  const [commonDir = '', url = ''] = ran.stdout.split('\n')
  return { repo: repositoryOf(commonDir), url }
}

/** The known boards' directories; none when there is no board config. */
async function knownBoards($: EngineInterface): Promise<string[]> {
  const config = (await $.env.get('XDG_CONFIG_HOME')) || `${await $.env.get('HOME')}/.config`
  const path = `${config}/board/config.json`
  if (!(await $.fs.exists(path))) return []
  const boards = parseBoards(await $.fs.read(path))
  if (!Array.isArray(boards)) throw new Unknown(boards.why)
  return boards
}

/**
 * The mounts that hold `file`, across every known board; empty when none does.
 * A mount holds the file when the file's repository is a worktree of the
 * mount's `repo`; else when its origin is a local path or `file://` URL of a
 * worktree of it; else when both repositories' origins name one repository.
 * So a separate clone is guarded too. A repository with no origin that is no
 * worktree of a mount stays outside the guard.
 */
async function mountsOf($: EngineInterface, file: string): Promise<Mount[]> {
  const found = await locate($, await folderOf($, file))
  if (found === null) return []
  const all: Mount[] = []
  for (const board of await knownBoards($)) {
    const ran = await sh($, MOUNTS, [board, MOUNTS_SQL])
    const mounts = ran.exitCode === 0 ? parseMounts(board, ran.stdout) : null
    if (mounts === null) throw new Unknown(`\`board -C ${board} query\` did not list its mounts: ${said(ran)}`)
    all.push(...mounts)
  }
  const worktreeOf = (repo: string) => all.filter(m => m.repo === repo)
  if (worktreeOf(found.repo).length > 0 || all.length === 0) return worktreeOf(found.repo)
  const local = localOrigin(found.url, found.repo)
  if (local !== null) {
    // An origin directory that is gone, or holds no repository, names no mount.
    const origin = await locate($, local).catch(() => null)
    return origin === null ? [] : worktreeOf(origin.repo)
  }
  const origin = originOf(found.url)
  if (origin === null) return []
  const ran = await sh($, ORIGINS, [...new Set(all.map(m => m.repo))])
  if (ran.exitCode !== 0) throw new Unknown(`git could not read the mounted repositories' origins: ${said(ran)}`)
  const origins = parseOrigins(ran.stdout)
  return all.filter(m => origins.get(m.repo) === origin)
}

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

/** The refusal of an edit of `file` by `agentId`, or null to admit it. */
async function refusal($: EngineInterface, file: string, agentId: string | undefined): Promise<string | null> {
  try {
    const mounts = await mountsOf($, file)
    if (mounts.length === 0) return null
    const named = await briefNamed($, agentId)
    if (named === null) return noBrief(file, mounts[0]!, null)
    for (const at of mounts) {
      const ran = await sh($, GET, [at.board, named])
      const change = parseChange(ran.stdout)
      if (change.kind === 'missing') continue
      if (change.kind === 'failed') {
        const why = ran.exitCode !== 0 && ran.stdout.trim() === '' ? said(ran) : change.why
        return unknown(file, `\`board -C ${at.board} get ${named}\` failed with ${why}`)
      }
      return judge(file, at, named, change)
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
