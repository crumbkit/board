// The band above the prompt: one row of badges that wraps, which each Board
// mod adds its own part to. The mods share nothing but this tree. Each mod
// keeps an identical copy of this file, because a mod installs alone; the
// mods workflow fails when a copy differs from the attention mod's.
//
// A part is a Box keyed `band:<slot>`. The row is the Box keyed `board-band`,
// the first child of the band's column; any other line a mod draws, such as
// a request to sign, follows the row in that column. A mod that draws reads
// the parts of the mods beneath it from `next(e)`'s tree, adds its own and
// returns the whole band, so the row holds every installed mod's part, in
// slot order, whichever mod runs first.
//
// The host asks each plugin for a band row of its own, names that plugin in
// `e.plugin`, and stacks the rows. So each mod draws its part into every
// Board mod's row, and `keepOneBand` leaves one of those rows on screen.

import type { On, RenderElement, RenderNode } from 'claude-code'

/** The row's key. */
export const ROW = 'board-band'

/**
 * Where a part sits, in key order. The letter is its section: a dim bar
 * separates the session's state from the buttons.
 */
export const SLOT = {
  state: 'band:a1-state',
  orders: 'band:b1-orders',
  controls: 'band:b2-controls',
  ready: 'band:b3-ready',
  waiting: 'band:b3-waiting',
  effects: 'band:b4-effects',
} as const

type Element = RenderElement & { type: 'Box' | 'Text'; props?: Record<string, string | number | boolean>; children?: RenderNode[] }

const isBox = (node: RenderNode | undefined): node is Element => typeof node === 'object' && node.type === 'Box'
const keyOf = (node: RenderNode | undefined) => (isBox(node) ? String(node.props?.key ?? '') : '')
const section = (key: string) => key.slice('band:'.length, 'band:'.length + 1)

/** The mods that draw the band. */
const MODS: readonly string[] = ['board-attention', 'board-effects', 'board-orders', 'board-plans', 'board-rules']

/** Whether a mod draws its part into the band row the host asks of `plugin`: only a Board mod's row. */
export const drawsInto = (plugin: string | undefined) => MODS.includes(plugin ?? '')

/** Each Box keyed as the band's row in `tree`, outermost first. */
export function bandRows(tree: RenderNode): Element[] {
  if (!isBox(tree)) return []
  if (keyOf(tree) === ROW) return [tree]
  return (tree.children ?? []).flatMap(bandRows)
}

const partsIn = (tree: RenderNode) => bandRows(tree).flatMap(r => (r.children ?? []).filter(n => keyOf(n).startsWith('band:'))).length

/**
 * `rows` with one band left: the row that holds the most parts, the earliest
 * of equals. The rows of other plugins pass unchanged.
 */
export function oneBand<R extends { tree: RenderNode }>(rows: readonly R[]): R[] {
  const bands = rows.filter(r => bandRows(r.tree).length > 0)
  const kept = bands.reduce<R | undefined>((best, r) => (best === undefined || partsIn(r.tree) > partsIn(best.tree) ? r : best), undefined)
  return rows.filter(r => r === kept || !bands.includes(r))
}

/** Hooks the band's arrangement so that it shows one band. Each mod hooks it; after the first there is one band left. */
export function keepOneBand(on: On) {
  on('ui.render', { component: 'AbovePromptList' }, ($, e, next) => next({ ...e, props: { ...e.props, rows: oneBand(e.props.rows) } }))
}

/** The parts and the other lines a band tree from `next(e)` holds; anything else is one other line. */
function split(below: RenderElement): { parts: Element[]; lines: RenderNode[] } {
  if (below.type === 'engine') return { parts: [], lines: [] }
  const row = isBox(below) ? below.children?.[0] : undefined
  if (keyOf(below) !== '' || keyOf(row) !== ROW || !isBox(row)) return { parts: [], lines: [below] }
  const parts = (row.children ?? []).filter((n): n is Element => keyOf(n).startsWith('band:'))
  return { parts, lines: (below as Element).children?.slice(1) ?? [] }
}

/**
 * The band with `mine` added to the parts `below` holds, a part of the same
 * slot replaced, and `lines` after the row. `below` comes back unchanged when
 * there is nothing to add.
 */
export function joinBand(below: RenderElement, mine: readonly RenderElement[], lines: readonly RenderNode[] = []): RenderElement {
  if (mine.length === 0 && lines.length === 0) return below
  const found = split(below)
  const added = mine.filter(isBox)
  const keys = new Set(added.map(keyOf))
  const parts = [...found.parts.filter(p => !keys.has(keyOf(p))), ...added].sort((a, b) => keyOf(a).localeCompare(keyOf(b)))
  const row: RenderNode[] = []
  parts.forEach((part, i) => {
    if (i > 0 && section(keyOf(part)) !== section(keyOf(parts[i - 1]))) {
      row.push({ type: 'Text', props: { dimColor: true }, children: ['|'] })
    }
    row.push(part)
  })
  const rest = [...lines, ...found.lines]
  return {
    type: 'Box',
    props: { flexDirection: 'column' },
    children: [{ type: 'Box', props: { key: ROW, flexDirection: 'row', flexWrap: 'wrap', columnGap: 1 }, children: row }, ...rest],
  }
}
