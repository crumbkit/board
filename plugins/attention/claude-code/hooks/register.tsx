import { atom, read, update } from 'claude-code'
import type { EngineInterface, Register, RenderElement } from 'claude-code'

import type { Pinned, Request } from '../types'
import { SLOT, drawsInto, joinBand, keepOneBand } from './band'
import { BLOCK, contextText, ordersButton, orderName, parsePinned } from './pinned'

// The mod reads and signs orders in the owner's personal board through
// Board. The button only runs `board call attention.sign`, which opens the
// signing window; Touch ID signs, and nothing here can sign or approve.

const pending = atom({ plugin: 'board-attention', key: 'pending' } as const, [] as Request[])
const open = atom({ plugin: 'board-attention', key: 'open' } as const, null as string | null)
const pinned = atom({ plugin: 'board-attention', key: 'pinned' } as const, null as Pinned | null)
const selected = atom({ plugin: 'board-attention', key: 'selected' } as const, null as string | null)
const showContext = atom({ plugin: 'board-attention', key: 'showContext' } as const, false)
const replacedBy = atom({ plugin: 'board-attention', key: 'replaced' } as const, false)

/** The command that installs the mod that replaces this one. */
export const INSTALL = 'claude plugin install board-orders@board'
/** The command that removes this mod once board-orders is installed. */
export const UNINSTALL = 'claude plugin uninstall board-attention@board'

/**
 * Whether `enabled`, the settings' `enabledPlugins`, turns on board-orders,
 * which replaces this mod. While it does, this mod adds no context and
 * registers no tool, so a session sees each order once.
 */
export function replaced(enabled: unknown): boolean {
  if (typeof enabled !== 'object' || enabled === null) return false
  return Object.entries(enabled).some(([id, on]) => id.split('@')[0] === 'board-orders' && on === true)
}

/** The band's line about the replacement: install it, or, once it is on, uninstall this mod. */
export const replacementLine = (isReplaced: boolean) =>
  isReplaced
    ? `board-orders now shows the pinned orders; remove this mod with \`${UNINSTALL}\`.`
    : `board-orders replaces this mod: run \`${INSTALL}\`, then \`${UNINSTALL}\`.`

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
  // Board names these codes orders:: since the plugin's rename, and
  // attention:: before it.
  if (failed?.code === 'orders::sign::declined' || failed?.code === 'attention::sign::declined') {
    const feedback = typeof failed.fields.feedback === 'string' ? failed.fields.feedback : failed.message
    return {
      kind: 'changes',
      message:
        `The owner did not sign the order ${where} and asked for these changes:\n\n${feedback.trim()}\n\n` +
        `Revise the draft, then call ${TOOL} again.`,
    }
  }
  if (failed?.code === 'orders::sign::cancelled' || failed?.code === 'attention::sign::cancelled') return { kind: 'cancelled' }
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
    const isReplaced = replaced(await $.settings.read({ key: 'enabledPlugins' }))
    await update($, replacedBy, () => isReplaced)
    if (isReplaced) return started
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
    if (await read($, replacedBy)) return context
    const answer = (await read($, pinned)) ?? (await readPinned($))
    const text = contextText(answer)
    if (text === null) return context
    return { ...context, blocks: [...context.blocks.filter(b => b.name !== BLOCK), { name: BLOCK, text }] }
  })

  // The host asks each plugin for its own row; this mod draws into every Board mod's.
  keepOneBand(on)
  on('ui.render', { component: 'AbovePrompt', plugin: '*' }, async ($, e, next) => {
    const below = await next(e)
    if (e.props.hasSurvey || !drawsInto(e.plugin)) return below
    const { Box, Button, Text } = $.ui.resolve(e)
    const isReplaced = await read($, replacedBy)
    const notice = (
      <Box key="replacement">
        <Text dimColor>{replacementLine(isReplaced)}</Text>
      </Box>
    ) as RenderElement
    if (isReplaced) return joinBand(below, [], [notice])
    const answer = await read($, pinned)
    const button = answer === null ? null : ordersButton(answer)
    const list = await read($, pending)
    const first = list[0]
    const opened = await read($, open)
    if (button === null && first === undefined) return below

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

    // The Orders button opens the list of pinned orders.
    const show = async () => {
      await update($, selected, () => null)
      await update($, showContext, () => false)
      await $.ui.open({ id: PANE, title: 'Pinned orders' })
    }

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

    const part =
      button === null ? null : (
        <Box key={SLOT.orders}>
          <Button key="orders" label={button.label} onPress={show}>
            {button.problem ? <Text color="red">{button.label}</Text> : button.label}
          </Button>
        </Box>
      )

    const lines = [...(request === null ? [] : [request as RenderElement]), notice]
    return joinBand(below, part === null ? [] : [part as RenderElement], lines)
  })

  on('ui.render', { component: 'Pane', requestId: PANE }, async ($, e) => {
    const { Box, Button, Text } = $.ui.resolve(e)
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
    const chosen = await read($, selected)
    const order = answer.orders.find(d => d.path === chosen)
    const all = (
      <Button key="all-orders" label="All pinned orders" dimColor onPress={() => update($, selected, () => null)} />
    )
    if (order !== undefined) {
      const shown = await read($, showContext)
      return (
        <Box flexDirection="column">
          <Text bold>{orderName(order.path)}</Text>
          <Text dimColor>
            {order.path}, until {order.expires}
          </Text>
          <Text color="green">Verified: signed by {order.signer}.</Text>
          <Box marginTop={1}>
            <Text>{order.order}</Text>
          </Box>
          <Box marginTop={1}>
            <Button
              key="context"
              label={shown ? 'Hide context' : 'Show context'}
              onPress={() => update($, showContext, v => !v)}
            />
            <Text> </Text>
            {all}
          </Box>
          {shown && (
            <Box marginTop={1} flexDirection="column">
              <Text dimColor>Context (non-binding), from {order.context}:</Text>
              <Text>{order.context_text}</Text>
            </Box>
          )}
        </Box>
      )
    }
    return (
      <Box flexDirection="column">
        <Text dimColor>From {answer.board}. Each verified order below is in this session's context.</Text>
        {answer.orders.map(d => (
          <Box key={d.path} flexDirection="column" marginTop={1}>
            <Button
              key={`order:${d.path}`}
              label={`${d.path}, until ${d.expires}`}
              onPress={async () => {
                await update($, selected, () => d.path)
                await update($, showContext, () => false)
              }}
            />
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
