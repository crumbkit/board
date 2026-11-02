import type { EngineInterface, Register } from 'claude-code'

import {
  MOUNTS_SQL,
  briefOf,
  judge,
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
// `board-brief: /changes/<name>` for a change in build to that repository,
// and, for a prototype, only on its branch `proto/<name>`.
// Every other file is admitted untouched. A lookup that fails refuses: a guard
// that admits what it cannot check would be optional. There is no bypass.
//
// Soft limit: the guard sees only these three tools. A file written through
// Bash (`sed -i`, a heredoc, a script) passes it. The review gate covers that
// gap: the reviewer checks that every commit of a change came from the
// change's builder sessions. A Bash guard would need a command parser.

const PATH = '"$HOME/.local/bin:/opt/homebrew/bin:/usr/local/bin:$PATH"'
// The file may not exist yet, so git runs in its nearest directory that does.
// It prints the repository's common directory, the checkout's branch (an
// empty line when HEAD is detached), then its origin URL, if any.
const LOCATE =
  'd=$1; while [ ! -d "$d" ]; do d=$(dirname "$d"); done; ' +
  'cd "$d" && git rev-parse --path-format=absolute --git-common-dir && ' +
  '{ git symbolic-ref --short -q HEAD || echo; } && { git config --get remote.origin.url || true; }'
// Each mounted repository's origin URL, `<repo>\t<url>` per line; empty when it has none.
const ORIGINS = 'for r; do printf \'%s\\t%s\\n\' "$r" "$(git -C "$r" config --get remote.origin.url 2>/dev/null)"; done'
// Exit 3 says there is no config, so no board is known.
const BOARDS = 'f="${XDG_CONFIG_HOME:-$HOME/.config}/board/config.json"; [ -e "$f" ] || exit 3; exec cat "$f"'
const MOUNTS = `PATH=${PATH} exec board -C "$1" --json query --all "$2"`
const GET = `PATH=${PATH} exec board -C "$1" --json get "$2"`

// A slow or hung git or board holds the edit no longer than this, then refuses it.
const TIMEOUT_MS = 5000

// The change each agent's brief named, by agent id ('' for the main
// conversation). `$.session.messages` returns the newest 4096 entries, and a
// compaction replaces the first message, so a brief found once is kept.
// Soft limit: an agent whose first edit comes after its first message left
// that window is refused as having no brief; the cache starts over when the
// module reloads. Reading the transcript file would replace it.
const briefs = new Map<string, string>()

type Ran = { exitCode: number; stdout: string; stderr: string }
const sh = ($: EngineInterface, script: string, args: string[]): Promise<Ran> =>
  $.process.run(['/bin/sh', '-c', script, 'guard', ...args], { timeoutMs: TIMEOUT_MS })

const said = (ran: Ran) => (ran.stderr.trim() || ran.stdout.trim()).slice(0, 500) || `exit ${ran.exitCode}`

class Unknown extends Error {}

/**
 * The mounts that hold `file`, across every known board; empty when none does.
 * A mount holds the file when the file's repository is a worktree of the
 * mount's `repo`, or else when both repositories' origins name one repository,
 * so a separate clone is guarded too. A repository with no origin that is no
 * worktree of a mount stays outside the guard.
 */
async function mountsOf($: EngineInterface, file: string): Promise<{ mounts: Mount[]; branch: string }> {
  const located = await sh($, LOCATE, [file])
  if (located.exitCode !== 0) {
    if (/not a git repository/i.test(located.stderr)) return { mounts: [], branch: '' }
    throw new Unknown(`git could not find the repository of the file: ${said(located)}`)
  }
  const [commonDir = '', branch = '', url = ''] = located.stdout.split('\n')
  const repo = repositoryOf(commonDir)
  const origin = originOf(url)
  const config = await sh($, BOARDS, [])
  if (config.exitCode === 3) return { mounts: [], branch }
  if (config.exitCode !== 0) throw new Unknown(`the board config could not be read: ${said(config)}`)
  const boards = parseBoards(config.stdout)
  if (!Array.isArray(boards)) throw new Unknown(boards.why)
  const all: Mount[] = []
  for (const board of boards) {
    const ran = await sh($, MOUNTS, [board, MOUNTS_SQL])
    const mounts = ran.exitCode === 0 ? parseMounts(board, ran.stdout) : null
    if (mounts === null) throw new Unknown(`\`board -C ${board} query\` did not list its mounts: ${said(ran)}`)
    all.push(...mounts)
  }
  const worktreeOf = all.filter(m => m.repo === repo)
  if (worktreeOf.length > 0 || origin === null || all.length === 0) return { mounts: worktreeOf, branch }
  const ran = await sh($, ORIGINS, [...new Set(all.map(m => m.repo))])
  if (ran.exitCode !== 0) throw new Unknown(`git could not read the mounted repositories' origins: ${said(ran)}`)
  const origins = parseOrigins(ran.stdout)
  return { mounts: all.filter(m => origins.get(m.repo) === origin), branch }
}

/** The change the acting agent's first message names, or null. */
async function briefNamed($: EngineInterface, agentId: string | undefined): Promise<string | null> {
  const key = agentId ?? ''
  const kept = briefs.get(key)
  if (kept !== undefined) return kept
  const messages = await $.session.messages(agentId === undefined ? {} : { agentId })
  if (!Array.isArray(messages)) throw new Unknown(`Claude Code read no conversation for this agent: ${messages.deny}`)
  const first = messages.find(m => m.role === 'user')
  const named = first === undefined ? null : briefOf(first.text)
  if (named !== null) briefs.set(key, named)
  return named
}

/** The refusal of an edit of `file` by `agentId`, or null to admit it. */
async function refusal($: EngineInterface, file: string, agentId: string | undefined): Promise<string | null> {
  try {
    const { mounts, branch } = await mountsOf($, file)
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
      return judge(file, at, named, change, branch)
    }
    return noBrief(file, mounts[0]!, named)
  } catch (error) {
    return unknown(file, error instanceof Unknown ? error.message : `the lookup threw ${String(error)}`)
  }
}

// A hook that throws is skipped and the call runs, so each `.catch` refuses.
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
