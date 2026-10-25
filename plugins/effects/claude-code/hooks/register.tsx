import { atom, read, update } from 'claude-code'
import type { EngineInterface, Register } from 'claude-code'

import type { Effect, Pending } from '../types'
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

type Answer = 'approve' | 'refuse'

const board = ($: EngineInterface, args: string[], timeoutMs = READ_MS) =>
  $.process.run(['/bin/sh', '-c', RUN, 'board', '--json', ...args], { timeoutMs })

/** Reads the pending effects, or the one with `id`; a string says why the read failed. */
async function query($: EngineInterface, id?: string): Promise<Omit<Pending, 'failed'> | string> {
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
      ? { effects: last?.effects ?? [], truncated: last?.truncated ?? false, failed: found }
      : { ...found, failed: null },
  )
}

/** Runs `board answer`; true when Board recorded it, else the owner is told what is known. */
async function answer($: EngineInterface, e: Effect, said: Answer): Promise<boolean> {
  const ran = await board($, ['answer', e.id, said], ANSWER_MS).catch(() => null)
  if (ran?.exitCode === 0) {
    await update($, pending, found => found && { ...found, effects: found.effects.filter(o => o.id !== e.id) })
    $.ui.toast(said === 'approve' ? `Approved: ${headline(e)}` : `Refused: ${headline(e)}`)
    return true
  }
  // Only Board's own failure report says nothing was written. A run that
  // timed out or was killed may have committed, so the list is read again.
  const refused = ran === null ? null : boardFailure(ran.stdout)
  if (refused !== null) {
    $.ui.toast(`No answer was recorded: ${visible(refused)}`)
  } else {
    $.ui.toast('Whether your answer was recorded is unknown, because board answer did not finish. The list is read again.')
    await refresh($)
  }
  return false
}

/**
 * Asks the owner about each effect the output names that Board still holds
 * pending and `asked` does not hold, and returns what the agent should read
 * about each answer. The dialog shows the effect as Board recorded it, never
 * the output's words.
 */
async function askOwner($: EngineInterface, output: string, asked: Set<string>): Promise<string[]> {
  const notes: string[] = []
  for (const id of askedIds(output)) {
    if (asked.has(id)) continue
    asked.add(id)
    const found = await query($, id)
    const effect = typeof found === 'string' ? undefined : found.effects[0]
    if (effect === undefined) continue
    let chosen: string
    try {
      // Later comes first, so a stray Enter answers nothing.
      chosen = await $.ui.ask(question(effect), { header: 'Board', options: ['Later', 'Refuse', 'Approve'] })
    } catch {
      continue // dismissed, or no one to ask: the effect stays pending
    }
    const said = chosen === 'Approve' ? 'approve' : chosen === 'Refuse' ? 'refuse' : null
    if (said !== null && (await answer($, effect, said))) notes.push(told(effect, said))
  }
  return notes
}

export const register: Register = on => {
  // The ids the dialog asked about, so each is asked once while the module
  // is loaded, and parallel Bash calls that name one id open one dialog.
  const asked = new Set<string>()

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
      const notes = await askOwner($, outputText(ran.result), asked)
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
              <Button key={`${effect.id}#refuse`} label="Refuse" onPress={() => answer($, effect, 'refuse')} />
              <Button key={`${effect.id}#approve`} label="Approve" onPress={() => answer($, effect, 'approve')} />
            </Box>
          </Box>
        ))}
        {found.truncated ? <Text dimColor>More wait; `board --personal query` lists them.</Text> : null}
      </Box>
    )
  })
}
