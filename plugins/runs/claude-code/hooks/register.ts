import { atom, read, update } from 'claude-code'
import type { EngineInterface, PluginOptions, Register } from 'claude-code'

import type { Agents, Started } from '../types'

// Every hook runs `board --personal`, so its records, the gate's changes and
// the brief come from the personal board whatever the session's directory.
//
// The recording hooks write after the engine has answered and never block:
// a failure is logged, never thrown. The pull request gate is the one hook
// that blocks what it watches. It refuses `gh pr create` before it runs when
// no open change names the repository and branch, because a pull request
// opened outside a change leaves the owner no record of its plan, checks and
// review. It reads local records only and makes no network call.

const started = atom({ plugin: 'board-runs', key: 'started' } as const, {} as Started)
const agents = atom({ plugin: 'board-runs', key: 'agents' } as const, {} as Agents)
const written = atom({ plugin: 'board-runs', key: 'written' } as const, 0)

// A put refused with one of these means the board does not take this record
// at all: no personal board, or no runs plugin. Only the debug log says so.
const QUIET = ['board::config::not_found', 'board::record::unknown_type']
const PR_URL = /https:\/\/[^\s/]+\/[^\s/]+\/([^\s/]+)\/pull\/(\d+)/g
// The line that ends a brief to a subagent working on a change (plugins/changes/SKILL.md).
const BOARD_LINE = /^Board:\s*(\/changes\/\S+)$/
// The commands that change directory, and the `gh pr` subcommands that open a pull request.
const CHANGES_DIRECTORY = ['cd', 'pushd', 'popd']
const OPENS = ['create', 'new']

type Config = { suite: string }
type Unit = { kind: 'agent' | 'task'; id: string; by: string; summary: string | undefined; noun: string }
const HOOK = { agent: 'subagent-stop', task: 'task-completed' }
type Refused = { code: string; message: string }
type Ran = { exitCode: number; stdout: string; stderr: string }
type Open = { changes: { path: string }[]; total: number }

/** The last line of `text` that holds anything but spaces, trimmed. */
export function finalLine(text: string | undefined): string | undefined {
  return text?.split('\n').map(line => line.trim()).filter(line => line !== '').at(-1)
}

/** The last pull request URL in `stdout`, with its repository's name and its number. */
export function pullRequest(stdout: string): { url: string; repo: string; number: string } | undefined {
  const match = [...stdout.matchAll(PR_URL)].at(-1)
  return match === undefined ? undefined : { url: match[0], repo: match[1]!, number: match[2]! }
}

/** The change the prompt's last line names as `Board: /changes/<name>`. */
export function boardLine(prompt: string): string | undefined {
  const line = finalLine(prompt)
  return line === undefined ? undefined : BOARD_LINE.exec(line)?.[1]
}

/** What `gh pr create` in `command` targets: the repository and branch it names, and the directory to read the rest from. */
export type Target = { repo: string | undefined; head: string | undefined; dir: string | undefined } | { unclear: string }

/** One simple command of a shell line: its words, without quotes or leading `VAR=x` assignments, and the operator after it. */
export type Simple = { words: string[]; then: string }

/**
 * The simple commands of `command`, split at `&&`, `||`, `;`, `|`, `(`, `)`
 * and newlines outside quotes. A heredoc's body is skipped, so text in it,
 * as in quotes, starts no command. This reads shell well enough to find
 * where commands start; it does not expand anything.
 */
export function commands(command: string): Simple[] {
  const found: Simple[] = []
  let words: string[] = []
  let word: string | undefined
  let heredocs: string[] = []
  const endWord = () => {
    if (word !== undefined) words.push(word)
    word = undefined
  }
  const end = (then: string) => {
    endWord()
    while (words.length > 0 && /^[A-Za-z_][A-Za-z0-9_]*=/.test(words[0]!)) words.shift()
    if (words.length > 0) found.push({ words, then })
    words = []
  }
  let at = 0
  while (at < command.length) {
    const char = command[at]!
    if (char === "'" || char === '"') {
      const close = command.indexOf(char, at + 1)
      const stop = close === -1 ? command.length : close
      word = (word ?? '') + command.slice(at + 1, stop)
      at = stop + 1
    } else if (char === '<' && command.startsWith('<<', at) && !command.startsWith('<<<', at)) {
      endWord()
      const marker = /^<<-?\s*(?:'([^']*)'|"([^"]*)"|([^\s;&|()<>]+))/.exec(command.slice(at))
      if (marker !== null) heredocs.push(marker[1] ?? marker[2] ?? marker[3]!)
      at += marker === null ? 2 : marker[0].length
    } else if (char === '\n') {
      end('\n')
      at += 1
      for (const delimiter of heredocs) {
        while (at < command.length) {
          const next = command.indexOf('\n', at)
          const line = command.slice(at, next === -1 ? command.length : next)
          at = next === -1 ? command.length : next + 1
          if (line.trim() === delimiter) break
        }
      }
      heredocs = []
    } else if (command.startsWith('&&', at) || command.startsWith('||', at)) {
      end(command.slice(at, at + 2))
      at += 2
    } else if (';|()&'.includes(char)) {
      end(char)
      at += 1
    } else if (/\s/.test(char)) {
      endWord()
      at += 1
    } else {
      word = (word ?? '') + char
      at += 1
    }
  }
  end('')
  return found
}

/** Whether a simple command is `gh pr create` or `gh pr new`. */
const opens = ({ words }: Simple) => words[0] === 'gh' && words[1] === 'pr' && OPENS.includes(words[2] ?? '')

/** Whether `command` runs `gh pr create` or `gh pr new` as a command of its own. */
export function opensPullRequest(command: string): boolean {
  return commands(command).some(opens)
}

/** The value of the flag `short` or `long` among `args`, as `-R x`, `--repo x` or `--repo=x`. */
function flag(args: string[], short: string, long: string): string | undefined {
  for (const [at, arg] of args.entries()) {
    if (arg === short || arg === long) return args[at + 1]
    if (arg.startsWith(`${long}=`)) return arg.slice(long.length + 1)
    if (arg.startsWith(short) && arg.length > short.length && !arg.startsWith('--')) return arg.slice(short.length)
  }
  return undefined
}

/**
 * The repository and branch the first `gh pr create` in `command` targets.
 *
 * `-R` and `--head` name them. What they leave out is read from the
 * directory: a leading `cd <dir> &&`, else the session's. Any other change
 * of directory before the command makes the directory unknown.
 */
export function target(command: string): Target {
  const all = commands(command)
  const index = all.findIndex(opens)
  if (index === -1) return { unclear: 'the command runs no gh pr create' }
  const args = all[index]!.words.slice(3)
  const repo = flag(args, '-R', '--repo')
  const head = flag(args, '-H', '--head')?.replace(/^[^:]+:/, '')
  if (repo !== undefined && head !== undefined) return { repo, head, dir: undefined }
  const before = all.slice(0, index)
  const first = before[0]
  const leading = first !== undefined && first.words[0] === 'cd' && first.words.length === 2 && first.then === '&&'
  const rest = leading ? before.slice(1) : before
  if (rest.some(simple => CHANGES_DIRECTORY.includes(simple.words[0]!))) {
    return { unclear: 'the command changes directory before gh pr create, so its repository and branch are unknown' }
  }
  return { repo, head, dir: leading ? first.words[1] : undefined }
}

/** `-R`'s `HOST/OWNER/REPO` as a URL, so the stored form of its remote is `OWNER/REPO`; any other value as given. */
export function remoteOf(repo: string): string {
  return /^[^/:@]+\.[^/:@]+\/[^/]+\/[^/]+$/.test(repo) ? `https://${repo}` : repo
}

const iso = (ms: number) => new Date(ms).toISOString().replace(/\.\d{3}Z$/, 'Z')
const stamp = (ms: number) => iso(ms).replace(/[-:]/g, '')
const segment = (id: string) => id.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '')

// The option's default is in plugin.json, which fills it in.
const config = (options: PluginOptions): Config => ({ suite: String(options.suite) })

function refusal(ran: Ran): Refused {
  try {
    const failure = JSON.parse(ran.stdout) as Partial<Refused>
    return { code: failure.code ?? 'unknown', message: failure.message ?? ran.stderr.trim() }
  } catch {
    return { code: 'unknown', message: ran.stderr.trim() || `board exited with status ${ran.exitCode}` }
  }
}

/** Runs `board --personal --json <args>`; resolves its output document, or the refusal's code and message. */
async function board($: EngineInterface, args: string[], stdin?: string): Promise<{ data: unknown } | Refused> {
  const ran = await $.process.run(['board', '--personal', '--json', ...args], stdin === undefined ? undefined : { stdin })
  if (ran.exitCode !== 0) return refusal(ran)
  return JSON.parse(ran.stdout) as { data: unknown }
}

/** Runs `board --personal call <capability>`; resolves its output, or the refusal. */
async function call<T>($: EngineInterface, capability: string, args: string[]): Promise<{ output: T } | Refused> {
  const ran = await board($, ['call', capability, ...args])
  return 'data' in ran ? { output: (ran.data as { output: T }).output } : ran
}

/** Runs `board put`; resolves the refusal's code and message, or undefined once written. */
async function put($: EngineInterface, path: string, body: object, expectNone = false): Promise<Refused | undefined> {
  const ran = await board($, ['put', path, ...(expectNone ? ['--expect', 'none'] : [])], JSON.stringify(body))
  return 'data' in ran ? undefined : ran
}

async function counted($: EngineInterface, line: string) {
  $.ui.log(`board: ${line}`)
  const count = await update($, written, n => n + 1)
  $.ui.status(`board: ${count} record${count === 1 ? '' : 's'} this session`)
}

function refused($: EngineInterface, what: string, refusal: Refused) {
  const to = QUIET.includes(refusal.code) ? 'debug' : 'transcript'
  $.ui.log(`board: ${what}: ${refusal.code} ${refusal.message}`, { to })
}

/** The session's id: the desktop app's `local_` id when it names one, as claims and questions use, else the transcript's. */
export async function sessionId($: EngineInterface): Promise<string> {
  const host = await $.env.get('CLAUDE_CODE_HOST_SESSION_ID')
  return host?.startsWith('local_') ? host : $.session.id()
}

/** The one open change this session holds, or undefined when it holds none or several. */
async function heldChange($: EngineInterface): Promise<string | undefined> {
  const open = await call<Open>($, 'changes.open', ['--holder', await sessionId($)])
  if (!('output' in open)) return void refused($, 'no held change', open)
  return open.output.total === 1 ? open.output.changes[0]!.path : undefined
}

/** The prompt with the change's brief before it, so the subagent starts with the plan and the controls command; undefined when there is no brief. */
async function briefed($: EngineInterface, prompt: string, change: string): Promise<string | undefined> {
  const brief = await call<object>($, 'changes.brief', ['--path', change])
  if (!('output' in brief)) {
    refused($, `no brief for ${change}`, brief)
    return undefined
  }
  const told = `The runs mod ran \`board --personal call changes.brief --path ${change}\` before you started. It printed:`
  return `${told}\n\n${JSON.stringify(brief.output, null, 2)}\n\n${prompt}`
}

// Whether this load has made sure the suite exists. A reload asks again.
let suiteReady = false

async function begin($: EngineInterface, key: string) {
  const now = await $.clock.now()
  await update($, started, map => ({ ...map, [key]: now }))
}

/** Writes the run of a delegated unit that stopped. A failure is logged, never thrown. */
async function writeRun($: EngineInterface, { suite }: Config, unit: Unit) {
  try {
    const finished = await $.clock.now()
    const key = `${unit.kind}:${unit.id}`
    const since = (await read($, started))[key]
    await update($, started, ({ [key]: _, ...rest }) => rest)
    const agent = unit.kind === 'agent' ? (await read($, agents))[unit.id] : undefined
    if (agent !== undefined) await update($, agents, ({ [unit.id]: _, ...rest }) => rest)
    if (!suiteReady) {
      const method = 'Each run is one subagent or task a Claude Code session delegated, recorded when it stopped.'
      const refusal = await put($, suite, { $type: 'runs/suite/v1', subjects: ['claude-code'], method }, true)
      if (refusal !== undefined && refusal.code !== 'board::put::stale') return refused($, `no suite ${suite}`, refusal)
      suiteReady = true
    }
    const path = `${suite}/runs/${stamp(since ?? finished)}-${segment(unit.id)}`
    const run = {
      $type: 'runs/run/v1',
      suite,
      by: unit.by,
      started: iso(since ?? finished),
      finished: iso(finished),
      scope: 'full',
      results: [],
      ...(since === undefined ? {} : { measures: { duration_s: Math.round((finished - since) / 1000) } }),
      ...(unit.summary === undefined ? {} : { summary: unit.summary }),
      ...(agent === undefined ? {} : { agent }),
      written_by: `runs/${HOOK[unit.kind]}`,
    }
    const refusal = await put($, path, run)
    if (refusal === undefined) await counted($, `recorded the ${unit.noun} as ${path}`)
    else refused($, `no run for the ${unit.noun}`, refusal)
  } catch (error) {
    $.ui.log(`board: no run for the ${unit.noun}: ${String(error)}`)
  }
}

/** Writes the agent record of a subagent that started, with its prompt beside it. A failure is logged, never thrown. */
async function writeAgent($: EngineInterface, id: string, record: object, prompt: string) {
  try {
    const path = `/agents/${segment(id)}`
    const lines = [
      { path: `${path}.md`, expect: 'none', text: prompt },
      { path, expect: 'none', record },
    ]
    const ran = await board($, ['put', '--batch'], lines.map(line => `${JSON.stringify(line)}\n`).join(''))
    if (!('data' in ran)) return refused($, `no agent record for ${id}`, ran)
    await update($, agents, map => ({ ...map, [id]: path }))
    await counted($, `recorded the subagent ${id} as ${path}`)
  } catch (error) {
    $.ui.log(`board: no agent record for ${id}: ${String(error)}`)
  }
}

/** What `git -C <dir> <args>` printed, trimmed, or undefined when it failed. */
async function git($: EngineInterface, dir: string, ...args: string[]): Promise<string | undefined> {
  const ran = await $.process.run(['git', '-C', dir, ...args])
  const out = ran.stdout.trim()
  return ran.exitCode === 0 && out !== '' ? out : undefined
}

/** `dir` as an absolute path, from the session's directory and with `~` as HOME. */
async function absolute($: EngineInterface, dir: string | undefined): Promise<string> {
  const cwd = await $.session.cwd()
  if (dir === undefined) return cwd
  if (dir === '~' || dir.startsWith('~/')) return `${(await $.env.get('HOME')) ?? ''}${dir.slice(1)}`
  return dir.startsWith('/') ? dir : `${cwd.replace(/\/$/, '')}/${dir}`
}

const RETRY = 'run gh pr create with -R <owner/name> and --head <branch>'
const GATE_FAILED = 'board: refused: the pull request gate failed, so it could not look for an open change; `claude --debug` logs why. Run gh pr create again.'

/** The open changes `command` opens a pull request for, or the reason the gate refuses it. */
async function gate($: EngineInterface, command: string): Promise<{ changes: string[] } | { deny: string }> {
  const found = target(command)
  if ('unclear' in found) return { deny: `board: refused: ${found.unclear}; ${RETRY}` }
  const dir = found.repo !== undefined && found.head !== undefined ? undefined : await absolute($, found.dir)
  const repo = found.repo ?? (dir === undefined ? undefined : await git($, dir, 'remote', 'get-url', 'origin'))
  const head = found.head ?? (dir === undefined ? undefined : await git($, dir, 'symbolic-ref', '--quiet', '--short', 'HEAD'))
  if (repo === undefined || head === undefined) {
    const missing = repo === undefined ? 'no origin' : 'no branch checked out'
    return { deny: `board: refused: ${dir} has ${missing}, so the pull request's repository and branch are unknown; ${RETRY}` }
  }
  const open = await call<Open>($, 'changes.open', ['--remote', remoteOf(repo), '--branch', head])
  if (!('output' in open)) {
    return { deny: `board: refused: board --personal call changes.open failed with ${open.code} ${open.message}; fix the personal board, then run gh pr create again` }
  }
  if (open.output.total === 0) {
    return {
      deny:
        `board: refused: no open change names ${repo} on ${head}, so the pull request would have no record. ` +
        `From the clone, on ${head}, run \`board --personal call changes.start --title '<title>' --session <session>\`, ` +
        'which records the repository too when the board has no record of it, then run gh pr create again.',
    }
  }
  return { changes: open.output.changes.map(change => change.path) }
}

/** Writes the `opened` record of the one change the pull request belongs to. A failure is logged, never thrown. */
async function opened($: EngineInterface, changes: string[], stdout: string) {
  try {
    const pr = pullRequest(stdout)
    if (pr === undefined) return $.ui.log('board: gh pr create printed no pull request URL, so no opened record was written', { to: 'debug' })
    if (changes.length !== 1) return $.ui.log(`board: ${changes.length} open changes name this branch (${changes.join(', ')}), so no opened record was written for ${pr.url}`)
    const path = `${changes[0]}/opened`
    const refusal = await put($, path, { $type: 'changes/opened/v1', pull_request: pr.url }, true)
    if (refusal === undefined) await counted($, `recorded ${pr.url} as ${path}`)
    else refused($, `no opened record for ${pr.url}`, refusal)
  } catch (error) {
    $.ui.log(`board: no opened record: ${String(error)}`)
  }
}

export const register: Register = (on, options) => {
  const cfg = config(options)

  on('agent.spawn', async ($, e, next) => {
    // A resumed run continues an agent recorded when it first started.
    if (e.resume !== undefined) return next(e)
    let worksOn: string | undefined
    let prompt = e.prompt
    try {
      // A change is worked on only when its brief could be read.
      const named = boardLine(e.prompt) ?? (await heldChange($))
      const withBrief = named === undefined ? undefined : await briefed($, e.prompt, named)
      if (withBrief !== undefined) [worksOn, prompt] = [named, withBrief]
    } catch (error) {
      $.ui.log(`board: no brief for the subagent: ${String(error)}`)
    }
    const now = await $.clock.now()
    const cwd = await absolute($, e.cwd)
    const result = await next(prompt === e.prompt ? e : { ...e, prompt })
    if (result.deny !== undefined || result.agentId === undefined) return result
    // A teammate or a workflow's agent gets no SubagentStop, so no run would ever close its record.
    if (e.isTeammate === true || e.workflow !== undefined) return result
    const record = {
      $type: 'runs/agent/v1',
      session: await sessionId($),
      cwd,
      started: iso(now),
      ...(worksOn === undefined ? {} : { works_on: worksOn }),
      written_by: 'runs/agent-spawn',
    }
    await writeAgent($, result.agentId, record, e.prompt)
    return result
  }).catch(($, e, next) => next(e))

  on('classic.SubagentStart', async ($, e, next) => {
    await begin($, `agent:${e.agent_id}`)
    return next(e)
  })

  on('classic.SubagentStop', async ($, e, next) => {
    const result = await next(e)
    if (result.block !== undefined) return result
    const noun = `${e.agent_type} subagent`
    await writeRun($, cfg, { kind: 'agent', id: e.agent_id, by: e.agent_type, summary: finalLine(e.last_assistant_message), noun })
    return result
  })

  on('classic.TaskCreated', async ($, e, next) => {
    await begin($, `task:${e.task_id}`)
    return next(e)
  })

  on('classic.TaskCompleted', async ($, e, next) => {
    const result = await next(e)
    if (result.block !== undefined) return result
    const by = e.teammate_name ?? 'claude-code'
    await writeRun($, cfg, { kind: 'task', id: e.task_id, by, summary: e.task_subject, noun: `task ${e.task_id}` })
    return result
  })

  // A gate that fails before the command runs refuses it; one that fails after keeps what the command gave.
  on('tool.call', { tool: 'Bash' }, async ($, e, next) => {
    if (!opensPullRequest(e.command)) return next(e)
    const allowed = await gate($, e.command)
    if ('deny' in allowed) return { deny: allowed.deny }
    const ran = await next(e)
    if (ran.deny === undefined && ran.isError !== true) await opened($, allowed.changes, ran.result.stdout)
    return ran
  }).catch(($, e, next) => (next.called ? next(e) : { deny: GATE_FAILED }))
}
