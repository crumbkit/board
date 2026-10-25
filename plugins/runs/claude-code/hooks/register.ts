import { atom, read, update } from 'claude-code'
import type { EngineInterface, PluginOptions, Register } from 'claude-code'

import type { Started } from '../types'

// Each hook writes one record through `board put` and reports it. A hook
// never blocks what it watches: it records after the engine has answered, and
// a failure is logged, never thrown.

const started = atom({ plugin: 'board-runs', key: 'started' } as const, {} as Started)
const written = atom({ plugin: 'board-runs', key: 'written' } as const, 0)

// A put refused with one of these means the board does not take this record
// at all: no board here, or no plans plugin. Only the debug log says so.
const QUIET = ['board::config::not_found', 'board::record::unknown_type']
const PR_CREATE = /(^|[\s;&|(])gh\s+pr\s+create\b/
const PR_URL = /https:\/\/[^\s/]+\/[^\s/]+\/([^\s/]+)\/pull\/(\d+)/g

type Config = { board: string; suite: string; decisionsTemplate: string }
type Unit = { kind: 'agent' | 'task'; id: string; by: string; summary: string | undefined; noun: string }
const HOOK = { agent: 'subagent-stop', task: 'task-completed' }
type Refused = { code: string; message: string }

/** The last line of `text` that holds anything but spaces, trimmed. */
export function finalLine(text: string | undefined): string | undefined {
  return text?.split('\n').map(line => line.trim()).filter(line => line !== '').at(-1)
}

/** The last pull request URL in `stdout`, with its repository's name and its number. */
export function pullRequest(stdout: string): { url: string; repo: string; number: string } | undefined {
  const match = [...stdout.matchAll(PR_URL)].at(-1)
  return match === undefined ? undefined : { url: match[0], repo: match[1]!, number: match[2]! }
}

const iso = (ms: number) => new Date(ms).toISOString().replace(/\.\d{3}Z$/, 'Z')
const stamp = (ms: number) => iso(ms).replace(/[-:]/g, '')
const segment = (id: string) => id.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '')

// The options' defaults are in plugin.json, which fills them in.
const config = (options: PluginOptions): Config => ({
  board: String(options.board ?? ''),
  suite: String(options.suite),
  decisionsTemplate: String(options.decisions_template),
})

/** Runs `board put`; resolves the refusal's code and message, or undefined once written. */
async function put($: EngineInterface, board: string, path: string, body: object, expectNone = false): Promise<Refused | undefined> {
  const argv = ['board', ...(board === '' ? [] : ['-C', board]), '--json', 'put', path, ...(expectNone ? ['--expect', 'none'] : [])]
  const ran = await $.process.run(argv, { stdin: JSON.stringify(body) })
  if (ran.exitCode === 0) return undefined
  try {
    const failure = JSON.parse(ran.stdout) as Partial<Refused>
    return { code: failure.code ?? 'unknown', message: failure.message ?? ran.stderr.trim() }
  } catch {
    return { code: 'unknown', message: ran.stderr.trim() || `board exited with status ${ran.exitCode}` }
  }
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

// Whether this load has made sure the suite exists. A reload asks again.
let suiteReady = false

async function begin($: EngineInterface, key: string) {
  const now = await $.clock.now()
  await update($, started, map => ({ ...map, [key]: now }))
}

/** Writes the run of a delegated unit that stopped. A failure is logged, never thrown. */
async function writeRun($: EngineInterface, { board, suite }: Config, unit: Unit) {
  try {
    const finished = await $.clock.now()
    const key = `${unit.kind}:${unit.id}`
    const since = (await read($, started))[key]
    await update($, started, ({ [key]: _, ...rest }) => rest)
    if (!suiteReady) {
      const method = 'Each run is one subagent or task a Claude Code session delegated, recorded when it stopped.'
      const refusal = await put($, board, suite, { $type: 'runs/suite/v1', subjects: ['claude-code'], method }, true)
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
      written_by: `runs/${HOOK[unit.kind]}`,
    }
    const refusal = await put($, board, path, run)
    if (refusal === undefined) await counted($, `recorded the ${unit.noun} as ${path}`)
    else refused($, `no run for the ${unit.noun}`, refusal)
  } catch (error) {
    $.ui.log(`board: no run for the ${unit.noun}: ${String(error)}`)
  }
}

/** Writes a decision stub for the pull request `gh pr create` printed. A failure is logged, never thrown. */
async function stub($: EngineInterface, { board, decisionsTemplate }: Config, stdout: string, agentId: string | undefined) {
  try {
    const pr = pullRequest(stdout)
    if (pr === undefined) return $.ui.log('board: gh pr create printed no pull request URL, so no decision stub was written', { to: 'debug' })
    const view = await $.process.run(['gh', 'pr', 'view', pr.url, '--json', 'title', '--jq', '.title'])
    const title = view.stdout.trim()
    if (view.exitCode !== 0 || title === '') return $.ui.log(`board: gh pr view ${pr.url} gave no title, so no decision stub was written`)
    const now = await $.clock.now()
    const repo = segment(pr.repo)
    const path = `${decisionsTemplate.replaceAll('{repo}', repo)}/${stamp(now)}-${repo}-${pr.number}`
    const decision = {
      $type: 'plans/decision/v1',
      decision: title,
      why: '',
      session: await $.session.id(),
      decided: iso(now),
      refs: [pr.url],
      review: 'wanted',
      written_by: 'runs/pr-create',
    }
    const refusal = await put($, board, path, decision)
    if (refusal !== undefined) return refused($, `no decision stub for ${path}`, refusal)
    await counted($, `decision stub ${path} for ${pr.url}, review wanted`)
    const ask =
      `The runs mod recorded ${pr.url} as a decision stub at ${path}. ` +
      'Its why is empty. Fill in why and alternatives: read it with board get, then board put it back.'
    // The row goes to the agent that opened the pull request, a subagent or the main one.
    await $.session.append({ message: { type: 'user', content: [{ type: 'text', text: ask }] }, ...(agentId === undefined ? {} : { agentId }) })
  } catch (error) {
    $.ui.log(`board: no decision stub: ${String(error)}`)
  }
}

export const register: Register = (on, options) => {
  const cfg = config(options)

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

  on('tool.call', { tool: 'Bash' }, async ($, e, next) => {
    const ran = await next(e)
    if (PR_CREATE.test(e.command) && ran.deny === undefined && ran.isError !== true) await stub($, cfg, ran.result.stdout, e.agentId)
    return ran
  })
}
