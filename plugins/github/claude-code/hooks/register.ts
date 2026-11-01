import type { EngineInterface, Register } from 'claude-code'

import { destination, refusal, writes } from './command'
import type { Write } from './command'
import { BOUNDARIES_SQL, covers, parseBoundaries, parseMount } from './cover'
import type { Boundary } from './cover'

// The mod reads the personal board and the checkout's git config, and writes
// nothing. It refuses a Bash call only when it is sure the call writes to
// GitHub and a boundary covers that destination; when anything it reads
// fails, the call runs as it would without the mod (README.md says why).

const PATH = '"$HOME/.local/bin:/opt/homebrew/bin:/usr/local/bin:$PATH"'
// Each script starts in the write's directory, $1, relative to the session's.
const ENTER = `d=$1; shift; case $d in "~"*) d="$HOME\${d#"~"}" ;; esac; [ -z "$d" ] || cd "$d" 2>/dev/null || exit 3`
const BOARD = `${ENTER}; PATH=${PATH} exec board --json "$@"`
// The URLs a push to remote $1 goes to, as configured before any rewrite.
// With no remote named, git's push remote for the branch, else origin. A
// remote that is not a configured name is a URL itself.
const URLS = `${ENTER}
r=$1
if [ -z "$r" ]; then
  b=$(git symbolic-ref -q HEAD) && r=$(git for-each-ref --format='%(push:remotename)' "$b")
  [ -n "$r" ] || r=origin
fi
git config --get-all "remote.$r.pushurl" || git config --get-all "remote.$r.url" || printf '%s\\n' "$r"`

// A slow or hung board or git holds the Bash call no longer than this.
const TIMEOUT_MS = 5000

const run = ($: EngineInterface, script: string, dir: string, args: string[]) =>
  $.process.run(['/bin/sh', '-c', script, 'sh', dir, ...args], { timeoutMs: TIMEOUT_MS })

/** The GitHub destinations a write goes to: the repository it names, or its checkout's remote. */
async function destinations($: EngineInterface, write: Write, dir: string): Promise<string[]> {
  if (write.repo !== undefined) return [`github:${write.repo}`.toLowerCase()]
  const remote = write.action === 'push' ? (write.remote ?? '') : 'origin'
  const ran = await run($, URLS, dir, [remote])
  if (ran.exitCode !== 0) return []
  return ran.stdout.split('\n').flatMap(url => destination(url) ?? [])
}

/** The reason to refuse `command`, or null to let it run. */
async function decide($: EngineInterface, command: string): Promise<string | null> {
  const found = writes(command)
  if (found === null || found.length === 0) return null
  let boundaries: Boundary[] | null = null
  for (const write of found) {
    const dir = write.dir
    if (dir === null) continue
    if (boundaries === null) {
      const ran = await run($, BOARD, dir, ['--personal', 'query', BOUNDARIES_SQL, '--limit', '1000'])
      boundaries = ran.exitCode === 0 ? parseBoundaries(ran.stdout) : null
      if (boundaries === null) return null
    }
    const matching = boundaries
    for (const to of await destinations($, write, dir)) {
      const candidates = matching.filter(b => covers(b, write.action, to, null))
      if (candidates.length === 0) continue
      // `from` limits a boundary to checkouts under a mount, so the mount is read only when one has it.
      let origin: string | null | undefined = null
      if (candidates.some(b => b.from !== null)) {
        const ran = await run($, BOARD, dir, ['refs', '.'])
        origin = parseMount(ran.exitCode, ran.stdout)
      }
      if (candidates.some(b => covers(b, write.action, to, origin))) return refusal(write, to)
    }
  }
  return null
}

export const register: Register = on => {
  on('tool.call', { tool: 'Bash' }, async ($, e, next) => {
    const reason = await decide($, e.command)
    return reason === null ? next(e) : { deny: reason }
  }).catch(($, e, next) => next(e))
}
