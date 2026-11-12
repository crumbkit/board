// What `board call attention.pinned` answered, and the words a session and the
// owner read from it. Pure functions, so the tests can pin every word.

import type { LeftOut, Order, Pinned } from '../types'

/** The name of the context block the session's first message carries. */
export const BLOCK = 'attentionOrders'

/** Board's failure code when there is no personal board, which the mod does not report. */
export const NO_BOARD = 'board::config::not_found'

/** Reads `board --personal --json call attention.pinned`'s exit status and output. */
export function parsePinned(board: string, exitCode: number, stdout: string, stderr: string): Pinned {
  const report = parseJson(stdout) ?? parseJson(stderr)
  if (exitCode === 0 && report?.schema === 'board/call-report/v2') {
    const output = (report.data as { output?: { orders?: Order[]; left_out?: LeftOut[] } } | undefined)?.output
    if (Array.isArray(output?.orders) && Array.isArray(output.left_out)) {
      return { kind: 'read', board, orders: output.orders, leftOut: output.left_out }
    }
  }
  if (report?.code === NO_BOARD) return { kind: 'none' }
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
  if (pinned.kind === 'none') return null
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
      'session started. Each one binds you until it expires. When you need to interpret an order, read its ' +
      'non-binding context with `board --personal attention verify --path <order>`.',
    ...pinned.orders.map(d => `## ${d.path}, until ${d.expires}\n\n${d.order}`),
  ]
  if (pinned.leftOut.length > 0) {
    parts.push(
      '## Pinned orders not shown\n\n' + pinned.leftOut.map(l => `- ${l.path}: ${l.code}. ${l.message}`).join('\n'),
    )
  }
  return parts.join('\n\n')
}

/** An order's name: the last segment of its path, so `/orders/evals` is `evals`. */
export const orderName = (path: string) => path.split('/').filter(Boolean).at(-1) ?? path

/** The band's Orders button, or null when there is nothing to show; `problem` draws it in red. */
export type OrdersButton = { label: string; problem: boolean }

export function ordersButton(pinned: Pinned): OrdersButton | null {
  if (pinned.kind === 'none') return null
  if (pinned.kind === 'failed') return { label: `Orders: ${pinned.code}`, problem: true }
  const { orders, leftOut } = pinned
  if (orders.length === 0 && leftOut.length === 0) return null
  if (leftOut.length > 0) return { label: `Orders ${orders.length}: ${leftOut.length} left out`, problem: true }
  return { label: `Orders ${orders.length}`, problem: false }
}
