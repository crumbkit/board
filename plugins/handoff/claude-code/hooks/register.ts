import type { EngineInterface, Register } from 'claude-code'

// One mod per session, no daemon. Each session measures itself after every
// turn (`session.measure`) and polls a trigger file every 30 seconds, so a
// session that is idle when another one notices the account's usage is told
// too. The files sit in the account's Claude config directory, which every
// session on that account shares and no other account does.

/** The context limit in tokens, and the share of a usage window, in percent, at which a session is told to hand off. */
type Limits = { contextTokens: number; usage: number }
export type Crossing = { key: string; why: string; window?: { kind: string; resetsAt: string | undefined } }
type Reading = { context: { tokens?: number }; rateLimits: readonly { kind: string; percentUsed: number; resetsAt?: string }[] }
type Trigger = { key: string; why: string; resetsAt?: string | undefined; at: string }
type Session = { cwd: string; asked: string[]; status: 'asked' | 'handed-off'; line?: string }

// Used when the board gives no value; the calibrated values replace them.
export const DEFAULTS: Limits = { contextTokens: 280_000, usage: 85 }
// The key a context crossing is asked under. 0.1.1 and earlier used `context` for a share of the window, which load() drops.
const CONTEXT = 'context-tokens'
// One `params.get` takes about 0.36 seconds, too slow for every turn, so the values are read again only once this old.
const PARAMS_MS = 5 * 60_000
const POLL_MS = 30_000
// A trigger whose window gave no reset time stops telling sessions after the longest window.
const NO_RESET_MS = 5 * 3_600_000
const MAX_LINES = 20
// A window clears its trigger only this many points under its share. Each session's reading comes from its own last
// response, so one that lags by a point or two must not clear a real crossing; a usage reset falls far below the share.
const CLEAR_MARGIN = 10
const SESSION_ID = /^[A-Za-z0-9_-]+$/

/** A whole number with a comma every three digits: 280,000. */
const count = (n: number) => String(Math.round(n)).replace(/\B(?=(\d{3})+(?!\d))/g, ',')

/** Everything a measurement crosses: the context limit, then each usage window at or over its share. */
export function crossings(reading: Reading, limits: Limits): Crossing[] {
  const found: Crossing[] = []
  const tokens = reading.context.tokens
  if (tokens !== undefined && tokens >= limits.contextTokens) {
    found.push({ key: CONTEXT, why: `its context holds ${count(tokens)} tokens, at or over its limit of ${count(limits.contextTokens)}` })
  }
  for (const w of reading.rateLimits) {
    if (w.percentUsed >= limits.usage) found.push({ key: `${w.kind}@${w.resetsAt}`, why: `the ${w.kind} usage limit is ${w.percentUsed}% used`, window: { kind: w.kind, resetsAt: w.resetsAt } })
  }
  return found
}

/** The first crossing of a measurement, if any. */
export const crossing = (reading: Reading, limits: Limits): Crossing | undefined => crossings(reading, limits)[0]

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

type Mem = {
  id: string | undefined
  loading: Promise<void> | undefined
  polling: boolean
  /** The limits in force and when they were read; read again once older than PARAMS_MS. */
  limits: { value: Promise<Limits>; at: number } | undefined
  /** Whether the session was told that the params are unreadable; it is told once. */
  paramsLogged: boolean
  /** The transcript the last classic `Stop` named, read when a measurement has no `context.tokens`. */
  transcript: string | undefined
  /** Whether this session id was told that its transcript gave no count; it is told once. */
  transcriptLogged: boolean
  /** The session's own latest percentUsed of each usage window kind, held in memory only. */
  usage: Map<string, number>
  cwd: string
  asked: Set<string>
  status: Session['status']
  line: string | undefined
}

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

async function readParam($: EngineInterface, name: string): Promise<number | undefined> {
  try {
    const ran = await $.process.run(['board', '--personal', '--json', 'call', 'params.get', '--name', name])
    const value = Number(JSON.parse(ran.stdout)?.data?.output?.value)
    if (ran.exitCode === 0 && Number.isFinite(value) && value > 0) return value
  } catch {
    // fall through to the default
  }
  return undefined
}

// Reads the calibrated values again once the copy is over PARAMS_MS old, so a changed value reaches a running session
// without a restart; the defaults stand in for one the board lacks.
async function loadLimits($: EngineInterface, mem: Mem): Promise<Limits> {
  const now = await $.clock.now()
  if (mem.limits === undefined || now - mem.limits.at > PARAMS_MS) {
    mem.limits = {
      at: now,
      value: (async () => {
        const contextTokens = await readParam($, 'handoff-context-tokens')
        const usage = await readParam($, 'handoff-usage-share')
        if ((contextTokens === undefined || usage === undefined) && !mem.paramsLogged) {
          mem.paramsLogged = true
          $.ui.log(`handoff: params unreadable, using ${count(DEFAULTS.contextTokens)} context tokens and ${DEFAULTS.usage}% usage`)
        }
        return { contextTokens: contextTokens ?? DEFAULTS.contextTokens, usage: usage ?? DEFAULTS.usage }
      })(),
    }
  }
  return mem.limits.value
}

type TranscriptEntry = { type?: string; isSidechain?: boolean; message?: { usage?: Record<string, unknown> } }

/** The input tokens of the transcript's last main-thread assistant entry that reports usage: uncached, cache-read and cache-written. */
export function transcriptTokens(text: string): number | undefined {
  const lines = text.split('\n')
  for (let i = lines.length - 1; i >= 0; i--) {
    const line = lines[i]!.trim()
    if (line === '') continue
    let entry: TranscriptEntry
    try {
      entry = JSON.parse(line) as TranscriptEntry
    } catch {
      continue
    }
    const usage = entry.message?.usage
    if (entry.type !== 'assistant' || entry.isSidechain === true || typeof usage !== 'object' || usage === null) continue
    const part = (name: string) => (typeof usage[name] === 'number' ? (usage[name] as number) : 0)
    return part('input_tokens') + part('cache_read_input_tokens') + part('cache_creation_input_tokens')
  }
  return undefined
}

/** The session's context in tokens: the measurement's own count, else the last usage of the transcript `Stop` named. */
async function contextTokens($: EngineInterface, mem: Mem, reading: Reading): Promise<number | undefined> {
  if (reading.context.tokens !== undefined) return reading.context.tokens
  const path = mem.transcript
  if (path === undefined) return undefined
  let tokens: number | undefined
  try {
    tokens = transcriptTokens(await $.fs.read(path))
  } catch {
    tokens = undefined
  }
  if (tokens === undefined && !mem.transcriptLogged) {
    mem.transcriptLogged = true
    $.ui.log(`handoff: could not count the context from ${path}: no assistant usage in it, or it could not be read`)
  }
  return tokens
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
  // `context` is 0.1.1's key for a share of the window, which this version no longer measures.
  mem.asked = new Set((earlier?.asked ?? []).filter(key => key !== 'context'))
  mem.usage = new Map()
  mem.transcriptLogged = false
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

/**
 * Records each new crossing; tells the session to hand off when it has not been told yet, its last handoff is done,
 * or it reached its context limit, which it is told about even after a usage prompt.
 * Memory is updated before the write, so a failed write still holds the key and the prompt still goes.
 */
async function ask($: EngineInterface, mem: Mem, found: readonly Crossing[]) {
  const fresh = found.filter(c => !mem.asked.has(c.key))
  if (fresh.length === 0) return
  const told = mem.asked.size === 0 || mem.status === 'handed-off' ? fresh[0] : fresh.find(c => c.key === CONTEXT)
  for (const c of fresh) mem.asked.add(c.key)
  if (told !== undefined) mem.status = 'asked'
  await save($, mem)
  if (told !== undefined) void $.prompt.submit({ text: message(told.why) })
}

/** The time after which a trigger tells no session. */
const expiry = (trigger: Trigger) => (trigger.resetsAt === undefined ? Date.parse(trigger.at) + NO_RESET_MS : Date.parse(trigger.resetsAt))
/** The window kind a usage key names: `seven_day` in `seven_day@<resetsAt>`. */
const kindOf = (key: string): string => key.split('@')[0] ?? key

/**
 * For each usage window the reading puts CLEAR_MARGIN points under its share, expires a live trigger of that kind and re-arms the session.
 * A usage reset lowers percentUsed but keeps resetsAt, so the trigger would otherwise tell sessions until the old reset.
 * It matches on the kind alone, so a reset that also moves resetsAt clears the trigger too.
 */
async function rearm($: EngineInterface, mem: Mem, reading: Reading, limits: Limits) {
  const under = new Map(reading.rateLimits.filter(w => w.percentUsed < limits.usage - CLEAR_MARGIN).map(w => [w.kind, w.percentUsed]))
  if (under.size === 0) return
  const path = `${await dir($)}/trigger.json`
  const trigger = await readJson<Trigger>($, path)
  const now = await $.clock.now()
  if (typeof trigger?.key === 'string' && under.has(kindOf(trigger.key)) && expiry(trigger) > now) {
    const cleared: Trigger = { key: trigger.key, why: `the ${kindOf(trigger.key)} usage limit fell to ${under.get(kindOf(trigger.key))}% used`, resetsAt: iso(now), at: iso(now) }
    try {
      await $.fs.write(path, JSON.stringify(cleared))
    } catch (error) {
      $.ui.log(`handoff: could not clear the trigger: ${String(error)}`)
    }
  }
  const stale = [...mem.asked].filter(key => under.has(kindOf(key)))
  if (stale.length === 0) return
  for (const key of stale) mem.asked.delete(key)
  await save($, mem)
}

export const register: Register = (on) => {
  const mem: Mem = {
    id: undefined,
    loading: undefined,
    polling: false,
    limits: undefined,
    paramsLogged: false,
    transcript: undefined,
    transcriptLogged: false,
    usage: new Map(),
    cwd: '',
    asked: new Set(),
    status: 'asked',
    line: undefined,
  }
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
      if (!(expiry(trigger) > (await $.clock.now()))) {
        // An expired trigger re-arms its key, so an idle session is told again if the window crosses again.
        if (mem.asked.delete(trigger.key)) await save($, mem)
        return
      }
      // A session whose own latest reading of the window is under the share is not told: the trigger is stale for it.
      // Its next reading at or over the share tells it and writes a fresh trigger.
      const own = mem.usage.get(kindOf(trigger.key))
      if (own !== undefined && own < (await loadLimits($, mem)).usage) return
      await ask($, mem, [{ key: trigger.key, why: trigger.why }])
    })
    return next(e)
  })

  // The classic Stop input names the transcript, which a measurement without `context.tokens` is counted from.
  on('classic.Stop', async ($, e, next) => {
    if (e.agent_id === undefined && e.transcript_path !== '') mem.transcript = e.transcript_path
    return next(e)
  })

  on('session.measure', async ($, e, next) => {
    await sync($, mem)
    const limits = await loadLimits($, mem)
    for (const w of e.rateLimits) mem.usage.set(w.kind, w.percentUsed)
    const tokens = await contextTokens($, mem, e)
    const reading: Reading = { context: tokens === undefined ? {} : { tokens }, rateLimits: e.rateLimits }
    // A count back under the limit (after /compact) re-arms the context prompt.
    if (mem.asked.has(CONTEXT) && tokens !== undefined && tokens < limits.contextTokens) mem.asked.delete(CONTEXT)
    await rearm($, mem, reading, limits)
    const fresh = crossings(reading, limits).filter(c => !mem.asked.has(c.key))
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
