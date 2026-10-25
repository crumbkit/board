import { atom, read, update } from 'claude-code'
import type { EngineInterface, Register } from 'claude-code'

import type { Effect, Pending } from '../types'
import { askedIds, failure, headline, outputText, parsePending, pendingSql, preview, question, told, waitingLine } from './effects'

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
// A slow or hung board holds a refresh or an answer no longer than this.
const TIMEOUT_MS = 5000
const REFRESH_MS = 60_000
// The pane lists this many effects; past it the band says "or more".
const LISTED = 100

type Answer = 'approve' | 'refuse'

const board = ($: EngineInterface, args: string[]) =>
  $.process.run(['/bin/sh', '-c', RUN, 'board', '--json', ...args], { timeoutMs: TIMEOUT_MS })

const query = async ($: EngineInterface, id?: string): Promise<Pending | null> => {
  const ran = await board($, ['--personal', 'query', pendingSql(id), '--limit', String(LISTED)])
  const found = ran.exitCode === 0 ? parsePending(ran.stdout) : null
  if (found === null) $.ui.log(`board: pending effects could not be read: ${failure(ran.exitCode, ran.stdout, ran.stderr)}`, { to: 'debug' })
  return found
}

/** Reads the pending effects again. A failure clears them, so the band draws nothing. */
async function refresh($: EngineInterface) {
  let found: Pending | null = null
  try {
    found = await query($)
  } catch (error) {
    $.ui.log(`board: pending effects could not be read: ${String(error)}`, { to: 'debug' })
  }
  await update($, pending, () => found)
}

/** Runs `board answer`; true when Board recorded it, else the owner is told why. */
async function answer($: EngineInterface, e: Effect, said: Answer): Promise<boolean> {
  try {
    const ran = await board($, ['answer', e.id, said])
    if (ran.exitCode !== 0) {
      $.ui.toast(`No answer was recorded: ${failure(ran.exitCode, ran.stdout, ran.stderr)}`)
      return false
    }
    await update($, pending, found => found && { ...found, effects: found.effects.filter(o => o.id !== e.id) })
    $.ui.toast(said === 'approve' ? `Approved: ${headline(e)}` : `Refused: ${headline(e)}`)
    return true
  } catch (error) {
    $.ui.toast(`No answer was recorded: ${String(error)}`)
    return false
  }
}

/**
 * Asks the owner about each effect the output names that Board still holds
 * pending, and returns what the agent should read about each answer. The
 * dialog shows the effect as Board recorded it, never the output's words.
 */
async function askOwner($: EngineInterface, output: string): Promise<string[]> {
  const notes: string[] = []
  for (const id of askedIds(output)) {
    const effect = (await query($, id))?.effects[0]
    if (effect === undefined) continue
    let chosen: string
    try {
      chosen = await $.ui.ask(question(effect), { header: 'Board', options: ['Approve', 'Refuse', 'Later'] })
    } catch {
      continue // dismissed, or no one to ask: the effect stays pending
    }
    const said = chosen === 'Approve' ? 'approve' : chosen === 'Refuse' ? 'refuse' : null
    if (said !== null && (await answer($, effect, said))) notes.push(told(effect, said))
  }
  return notes
}

export const register: Register = on => {
  on('session.start', async ($, e, next) => {
    const started = await next(e)
    await refresh($)
    $.clock.every(REFRESH_MS, () => void refresh($))
    return started
  })

  // The command has run when `next` resolves, so nothing here may run it
  // again: a failure leaves the result as it was.
  on('tool.call', { tool: 'Bash' }, async ($, e, next) => {
    const ran = await next(e)
    if (ran.deny !== undefined) return ran
    try {
      const notes = await askOwner($, outputText(ran.result))
      if (notes.length === 0) return ran
      await refresh($)
      return { ...ran, context: [...(ran.context ?? []), ...notes] }
    } catch (error) {
      $.ui.log(`board: the asked effect was not shown: ${String(error)}`, { to: 'debug' })
      return ran
    }
  })

  on('ui.render', { component: 'AbovePrompt' }, async ($, e, next) => {
    const below = await next(e)
    const found = await read($, pending)
    if (e.props.hasSurvey || found === null || found.effects.length === 0) return below
    const { Box, Button, Text } = $.ui.resolve(e)
    const line = waitingLine(found.effects.length) + (found.truncated ? ' (or more)' : '')
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
    if (found === null || found.effects.length === 0) return <Text dimColor>No effect waits for your answer.</Text>
    return (
      <Box flexDirection="column">
        {found.effects.map(effect => (
          <Box key={effect.id} flexDirection="column" marginBottom={1}>
            <Text>{headline(effect)}</Text>
            <Text dimColor>{preview(effect)}</Text>
            <Box>
              <Button key={`${effect.id}#approve`} label="Approve" variant="primary" onPress={() => answer($, effect, 'approve')} />
              <Button key={`${effect.id}#refuse`} label="Refuse" onPress={() => answer($, effect, 'refuse')} />
            </Box>
          </Box>
        ))}
        {found.truncated ? <Text dimColor>More wait; `board --personal query` lists them.</Text> : null}
      </Box>
    )
  })
}
