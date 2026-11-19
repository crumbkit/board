import type { EngineInterface, Register } from 'claude-code'

import { codeIn, refusal, writes } from './command'
import type { Write } from './command'

// The mod writes nothing. It refuses a Bash call only when it is sure the call
// writes to GitHub and a boundary covers that destination; when anything it
// reads fails, the call runs as it would without the mod (README.md says why).
// It reads the code spans and fenced blocks of an Agent call's prompt by the
// same rules, and advises the lead and the builder of each covered write,
// which the builder's own Bash call would be refused; it never refuses the call.
//
// `github.covers` decides the destination and whether a boundary covers it, by
// the sending actions' own rules. It reads the git config and the board mount
// of the directory the command names, before the person has approved the
// command: git config there is read, though no git hook runs. Every value
// reaches the shell as a positional parameter, never as script text.

const PATH = '"$HOME/.local/bin:/opt/homebrew/bin:/usr/local/bin:$PATH"'
// Starts in the write's directory, $1, relative to the session's, and runs board with the rest.
const COVERS = `d=$1; shift; case $d in "~"*) d="$HOME\${d#"~"}" ;; esac
[ -z "$d" ] || cd "$d" 2>/dev/null || exit 3
PATH=${PATH} exec board --json --personal call github.covers "$@"`

// A slow or hung board holds the Bash call no longer than this.
const TIMEOUT_MS = 5000

/** The first destination of `write` a boundary covers, or null when none is or `github.covers` fails. */
async function covered($: EngineInterface, write: Write, dir: string): Promise<string | null> {
  const args = ['--action', write.action]
  if (write.repo !== undefined) args.push('--repo', write.repo)
  else if (write.remote) args.push('--remote', write.remote)
  const ran = await $.process.run(['/bin/sh', '-c', COVERS, 'sh', dir, ...args], { timeoutMs: TIMEOUT_MS })
  if (ran.exitCode !== 0) return null
  try {
    const report = JSON.parse(ran.stdout) as { data?: { output?: { covered?: { to?: unknown }[] } } }
    const to = report.data?.output?.covered?.[0]?.to
    return typeof to === 'string' ? to : null
  } catch {
    return null
  }
}

/** The reason to refuse `command`, or null to let it run. */
async function decide($: EngineInterface, command: string): Promise<string | null> {
  for (const write of writes(command) ?? []) {
    if (write.dir === null) continue
    const to = await covered($, write, write.dir)
    if (to !== null) return refusal(write, to)
  }
  return null
}

/** The advice on each covered write a code span or fenced block of `prompt` makes, or null for none. Never throws. */
async function briefAdvice($: EngineInterface, prompt: string): Promise<string | null> {
  // Each write once, by what github.covers is asked: its directory, action, repository and remote.
  const unique = new Map<string, [Write, string]>()
  try {
    for (const code of codeIn(prompt)) {
      for (const write of writes(code) ?? []) {
        const key = [write.dir, write.action, write.repo ?? '', write.remote ?? ''].join('\0')
        if (write.dir !== null && !unique.has(key)) unique.set(key, [write, write.dir])
      }
    }
    const found = await Promise.all(
      [...unique.values()].map(async ([write, dir]) => {
        const to = await covered($, write, dir)
        return to === null ? null : `github::brief::covered: ${refusal(write, to)}`
      }),
    )
    const advice = [...new Set(found.filter(line => line !== null))]
    return advice.length === 0 ? null : advice.join('\n')
  } catch {
    return null
  }
}

export const register: Register = on => {
  on('tool.call', { tool: 'Bash' }, async ($, e, next) => {
    const reason = await decide($, e.command)
    return reason === null ? next(e) : { deny: reason }
  }).catch(($, e, next) => next(e))
  // The advice goes to the builder's prompt and the lead's result; briefAdvice never throws, so next runs once.
  on('tool.call', { tool: 'Agent' }, async ($, e, next) => {
    const advice = await briefAdvice($, e.prompt)
    if (advice === null) return next(e)
    const ran = await next({ ...e, prompt: `${e.prompt}\n\n${advice}` })
    return ran.deny !== undefined ? ran : { ...ran, context: [...(ran.context ?? []), advice] }
  })
}
