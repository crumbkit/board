import { atom, read, update } from 'claude-code'
import type { EngineInterface, Register } from 'claude-code'

import type { Effect, Pending } from '../types'
import type { Listed } from './effects'
import { askedIds, boardFailure, failure, headline, outputText, parsePending, pendingSql, preview, question, told, visible, waitingLine } from './effects'

// The mod lists the owner's pending effects from the personal board and
// answers one with `board answer`. Board makes every decision and checks
// every answer; the mod only draws and runs the command. It offers no way to
// save a rule: `board answer --save-rule` asks for no Touch ID yet, and
// plan/board-ui.md step 4 adds that signing to Board first.

const pending = atom({ plugin: 'board-effects', key: 'pending' } as const, null as Pending | null)

const PANE = 'effects'
// Board lives in ~/.local/bin, which a session's PATH may lack. Each value
// reaches board as its own argument through "$@".
const RUN = 'PATH="$HOME/.local/bin:/opt/homebrew/bin:/usr/local/bin:$PATH" exec board "$@"'
// A slow or hung board holds a refresh no longer than this.
const READ_MS = 5000
// An answer waits up to 5 s for the personal board's lock and then commits,
// so it has longer before its outcome is called unknown.
const ANSWER_MS = 30_000
const REFRESH_MS = 60_000
// The pane lists this many effects; past it the band says "or more".
const LISTED = 100
// The failure when there is no personal board, which the band does not report.
const NO_BOARD = 'board::config::not_found'

type Answer = 'approve' | 'refuse'

/** What the module keeps between calls while it is loaded. */
type Local = {
  /** The ids a dialog has asked about, so each is asked once. */
  asked: Set<string>
  /** The ids whose dialog is being prepared or is open, so parallel calls open one. */
  opening: Set<string>
  /** The ids whose answer is running, so a second press runs nothing. */
  answering: Set<string>
}

const board = ($: EngineInterface, args: string[], timeoutMs = READ_MS) =>
  $.process.run(['/bin/sh', '-c', RUN, 'board', '--json', ...args], { timeoutMs })

/** Reads the pending effects, or the one with `id`; a string says why the read failed. */
async function query($: EngineInterface, id?: string): Promise<Listed | string> {
  try {
    const ran = await board($, ['--personal', 'query', pendingSql(id), '--limit', String(LISTED)])
    if (ran.isStdoutTruncated) return 'board printed more than the 4 MiB a process run keeps'
    const found = ran.exitCode === 0 ? parsePending(ran.stdout, await $.env.get('HOME')) : null
    return found ?? failure(ran.exitCode, ran.stdout, ran.stderr)
  } catch (error) {
    return `board did not finish: ${String(error)}`
  }
}

/** Reads the pending effects again. A failure keeps the last list and says why. */
async function refresh($: EngineInterface) {
  const found = await query($)
  if (typeof found === 'string') $.ui.log(`board: pending effects could not be read: ${found}`, { to: 'debug' })
  await update($, pending, last =>
    typeof found === 'string'
      ? { effects: last?.effects ?? [], truncated: last?.truncated ?? false, failed: found, worked: last?.worked ?? false }
      : { ...found, failed: null, worked: true },
  )
}

/**
 * Runs `board answer`; true when Board recorded it, else the owner is told
 * what is known. A second press while it runs runs nothing. The list is read
 * again afterwards, whatever the outcome.
 */
async function answer($: EngineInterface, local: Local, e: Effect, said: Answer): Promise<boolean> {
  if (local.answering.has(e.id)) {
    $.ui.toast(`An answer to this effect is already running: ${headline(e)}`)
    return false
  }
  local.answering.add(e.id)
  try {
    return await answerOnce($, e, said)
  } finally {
    local.answering.delete(e.id)
    await refresh($)
  }
}

/**
 * Reads the effect again and answers it if it is still pending. `board
 * answer` overwrites a seen record, so a stale row or dialog could otherwise
 * turn an answer given elsewhere into this one. The re-read is a stop-gap:
 * it can go once Board refuses a second answer to an effect.
 */
async function answerOnce($: EngineInterface, e: Effect, said: Answer): Promise<boolean> {
  const now = await query($, e.id)
  if (typeof now === 'string') {
    $.ui.toast(`No answer was sent, because the effect could not be read again: ${visible(now)}`)
    return false
  }
  if (now.effects.length === 0) {
    $.ui.toast(`No answer was sent, because the effect is no longer pending: ${headline(e)}`)
    return false
  }
  const ran = await board($, ['answer', e.id, said], ANSWER_MS).catch(() => null)
  if (ran?.exitCode === 0) {
    $.ui.toast(said === 'approve' ? `Approved: ${headline(e)}` : `Refused: ${headline(e)}`)
    return true
  }
  // Only Board's own failure report says nothing was written. A run that
  // timed out or was killed may have committed.
  const refused = ran === null ? null : boardFailure(ran.stdout)
  $.ui.toast(
    refused !== null
      ? `Board recorded no answer: ${visible(refused)}`
      : 'Whether your answer was recorded is unknown, because board answer did not finish. The list is read again.',
  )
  return false
}

/**
 * Asks the owner about each effect the output names that Board still holds
 * pending and no dialog has asked about, and returns what the agent should
 * read about each. The dialog shows the effect as Board recorded it, never
 * the output's words. A read that fails leaves the ids to be asked next time.
 */
async function askOwner($: EngineInterface, local: Local, output: string): Promise<string[]> {
  const ids = askedIds(output).filter(id => !local.asked.has(id) && !local.opening.has(id))
  if (ids.length === 0) return []
  for (const id of ids) local.opening.add(id)
  const notes: string[] = []
  try {
    // One list read finds which ids Board holds pending, so output full of
    // ids Board does not hold costs one query, not one per id.
    const listed = await query($)
    if (typeof listed === 'string') {
      $.ui.log(`board: pending effects could not be read: ${listed}`, { to: 'debug' })
      return []
    }
    const held = new Set(listed.effects.map(e => e.id))
    for (const id of ids) {
      if (!listed.truncated && !held.has(id)) continue
      const found = await query($, id)
      if (typeof found === 'string') {
        $.ui.log(`board: pending effect ${id} could not be read: ${found}`, { to: 'debug' })
        continue
      }
      const effect = found.effects[0]
      if (effect === undefined) continue
      local.asked.add(id)
      let chosen: string
      try {
        // Later comes first, so a stray Enter answers nothing.
        chosen = await $.ui.ask(question(effect), { header: 'Board', options: ['Later', 'Refuse', 'Approve'] })
      } catch {
        continue // dismissed, or no one to ask: the effect stays pending
      }
      if (chosen === 'Later') notes.push(told(effect, 'later'))
      const said = chosen === 'Approve' ? 'approve' : chosen === 'Refuse' ? 'refuse' : null
      if (said !== null && (await answer($, local, effect, said))) notes.push(told(effect, said))
    }
  } finally {
    for (const id of ids) local.opening.delete(id)
  }
  return notes
}

export const register: Register = on => {
  const local: Local = { asked: new Set(), opening: new Set(), answering: new Set() }
  // The minute's refresh starts with the first band drawn, which a `-p` run
  // never draws, and it starts once however many sessions start.
  let polling = false

  on('session.start', async ($, e, next) => {
    const started = await next(e)
    void refresh($)
    return started
  })

  // The command has run when `next` resolves, so nothing here may run it
  // again: a failure leaves the result as it was.
  on('tool.call', { tool: 'Bash' }, async ($, e, next) => {
    const ran = await next(e)
    if (ran.deny !== undefined) return ran
    try {
      const notes = await askOwner($, local, outputText(ran.result))
      if (notes.length === 0) return ran
      return { ...ran, context: [...(ran.context ?? []), ...notes] }
    } catch (error) {
      $.ui.log(`board: the asked effect was not shown: ${String(error)}`, { to: 'debug' })
      return ran
    }
  })

  on('ui.render', { component: 'AbovePrompt' }, async ($, e, next) => {
    const below = await next(e)
    if (!polling) {
      polling = true
      $.clock.every(REFRESH_MS, () => void refresh($))
    }
    const found = await read($, pending)
    if (e.props.hasSurvey || found === null) return below
    // A failed read is marked, unless no read has worked because there is no
    // personal board.
    const unread = found.failed !== null && (found.worked || !found.failed.startsWith(NO_BOARD))
    if (found.effects.length === 0 && !unread) return below
    const { Box, Button, Text } = $.ui.resolve(e)
    const more = found.truncated ? ' or more' : ''
    const line = !unread
      ? waitingLine(found.effects.length) + (found.truncated ? ' (or more)' : '')
      : found.worked
        ? `Board: could not read pending effects (last read: ${found.effects.length}${more})`
        : 'Board: could not read pending effects'
    return (
      <Box flexDirection="column">
        {below.type === 'engine' ? null : below}
        <Box>
          <Text>{line} </Text>
          <Button key="show" label="Show" onPress={() => void $.ui.open({ id: PANE, title: 'Pending effects' })} />
        </Box>
      </Box>
    )
  })

  on('ui.render', { component: 'Pane', requestId: PANE }, async ($, e) => {
    const found = await read($, pending)
    const { Box, Button, Text } = $.ui.resolve(e)
    if (found === null) return <Text dimColor>The pending effects have not been read yet.</Text>
    const stale = found.effects.length === 0 ? '' : ' This list is from the last read that worked.'
    const failed =
      found.failed === null ? null : <Text color="red">{visible(`The pending effects could not be read: ${found.failed}.${stale}`)}</Text>
    if (found.effects.length === 0) return failed ?? <Text dimColor>No effect waits for your answer.</Text>
    return (
      <Box flexDirection="column">
        {failed}
        {found.effects.map(effect => (
          <Box key={effect.id} flexDirection="column" marginBottom={1}>
            <Text>{headline(effect)}</Text>
            <Text dimColor>{preview(effect)}</Text>
            <Box>
              <Button key={`${effect.id}#refuse`} label="Refuse" onPress={() => answer($, local, effect, 'refuse')} />
              <Button key={`${effect.id}#approve`} label="Approve" onPress={() => answer($, local, effect, 'approve')} />
            </Box>
          </Box>
        ))}
        {found.truncated ? <Text dimColor>More wait; `board --personal ls /effects/pending` lists them, with the effects already answered.</Text> : null}
      </Box>
    )
  })
}
