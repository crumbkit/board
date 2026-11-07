import { atom, read, update } from 'claude-code'
import type { EngineInterface, Register } from 'claude-code'

import type { Pinned, Request } from '../types'
import { BLOCK, bandLine, contextText, parsePinned } from './pinned'

// The mod reads and signs orders in the owner's personal board through
// Board. The button only runs `board call attention.sign`, which opens the
// signing window; Touch ID signs, and nothing here can sign or approve.

const pending = atom({ plugin: 'board-attention', key: 'pending' } as const, [] as Request[])
const open = atom({ plugin: 'board-attention', key: 'open' } as const, null as string | null)
const pinned = atom({ plugin: 'board-attention', key: 'pinned' } as const, null as Pinned | null)

const TOOL = 'request_signature'
const BOARD = 'the personal board'
// Board lives in ~/.local/bin, which a session's PATH may lack. Each value
// reaches board as its own argument through "$@". A call with no flags
// reads stdin, so /dev/null keeps it from waiting.
const RUN = 'PATH="$HOME/.local/bin:/opt/homebrew/bin:/usr/local/bin:$PATH" exec board "$@" < /dev/null'
// attention.sign gives the owner 10 minutes, the longest a mod's process may
// run. A sign still running then is killed, and its outcome is unknown.
const SIGN_MS = 600_000
const PANE = 'attention-pinned'

const board = ($: EngineInterface, args: string[], timeoutMs: number) =>
  $.process.run(['/bin/sh', '-c', RUN, 'board', '--personal', '--json', ...args], { timeoutMs })

/** What happened when the owner closed the signing window. */
export type Outcome =
  | { kind: 'signed'; message: string }
  | { kind: 'changes'; message: string }
  | { kind: 'cancelled' }
  | { kind: 'unknown'; message: string }
  | { kind: 'failed'; message: string }

/**
 * The owner's message when the sign process ended without an answer, because
 * it was killed or ran past the mod's limit. The signature may have been written.
 */
export const unknownOutcome = (request: Request) =>
  `The outcome of signing ${request.path} in ${BOARD} is unknown, because the sign process ended without an ` +
  `answer. Check it with \`board --personal call attention.verify --path '${request.path}'\`.`

/** Board's failure document on stdout, if it printed one. */
function failure(stdout: string): { code: string; message: string; fields: Record<string, unknown> } | null {
  try {
    const value = JSON.parse(stdout) as { code?: unknown; message?: unknown; fields?: unknown }
    if (typeof value.code !== 'string' || typeof value.message !== 'string') return null
    const fields = typeof value.fields === 'object' && value.fields !== null ? (value.fields as Record<string, unknown>) : {}
    return { code: value.code, message: value.message, fields }
  } catch {
    return null
  }
}

/** Reads `board --json call attention.sign`'s exit status and output as what the owner decided. */
export function outcome(request: Request, exitCode: number, stdout: string, stderr: string): Outcome {
  const where = `${request.path} in ${BOARD}`
  if (exitCode === 0) {
    return {
      kind: 'signed',
      message: `The owner signed the order ${where}. Agents can check it with \`board --personal call attention.verify --path '${request.path}'\`.`,
    }
  }
  const failed = failure(stdout)
  if (failed?.code === 'attention::sign::declined') {
    const feedback = typeof failed.fields.feedback === 'string' ? failed.fields.feedback : failed.message
    return {
      kind: 'changes',
      message:
        `The owner did not sign the order ${where} and asked for these changes:\n\n${feedback.trim()}\n\n` +
        `Revise the draft, then call ${TOOL} again.`,
    }
  }
  if (failed?.code === 'attention::sign::cancelled') return { kind: 'cancelled' }
  // Board prints a failure document for every failure under --json, and a
  // child ended by a signal reads as exit status 1, so exit 1 without one
  // means the process was killed.
  if (failed === null && exitCode === 1) return { kind: 'unknown', message: unknownOutcome(request) }
  const why = failed === null ? `exit status ${exitCode}:\n\n${(stderr.trim() || stdout.trim()).slice(0, 2000)}` : `${failed.code}: ${failed.message}`
  return { kind: 'failed', message: `Signing the order ${where} failed with ${why}` }
}

/** Runs attention.pinned, which verifies each pinned order, and keeps its answer for the session. */
async function readPinned($: EngineInterface): Promise<Pinned> {
  let answer: Pinned
  try {
    // pinned checks each signature, so a slow board holds the session start no
    // longer than this.
    const ran = await board($, ['call', 'attention.pinned'], 10000)
    answer = parsePinned(BOARD, ran.exitCode, ran.stdout, ran.stderr)
  } catch (error) {
    answer = { kind: 'failed', board: BOARD, code: 'not run', message: String(error) }
  }
  await update($, pinned, () => answer)
  return answer
}

export const register: Register = on => {
  on('session.start', async ($, e, next) => {
    const started = await next(e)
    await $.tool.register({
      name: TOOL,
      description:
        'Asks the owner to review and sign an attention order you drafted in the personal board. A button ' +
        "appears above the owner's prompt; it opens the signing window, where the owner signs with Touch ID or " +
        'requests changes. This returns at once. The decision arrives later as a new message, so do not ' +
        'wait or poll for it. Write the order and its context file first.',
      inputSchema: {
        type: 'object',
        properties: {
          path: { type: 'string', description: "The order record's path in the personal board, such as /orders/evals." },
        },
        required: ['path'],
        additionalProperties: false,
      },
      isDeferred: false,
    })
    await readPinned($)
    return started
  })

  on('tool.call', { tool: 'mcp__board-attention__request_signature' }, async ($, e) => {
    const request = { path: String(e.path ?? '') }
    if (!request.path.startsWith('/')) {
      return { deny: `${TOOL}: path must be absolute in the personal board, as /orders/evals.` }
    }
    await update($, pending, list => [...list.filter(r => r.path !== request.path), request])
    return {
      result: `Asked the owner to review ${request.path} in ${BOARD}. Their decision arrives as a new message.`,
    }
  })

  // One block, which the engine sends in the first request and again after a
  // compaction. It reads what session.start read; a session whose start
  // came before this module loaded reads it here.
  on('prompt.context', async ($, e, next) => {
    const context = await next(e)
    const answer = (await read($, pinned)) ?? (await readPinned($))
    const text = contextText(answer)
    if (text === null) return context
    return { ...context, blocks: [...context.blocks.filter(b => b.name !== BLOCK), { name: BLOCK, text }] }
  })

  on('ui.render', { component: 'AbovePrompt' }, async ($, e, next) => {
    const below = await next(e)
    if (e.props.hasSurvey) return below
    const { Box, Button, Text } = $.ui.resolve(e)
    const answer = await read($, pinned)
    const line = answer === null ? null : bandLine(answer)
    const list = await read($, pending)
    const first = list[0]
    const opened = await read($, open)
    if (line === null && first === undefined) return below

    const done = async (message?: string) => {
      if (first === undefined) return
      await update($, pending, l => l.filter(r => r.path !== first.path))
      if (message !== undefined) await $.prompt.submit({ text: message })
    }

    const review = async () => {
      if (first === undefined) return
      await update($, open, () => first.path)
      try {
        const ran = await board($, ['call', 'attention.sign', '--path', first.path], SIGN_MS)
        const decided = outcome(first, ran.exitCode, ran.stdout, ran.stderr)
        if (decided.kind === 'cancelled') $.ui.toast(`Not signed: ${first.path} is still waiting.`)
        else if (decided.kind === 'unknown') $.ui.toast(decided.message)
        else await done(decided.message)
      } catch {
        // process.run rejects when the sign runs past SIGN_MS and is killed.
        // /bin/sh always starts, so this is the only rejection left.
        $.ui.toast(unknownOutcome(first))
      } finally {
        await update($, open, () => null)
      }
    }

    const dismiss = () =>
      first && done(`The owner dismissed the request to sign ${first.path} in ${BOARD}, without signing it.`)

    const more = list.length > 1 ? ` (and ${list.length - 1} more)` : ''
    const request =
      first === undefined ? null : opened !== null ? (
        <Box>
          <Text dimColor>The signing window for {opened} is open.</Text>
        </Box>
      ) : (
        <Box>
          <Text>
            Order {first.path} is waiting for your signature{more}.{' '}
          </Text>
          <Button key="review" label="Review and sign" variant="primary" onPress={review} />
          <Text> </Text>
          <Button key="dismiss" label="Dismiss" dimColor onPress={dismiss} />
        </Box>
      )

    return (
      <Box flexDirection="column">
        {line !== null && (
          <Box>
            {line.text !== '' && <Text>{line.text} </Text>}
            {line.problem !== null && <Text color="red">{line.problem} </Text>}
            <Button key="show-pinned" label="Show" onPress={() => $.ui.open({ id: PANE, title: 'Pinned orders' })} />
          </Box>
        )}
        {request}
        {below.type === 'engine' ? null : below}
      </Box>
    )
  })

  on('ui.render', { component: 'Pane', requestId: PANE }, async ($, e) => {
    const { Box, Text } = $.ui.resolve(e)
    const answer = await read($, pinned)
    if (answer === null) return <Text dimColor>The pinned orders have not been read yet.</Text>
    if (answer.kind === 'none') return <Text dimColor>There is no personal board, so nothing is pinned.</Text>
    if (answer.kind === 'failed') {
      return (
        <Box flexDirection="column">
          <Text color="red">board call attention.pinned on {answer.board} failed, so no order is in context.</Text>
          <Text>
            {answer.code}: {answer.message}
          </Text>
        </Box>
      )
    }
    return (
      <Box flexDirection="column">
        <Text dimColor>From {answer.board}. Each verified order below is in this session's context.</Text>
        {answer.orders.map(d => (
          <Box key={d.path} flexDirection="column" marginTop={1}>
            <Text bold>
              {d.path}, until {d.expires}
            </Text>
            <Text color="green">Verified: signed by {d.signer}.</Text>
            <Text>{d.order}</Text>
          </Box>
        ))}
        {answer.leftOut.map(l => (
          <Box key={l.path} flexDirection="column" marginTop={1}>
            <Text bold>{l.path}</Text>
            <Text color="red">
              Not in this session's context: {l.message} ({l.code}).
            </Text>
          </Box>
        ))}
      </Box>
    )
  })
}
