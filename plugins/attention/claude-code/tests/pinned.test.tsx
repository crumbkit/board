import { expect, test } from 'claude-code/testing'
import type { On } from 'claude-code'
import type { Engine } from 'claude-code/testing'

import { SLOT, joinBand } from '../hooks/band'
import { contextText, orderBadges, parsePinned } from '../hooks/pinned'

// These prove what a session and the owner are shown from attention.pinned's
// answer, with `board` stood in by a process.run hook. They do not run board
// or check a signature: board-cli's attention tests prove that pinned verifies.

const BOARD = 'the personal board'
const CWD = '/Users/luke/Code/board'
const EVALS = {
  path: '/orders/evals',
  signer: 'owner',
  order: 'Record every test as an eval.',
  expires: '2026-12-31',
  context: '/orders/evals.md',
  context_text: 'Luke asked for this on 2026-10-20.',
}
const REPORT = {
  schema: 'board/call-report/v2',
  data: {
    capability: 'pinned',
    plugin: 'attention',
    output: {
      orders: [EVALS],
      left_out: [
        { path: '/orders/old', code: 'attention::order::expired', message: 'the order expired on 2026-10-01' },
      ],
    },
  },
}
const PANE_PROPS = {
  title: 'Pinned orders',
  isFocused: false,
  bodyColumns: 80,
  placement: 'dock',
  scroll: { offset: 0, bodyRows: 40 },
  view: {},
} as const
const BAND_PROPS = {
  hasSurvey: false,
  isWorking: false,
  maxRows: 10,
  bodyColumns: 80,
  scroll: { offset: 0, bodyRows: 10 },
  view: {},
} as const

/** Stands in for the engine beneath the plugin, with `board` answering `answer`. */
function engine(on: On, answer: { exitCode: number; stdout: string; stderr?: string }, beneath?: 'controls') {
  const ran: (readonly string[])[] = []
  const opened: string[] = []
  on('process.run', (_, e) => {
    ran.push(e.argv)
    return { value: { stderr: '', ...answer, isStdoutTruncated: false, isStderrTruncated: false } }
  })
  on('tool.register', (_, e) => ({ value: { tool: `mcp__board-attention__${e.name}` } }))
  on('ui.open', (_, e) => {
    opened.push(e.id)
    return { value: { isPlaced: true } }
  })
  on('ui.render', ($, e) => {
    const { Box, Button } = $.ui.resolve(e)
    if (beneath === undefined || e.component !== 'AbovePrompt') return Box({})
    // Another mod's part, as the controls mod beneath this one draws it.
    const part = Box({ key: SLOT.controls, children: [Button({ key: 'controls', label: 'Controls 5', onPress: () => undefined })] })
    return joinBand(Box({}), [part])
  })
  on('prompt.context', (_, e) => ({ blocks: e.blocks }))
  on('session.start', (_, e) => ({ cwd: e.cwd }))
  return { ran, opened }
}

const start = ($: Engine) =>
  $.session.start({ cwd: CWD, surface: 'terminal', isInteractive: true })

test('the context block holds each verified order with its expiry, and names each one left out', () => {
  const pinned = parsePinned(BOARD, 0, JSON.stringify(REPORT), '')
  expect(contextText(pinned)).toBe(
    'The owner signed each order below with Touch ID, and `board call attention.verify` checked it as this ' +
      'session started. Each one binds you until it expires. Its context is non-binding background that says ' +
      'what the owner meant.\n\n' +
      '## /orders/evals, until 2026-12-31\n\nRecord every test as an eval.\n\n' +
      'Context (non-binding):\n\nLuke asked for this on 2026-10-20.\n\n' +
      '## Pinned orders not shown\n\n- /orders/old: attention::order::expired. the order expired on 2026-10-01',
  )
})

test('a board that refuses the call is reported with its code, and an empty answer adds no block', () => {
  const failure = { schema: 'board/failure/v1', code: 'attention::signers::missing', message: 'there is no allowed_signers file' }
  expect(contextText(parsePinned(BOARD, 1, JSON.stringify(failure), ''))).toBe(
    `The owner may have pinned orders for this session, but \`board call attention.pinned\` on ${BOARD} ` +
      'failed with attention::signers::missing. Its message follows as board printed it.\n\n' +
      '```\nthere is no allowed_signers file\n```',
  )
  const empty = { ...REPORT, data: { ...REPORT.data, output: { orders: [], left_out: [] } } }
  expect(contextText(parsePinned(BOARD, 0, JSON.stringify(empty), ''))).toBeNull()
})

test('session start reads the pinned orders once, and the first message carries them as one block', async ($, on) => {
  const { ran } = engine(on, { exitCode: 0, stdout: JSON.stringify(REPORT) })
  await start($)
  const context = await $.prompt.context({ blocks: [{ name: 'claudeMd', text: 'instructions' }] })

  expect(ran.length).toBe(1)
  expect(ran[0]?.slice(-5)).toEqual(['board', '--personal', '--json', 'call', 'attention.pinned'])
  expect(context.blocks.map(b => b.name)).toEqual(['claudeMd', 'attentionOrders'])
  expect(context.blocks[1]?.text).toContain('## /orders/evals, until 2026-12-31')
})

test('with nothing pinned the first message carries no block of the plugin\'s', async ($, on) => {
  const empty = { ...REPORT, data: { ...REPORT.data, output: { orders: [], left_out: [] } } }
  engine(on, { exitCode: 0, stdout: JSON.stringify(empty) })
  await start($)
  const context = await $.prompt.context({ blocks: [{ name: 'claudeMd', text: 'instructions' }] })
  expect(context.blocks.map(b => b.name)).toEqual(['claudeMd'])
})

test('each verified order is a badge named by its path\'s last segment, and each one left out a red badge with its code', () => {
  expect(orderBadges(parsePinned(BOARD, 0, JSON.stringify(REPORT), ''))).toEqual([
    { label: 'evals', path: '/orders/evals', problem: false },
    { label: 'old: attention::order::expired', path: '/orders/old', problem: true },
  ])
  const failure = { schema: 'board/failure/v1', code: 'attention::signers::missing', message: 'there is no allowed_signers file' }
  expect(orderBadges(parsePinned(BOARD, 1, JSON.stringify(failure), ''))).toEqual([
    { label: 'Pinned orders could not be read: there is no allowed_signers file', path: null, problem: true },
  ])
  const empty = { ...REPORT, data: { ...REPORT.data, output: { orders: [], left_out: [] } } }
  expect(orderBadges(parsePinned(BOARD, 0, JSON.stringify(empty), ''))).toEqual([])
})

test('the band shows a badge per order, and a badge opens its order with its expiry, signature and context', async ($, on) => {
  const { opened } = engine(on, { exitCode: 0, stdout: JSON.stringify(REPORT) })
  await start($)

  for (const surface of ['terminal', 'desktop'] as const) {
    const band = await $.ui.mount({ plugin: 'board-attention', surface, component: 'AbovePrompt', props: BAND_PROPS })
    expect((await band.find({ type: 'Text', text: 'Orders' }))?.text).toBe('Orders')
    expect((await band.find({ key: 'order:/orders/evals' }))?.text).toBe('evals')
    expect((await band.find({ key: 'order:/orders/old' }))?.text).toBe('old: attention::order::expired')
    await band.press({ key: 'order:/orders/evals' })
    await band.unmount()

    const pane = await $.ui.mount({
      plugin: 'board-attention',
      surface,
      component: 'Pane',
      requestId: 'attention-pinned',
      props: PANE_PROPS,
    })
    expect(await pane.find({ text: '/orders/evals, until 2026-12-31' })).toBeDefined()
    expect(await pane.find({ text: 'Verified: signed by owner.' })).toBeDefined()
    expect(await pane.find({ text: 'Record every test as an eval.' })).toBeDefined()
    expect(await pane.find({ text: 'Luke asked for this on 2026-10-20.' })).toBeUndefined()
    await pane.press({ key: 'context' })
    expect(await pane.find({ text: 'Context (non-binding), from /orders/evals.md:' })).toBeDefined()
    expect(await pane.find({ text: 'Luke asked for this on 2026-10-20.' })).toBeDefined()
    await pane.press({ key: 'all-orders' })
    expect(await pane.find({ text: /Not in this session's context: the order expired on 2026-10-01 \(attention::order::expired\)/ })).toBeDefined()
    await pane.unmount()
  }
  expect(opened).toEqual(['attention-pinned', 'attention-pinned'])
})

test('a left-out badge opens the pane on its code and message', async ($, on) => {
  engine(on, { exitCode: 0, stdout: JSON.stringify(REPORT) })
  await start($)
  const band = await $.ui.mount({ plugin: 'board-attention', surface: 'terminal', component: 'AbovePrompt', props: BAND_PROPS })
  await band.press({ key: 'order:/orders/old' })
  const pane = await $.ui.mount({ plugin: 'board-attention', surface: 'terminal', component: 'Pane', requestId: 'attention-pinned', props: PANE_PROPS })
  expect(await pane.find({ text: "Not in this session's context: the order expired on 2026-10-01 (attention::order::expired)." })).toBeDefined()
  expect(await pane.find({ text: /evals/ })).toBeUndefined()
})

test('the order badges join the parts other mods draw beneath, in one row with a bar between sections', async ($, on) => {
  engine(on, { exitCode: 0, stdout: JSON.stringify(REPORT) }, 'controls')
  await start($)
  const band = await $.ui.mount({ plugin: 'board-attention', surface: 'terminal', component: 'AbovePrompt', props: BAND_PROPS })
  const drawn = await band.drawn()
  const row = drawn.type === 'Box' ? drawn.children?.[0] : undefined
  expect(typeof row === 'object' && row.type === 'Box' ? row.props?.key : null).toBe('board-band')
  const keys = typeof row === 'object' && row.type === 'Box' ? row.children?.map(c => (typeof c === 'object' && c.type === 'Box' ? c.props?.key : 'text')) : []
  expect(keys).toEqual([SLOT.orders, 'text', SLOT.controls])
  expect((await band.find({ key: 'controls' }))?.text).toBe('Controls 5')
  expect((await band.find({ type: 'Text', text: '|' }))?.text).toBe('|')
})

test('a board that cannot be read is one red badge in the band', async ($, on) => {
  const failure = { schema: 'board/failure/v1', code: 'attention::signers::missing', message: 'there is no allowed_signers file' }
  engine(on, { exitCode: 1, stdout: JSON.stringify(failure) })
  await start($)
  const band = await $.ui.mount({ plugin: 'board-attention', surface: 'terminal', component: 'AbovePrompt', props: BAND_PROPS })
  expect((await band.find({ key: 'order:' }))?.text).toBe('Pinned orders could not be read: there is no allowed_signers file')
})

test('with no personal board the mod shows nothing, adds no block and runs board once, and a board that answers is shown', async ($, on) => {
  const failure = { schema: 'board/failure/v1', code: 'board::config::not_found', message: 'no personal board is named' }
  const answer = { exitCode: 1, stdout: JSON.stringify(failure) }
  const { ran } = engine(on, answer)
  await start($)
  for (let i = 0; i < 2; i++) {
    const context = await $.prompt.context({ blocks: [{ name: 'claudeMd', text: 'instructions' }] })
    expect(context.blocks.map(b => b.name)).toEqual(['claudeMd'])
  }
  expect(ran.length).toBe(1)
  const band = await $.ui.mount({ plugin: 'board-attention', surface: 'terminal', component: 'AbovePrompt', props: BAND_PROPS })
  expect(await band.find({ text: /Pinned orders could not be read/ })).toBeUndefined()
  expect(await band.find({ text: 'Orders' })).toBeUndefined()
  await band.unmount()

  answer.exitCode = 0
  answer.stdout = JSON.stringify(REPORT)
  await start($)
  const context = await $.prompt.context({ blocks: [{ name: 'claudeMd', text: 'instructions' }] })
  expect(context.blocks.map(b => b.name)).toEqual(['claudeMd', 'attentionOrders'])
  const shown = await $.ui.mount({ plugin: 'board-attention', surface: 'terminal', component: 'AbovePrompt', props: BAND_PROPS })
  expect((await shown.find({ key: 'order:/orders/evals' }))?.text).toBe('evals')
})
