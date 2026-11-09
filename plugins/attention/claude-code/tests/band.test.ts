import { expect, test } from 'claude-code/testing'
import type { RenderElement, RenderNode } from 'claude-code'

import { ROW, SLOT, joinBand } from '../hooks/band'

// These prove how a mod's part joins the band the mods beneath it drew. Each
// Board mod keeps an identical copy of band.ts, which the mods workflow
// compares with this mod's, so this test covers all four.

const part = (key: string, text: string): RenderElement => ({ type: 'Box', props: { key }, children: [text] })
const ENGINE: RenderElement = { type: 'engine', ref: 0 }

/** The row's children as their keys, a bar as `|`. */
function row(band: RenderElement): string[] {
  const first = band.type === 'Box' ? band.children?.[0] : undefined
  if (typeof first !== 'object' || first.type !== 'Box' || first.props?.key !== ROW) return []
  return (first.children ?? []).map((c: RenderNode) => (typeof c === 'object' && c.type === 'Box' ? String(c.props?.key) : typeof c === 'object' && c.type === 'Text' ? String(c.children?.[0]) : '?'))
}

test('with nothing to add the tree beneath comes back unchanged', () => {
  expect(joinBand(ENGINE, [])).toBe(ENGINE)
})

test('the parts sit in slot order whichever mod draws first, with a bar between sections', () => {
  const effects = joinBand(ENGINE, [part(SLOT.effects, 'Effects 2')])
  const state = joinBand(effects, [part(SLOT.state, 'Board personal')])
  const orders = joinBand(state, [part(SLOT.orders, 'Orders 4')])
  const all = joinBand(orders, [part(SLOT.controls, 'Controls 5'), part(SLOT.waiting, 'Questions 3')])
  expect(row(all)).toEqual([SLOT.state, '|', SLOT.orders, SLOT.controls, SLOT.waiting, SLOT.effects])
})

test('a part replaces the part of the same slot, so a mod drawn twice shows once', () => {
  const once = joinBand(ENGINE, [part(SLOT.orders, 'Orders 3')])
  const twice = joinBand(once, [part(SLOT.orders, 'Orders 4')])
  expect(row(twice)).toEqual([SLOT.orders])
})

test('the lines a mod adds follow the row, and a tree that is not a band is kept as a line after them', () => {
  const foreign: RenderElement = { type: 'Text', children: ['another plugin'] }
  const line: RenderElement = { type: 'Text', children: ['Order /orders/x is waiting for your signature.'] }
  const band = joinBand(foreign, [part(SLOT.controls, 'Controls 5')], [line])
  expect(row(band)).toEqual([SLOT.controls])
  expect(band.type === 'Box' ? band.children?.slice(1) : []).toEqual([line, foreign])
  const above = joinBand(band, [part(SLOT.state, 'Board personal')])
  expect(row(above)).toEqual([SLOT.state, '|', SLOT.controls])
  expect(above.type === 'Box' ? above.children?.slice(1) : []).toEqual([line, foreign])
})
