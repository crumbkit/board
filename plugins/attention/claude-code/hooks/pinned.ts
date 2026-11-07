// What `board call attention.pinned` answered, and the words a session and the
// owner read from it. Pure functions, so the tests can pin every word.

import type { LeftOut, Order, Pinned } from '../types'

/** The name of the context block the session's first message carries. */
export const BLOCK = 'attentionOrders'

/** Reads `board --personal --json call attention.pinned`'s exit status and output. */
export function parsePinned(board: string, exitCode: number, stdout: string, stderr: string): Pinned {
  const report = parseJson(stdout) ?? parseJson(stderr)
  if (exitCode === 0 && report?.schema === 'board/call-report/v2') {
    const output = (report.data as { output?: { orders?: Order[]; left_out?: LeftOut[] } } | undefined)?.output
    if (Array.isArray(output?.orders) && Array.isArray(output.left_out)) {
      return { kind: 'read', board, orders: output.orders, leftOut: output.left_out }
    }
  }
  if (typeof report?.code === 'string' && typeof report.message === 'string') {
    return { kind: 'failed', board, code: report.code, message: report.message }
  }
  const text = (stderr.trim() || stdout.trim()).slice(0, 2000)
  return { kind: 'failed', board, code: `exit ${exitCode}`, message: text || 'board printed nothing' }
}

function parseJson(text: string): Record<string, unknown> | null {
  try {
    const value: unknown = JSON.parse(text)
    return typeof value === 'object' && value !== null ? (value as Record<string, unknown>) : null
  } catch {
    return null
  }
}

/** The context block's text, or `null` when nothing is pinned. */
export function contextText(pinned: Pinned): string | null {
  if (pinned.kind === 'failed') {
    return (
      `The owner may have pinned orders for this session, but \`board call attention.pinned\` on ` +
      `${pinned.board} failed with ${pinned.code}. Its message follows as board printed it.\n\n` +
      '```\n' + pinned.message + '\n```'
    )
  }
  if (pinned.orders.length === 0 && pinned.leftOut.length === 0) return null
  const parts = [
    'The owner signed each order below with Touch ID, and `board call attention.verify` checked it as this ' +
      'session started. Each one binds you until it expires. Its context is non-binding background that says ' +
      'what the owner meant.',
    ...pinned.orders.map(
      d => `## ${d.path}, until ${d.expires}\n\n${d.order}\n\nContext (non-binding):\n\n${d.context_text}`,
    ),
  ]
  if (pinned.leftOut.length > 0) {
    parts.push(
      '## Pinned orders not shown\n\n' + pinned.leftOut.map(l => `- ${l.path}: ${l.code}. ${l.message}`).join('\n'),
    )
  }
  return parts.join('\n\n')
}

/** The band's line, or `null` when it has nothing to say. `problem` is drawn as a warning. */
export function bandLine(pinned: Pinned): { text: string; problem: string | null } | null {
  if (pinned.kind === 'failed') {
    return { text: '', problem: `Pinned orders could not be read: ${pinned.message}` }
  }
  const shown = pinned.orders.length
  const failed = pinned.leftOut.length
  if (shown === 0 && failed === 0) return null
  const text = shown === 1 ? '1 signed order binds this session.' : `${shown} signed orders bind this session.`
  const problem =
    failed === 0
      ? null
      : failed === 1
        ? '1 pinned order is not in context.'
        : `${failed} pinned orders are not in context.`
  return { text, problem }
}
