import type { EngineInterface, Register } from 'claude-code'

// One mod per session, no daemon. Each session measures itself after every
// turn (`session.measure`) and polls a trigger file every 30 seconds, so a
// session that is idle when another one notices the account's usage is told
// too. The files sit in the account's Claude config directory, which every
// session on that account shares and no other account does.

type Shares = { context: number; usage: number }
export type Crossing = { key: string; why: string; window?: { kind: string; resetsAt: string | undefined } }
type Reading = { context: { percent?: number }; rateLimits: readonly { kind: string; percentUsed: number; resetsAt?: string }[] }
type Trigger = { key: string; why: string; resetsAt?: string | undefined; at: string }
type Session = { cwd: string; asked: string[]; status: 'asked' | 'handed-off'; line?: string }

// Used when the board gives no value; the calibrated values replace them.
export const DEFAULTS: Shares = { context: 75, usage: 85 }
const POLL_MS = 30_000
// A trigger whose window gave no reset time stops telling sessions after the longest window.
const NO_RESET_MS = 5 * 3_600_000
const MAX_LINES = 20
const SESSION_ID = /^[A-Za-z0-9_-]+$/

/** Everything a measurement crosses: the context share, then each usage window at or over its share. */
export function crossings(reading: Reading, shares: Shares): Crossing[] {
  const found: Crossing[] = []
  const percent = reading.context.percent
  if (percent !== undefined && percent >= shares.context) found.push({ key: 'context', why: `its context window is ${percent}% full` })
  for (const w of reading.rateLimits) {
    if (w.percentUsed >= shares.usage) found.push({ key: `${w.kind}@${w.resetsAt}`, why: `the ${w.kind} usage limit is ${w.percentUsed}% used`, window: { kind: w.kind, resetsAt: w.resetsAt } })
  }
  return found
}

/** The first crossing of a measurement, if any. */
export const crossing = (reading: Reading, shares: Shares): Crossing | undefined => crossings(reading, shares)[0]

/** The line that resumes a session: its directory and id, then its own handoff line when it wrote one. */
export function resumeLine(id: string, cwd: string, handoff?: string): string {
  const dir = /^[\w@%+=:,./-]+$/.test(cwd) ? cwd : `'${cwd.replace(/'/g, "'\\''")}'`
  return `cd ${dir} && claude --resume ${id}${handoff === undefined ? '' : `  # ${handoff}`}`
}

const iso = (ms: number) => new Date(ms).toISOString().replace(/\.\d{3}Z$/, 'Z')
const handoffLine = (answer: string) => answer.split('\n').map(l => l.trim()).filter(l => l.startsWith('handoff:')).at(-1)

const message = (why: string) =>
  `Account usage or your context is near its limit: ${why}. Follow the handoff skill now, before anything else. ` +
  'Commit and push every branch in each checkout you used, record state in the board, and end your reply with the one-line handoff ' +
  '(`handoff: <board path> — resume: <command>`) and then READY TO ARCHIVE.'

type Mem = { id: string | undefined; loading: Promise<void> | undefined; polling: boolean; shares: Promise<Shares> | undefined; cwd: string; asked: Set<string>; status: Session['status']; line: string | undefined }

async function dir($: EngineInterface) {
  const config = (await $.env.get('CLAUDE_CONFIG_DIR')) ?? `${(await $.env.get('HOME')) ?? ''}/.claude`
  return `${config}/board-handoff`
}

async function readJson<T>($: EngineInterface, path: string): Promise<T | undefined> {
  try {
    return JSON.parse(await $.fs.read(path)) as T
  } catch {
    return undefined
  }
}

async function readShare($: EngineInterface, name: string): Promise<number | undefined> {
  try {
    const ran = await $.process.run(['board', '--personal', '--json', 'call', 'params.get', '--name', name])
    const value = Number(JSON.parse(ran.stdout)?.data?.output?.value)
    if (ran.exitCode === 0 && Number.isFinite(value) && value > 0) return value
  } catch {
    // fall through to the default
  }
  return undefined
}

// Reads each calibrated value once per session; the defaults stand in for one the board lacks.
function loadShares($: EngineInterface, mem: Mem): Promise<Shares> {
  mem.shares ??= (async () => {
    const context = await readShare($, 'handoff-context-share')
    const usage = await readShare($, 'handoff-usage-share')
    if (context === undefined || usage === undefined) {
      $.ui.log(`handoff: params unreadable, using ${DEFAULTS.context}% context and ${DEFAULTS.usage}% usage`)
    }
    return { context: context ?? DEFAULTS.context, usage: usage ?? DEFAULTS.usage }
  })()
  return mem.shares
}

async function save($: EngineInterface, mem: Mem) {
  const id = await $.session.id()
  if (!SESSION_ID.test(id)) return
  const session: Session = { cwd: mem.cwd, asked: [...mem.asked], status: mem.status, ...(mem.line === undefined ? {} : { line: mem.line }) }
  try {
    await $.fs.write(`${await dir($)}/sessions/${id}.json`, JSON.stringify(session))
  } catch (error) {
    $.ui.log(`handoff: could not write the session file: ${String(error)}`)
  }
}

/** Loads what the session's own file holds. /clear and /resume change the id without a session.start, so every hook calls this first. */
async function load($: EngineInterface, mem: Mem, id: string) {
  const earlier = SESSION_ID.test(id) ? await readJson<Session>($, `${await dir($)}/sessions/${id}.json`) : undefined
  mem.asked = new Set(earlier?.asked ?? [])
  mem.status = earlier?.status ?? 'asked'
  mem.line = earlier?.line
}

/** Loads what the session's own file holds. /clear and /resume change the id without a session.start, so every hook calls this first and waits for the load. */
async function sync($: EngineInterface, mem: Mem) {
  const id = await $.session.id()
  if (mem.loading === undefined || mem.id !== id) {
    mem.id = id
    mem.loading = load($, mem, id)
  }
  await mem.loading
}

/** Records each new crossing; tells the session to hand off when it has not been told yet or its last handoff is done. */
async function ask($: EngineInterface, mem: Mem, found: readonly Crossing[]) {
  const fresh = found.filter(c => !mem.asked.has(c.key))
  if (fresh.length === 0) return
  const prompt = mem.asked.size === 0 || mem.status === 'handed-off'
  for (const c of fresh) mem.asked.add(c.key)
  if (prompt) mem.status = 'asked'
  await save($, mem)
  if (prompt) void $.prompt.submit({ text: message(fresh[0]!.why) })
}

export const register: Register = (on) => {
  const mem: Mem = { id: undefined, loading: undefined, polling: false, shares: undefined, cwd: '', asked: new Set(), status: 'asked', line: undefined }
  on('session.start', async ($, e, next) => {
    mem.cwd = e.cwd
    await sync($, mem)
    await $.command.register({ name: 'handoff-lines', description: 'Print the resume line of each session on this account that was told to hand off' })
    if (mem.polling) return next(e)
    mem.polling = true
    $.clock.every(POLL_MS, async () => {
      await sync($, mem)
      const trigger = await readJson<Trigger>($, `${await dir($)}/trigger.json`)
      if (trigger === undefined) return
      const now = await $.clock.now()
      const expires = trigger.resetsAt === undefined ? Date.parse(trigger.at) + NO_RESET_MS : Date.parse(trigger.resetsAt)
      if (!(expires > now)) return
      await ask($, mem, [{ key: trigger.key, why: trigger.why }])
    })
    return next(e)
  })

  on('session.measure', async ($, e, next) => {
    await sync($, mem)
    const shares = await loadShares($, mem)
    // A context share the session fell back under (after /compact) can be crossed and asked about again.
    if (mem.asked.has('context') && e.context.percent !== undefined && e.context.percent < shares.context) mem.asked.delete('context')
    const fresh = crossings(e, shares).filter(c => !mem.asked.has(c.key))
    for (const c of fresh) {
      if (c.window === undefined) continue
      const trigger: Trigger = { key: c.key, why: c.why, at: iso(await $.clock.now()), ...(c.window.resetsAt === undefined ? {} : { resetsAt: c.window.resetsAt }) }
      try {
        await $.fs.write(`${await dir($)}/trigger.json`, JSON.stringify(trigger))
      } catch (error) {
        $.ui.log(`handoff: could not write the trigger: ${String(error)}`)
      }
    }
    await ask($, mem, fresh)
    return next(e)
  })

  // The first turn of a told session that ends with a handoff line is its handoff; an aborted turn is not.
  on('turn.complete', async ($, e, next) => {
    const result = await next(e)
    await sync($, mem)
    const handoff = handoffLine(e.answer)
    if (e.agentId !== undefined || e.isAborted || mem.status !== 'asked' || mem.asked.size === 0 || handoff === undefined) return result
    mem.status = 'handed-off'
    mem.line = resumeLine(await $.session.id(), mem.cwd, handoff)
    $.ui.log(`handoff: ${mem.line}`)
    await save($, mem)
    return result
  })

  on('command.run', { command: 'handoff-lines' }, async ($, e, next) => {
    const folder = `${await dir($)}/sessions`
    let names: string[] = []
    try {
      names = (await $.fs.list(folder)).filter(entry => entry.name.endsWith('.json') && SESSION_ID.test(entry.name.slice(0, -5))).sort((a, b) => a.mtimeMs - b.mtimeMs).map(entry => entry.name)
    } catch {
      // no session was told yet
    }
    const lines: string[] = []
    for (const name of names.slice(-MAX_LINES)) {
      const session = await readJson<Session>($, `${folder}/${name}`)
      lines.push(session?.line ?? `${resumeLine(name.slice(0, -5), session?.cwd ?? '.')}  # not handed off yet`)
    }
    if (names.length > MAX_LINES) lines.push(`(${names.length - MAX_LINES} more session files not shown)`)
    return { text: lines.length === 0 ? 'No session has been told to hand off.' : lines.join('\n') }
  })
}
