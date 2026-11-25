// The prompt log: a SHA-256 digest of each prompt the owner types in a
// session, and of each of its lines, in
// `$XDG_STATE_HOME/board/prompts/<session>` or
// `~/.local/state/board/prompts/<session>`, one `<unix seconds> <digest>`
// line each, kept 30 days. `plans.answer --session` marks an answer
// `heard` when the digest of its words is there. Only digests are kept, on
// this Mac, and the log is never synced.
//
// The log is a guardrail and no proof: an agent with a shell can append a
// digest, and a prompt the owner pasted from an agent counts as typed.

import type { EngineInterface } from 'claude-code'

/** How many days a digest is kept, as `board_model::PROMPT_DAYS` reads it. */
export const PROMPT_DAYS = 30

const DAY_SECONDS = 24 * 60 * 60

/** The hex SHA-256 of `text` with its leading and trailing whitespace trimmed. */
export async function digest(text: string): Promise<string> {
  const bytes = new TextEncoder().encode(text.trim())
  const hashed = new Uint8Array(await crypto.subtle.digest('SHA-256', bytes))
  return Array.from(hashed, byte => byte.toString(16).padStart(2, '0')).join('')
}

/** The digests a prompt adds: the whole prompt's, then each other line's that holds more than whitespace. */
export async function digests(text: string): Promise<string[]> {
  const pieces = [text, ...text.split('\n')].filter(piece => piece.trim() !== '')
  const found = await Promise.all(pieces.map(digest))
  return [...new Set(found)]
}

/** The lines of `log` from the last 30 days before `now`, in seconds; a line that does not read as `<seconds> <digest>` is dropped. */
export function kept(log: string, now: number): string[] {
  const since = now - PROMPT_DAYS * DAY_SECONDS
  return log.split('\n').filter(line => {
    const [at, hex, ...rest] = line.trim().split(/\s+/)
    return rest.length === 0 && /^[0-9a-f]{64}$/.test(hex ?? '') && Number(at) >= since
  })
}

/** What a prompt.submit hook reads to decide whether the owner typed the prompt. */
export type Submitted = {
  text: string
  origin: { kind: string }
  isMeta?: true
  command?: unknown
}

/** Whether the owner typed the prompt: at the terminal or through Remote Control, and not a command or a hidden prompt. */
export const typedByOwner = (e: Submitted) =>
  (e.origin.kind === 'composer' || e.origin.kind === 'bridge') && e.isMeta !== true && e.command === undefined

/** The directory of the prompt logs, or null when neither variable names one. */
async function directory($: EngineInterface): Promise<string | null> {
  const state = await $.env.get('XDG_STATE_HOME')
  if (state?.startsWith('/')) return `${state}/board/prompts`
  const home = await $.env.get('HOME')
  return home?.startsWith('/') ? `${home}/.local/state/board/prompts` : null
}

/**
 * Adds the digests of a prompt the owner typed to this session's log, and
 * drops those older than 30 days. A session with no `local_` id, which is
 * how `plans.answer` names a session, keeps no log. A failure is logged and
 * never stops the prompt.
 */
export async function logPrompt($: EngineInterface, e: Submitted): Promise<void> {
  if (!typedByOwner(e)) return
  try {
    const session = await $.env.get('CLAUDE_CODE_HOST_SESSION_ID')
    if (!session?.startsWith('local_') || !/^[A-Za-z0-9_-]+$/.test(session)) return
    const where = await directory($)
    if (where === null) return
    const file = `${where}/${session}`
    const now = Math.floor((await $.clock.now()) / 1000)
    const old = (await $.fs.exists(file)) ? await $.fs.read(file) : ''
    const added = (await digests(e.text)).map(hex => `${now} ${hex}`)
    await $.fs.write(file, [...kept(old, now), ...added].join('\n') + '\n')
  } catch (error) {
    $.ui.log(`board: the prompt log was not written: ${String(error)}`, { to: 'debug' })
  }
}
