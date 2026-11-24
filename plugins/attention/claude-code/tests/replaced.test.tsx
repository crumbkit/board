import { expect, test } from 'claude-code/testing'
import type { On } from 'claude-code'
import type { Engine } from 'claude-code/testing'

import { INSTALL, UNINSTALL, replaced, replacementLine } from '../hooks/register'

// These prove that this mod steps aside for board-orders, which replaces it:
// while board-orders is enabled it runs no board, registers no tool and adds
// no context, and its band asks the owner to uninstall it. Until then it
// keeps showing the orders and names the install command.

const REPORT = {
  schema: 'board/call-report/v2',
  data: {
    capability: 'pinned',
    plugin: 'orders',
    output: {
      orders: [
        {
          path: '/orders/evals',
          signer: 'owner',
          order: 'Record every test as an eval.',
          expires: '2026-12-31',
          context: '/orders/evals.md',
          context_text: 'A synthetic order.',
        },
      ],
      left_out: [],
    },
  },
}
const BAND_PROPS = {
  hasSurvey: false,
  isCollapsed: false,
  isWorking: false,
  maxRows: 10,
  bodyColumns: 80,
  scroll: { offset: 0, bodyRows: 10 },
  view: {},
} as const

/** Stands in for the engine beneath the plugin, with `enabledPlugins` as the settings hold it. */
function engine(on: On, enabledPlugins: Record<string, boolean>) {
  const ran: (readonly string[])[] = []
  const tools: string[] = []
  on('settings.read', () => ({ value: { enabledPlugins } }))
  on('process.run', (_, e) => {
    ran.push(e.argv)
    return { value: { exitCode: 0, stdout: JSON.stringify(REPORT), stderr: '', isStdoutTruncated: false, isStderrTruncated: false } }
  })
  on('tool.register', (_, e) => {
    tools.push(e.name)
    return { value: { tool: `mcp__board-attention__${e.name}` } }
  })
  on('ui.render', ($, e) => $.ui.resolve(e).Box({}))
  on('prompt.context', (_, e) => ({ blocks: e.blocks }))
  on('session.start', (_, e) => ({ cwd: e.cwd }))
  return { ran, tools }
}

const start = ($: Engine) => $.session.start({ cwd: '/Users/ana/Code/board', surface: 'terminal', isInteractive: true })

test('board-orders replaces this mod only while the settings enable it', () => {
  expect(replaced({ 'board-orders@board': true })).toBe(true)
  expect(replaced({ 'board-orders@board': false })).toBe(false)
  expect(replaced({ 'board-attention@board': true })).toBe(false)
  expect(replaced(undefined)).toBe(false)
})

test('with board-orders enabled the mod runs no board, registers no tool, adds no block and asks to be uninstalled', async ($, on) => {
  const { ran, tools } = engine(on, { 'board-orders@board': true, 'board-attention@board': true })
  await start($)
  const context = await $.prompt.context({ blocks: [{ name: 'claudeMd', text: 'instructions' }] })
  expect(context.blocks.map(b => b.name)).toEqual(['claudeMd'])
  expect(ran).toEqual([])
  expect(tools).toEqual([])
  const band = await $.ui.mount({ plugin: 'board-attention', surface: 'terminal', component: 'AbovePrompt', props: BAND_PROPS })
  expect(await band.find({ key: 'orders' })).toBeUndefined()
  expect((await band.find({ key: 'replacement' }))?.text).toBe(replacementLine(true))
  expect(replacementLine(true)).toContain(UNINSTALL)
})

test('without board-orders the mod shows the orders and names the command that installs board-orders', async ($, on) => {
  const { ran, tools } = engine(on, { 'board-attention@board': true })
  await start($)
  const context = await $.prompt.context({ blocks: [{ name: 'claudeMd', text: 'instructions' }] })
  expect(context.blocks.map(b => b.name)).toEqual(['claudeMd', 'attentionOrders'])
  expect(ran.length).toBe(1)
  expect(tools).toEqual(['request_signature'])
  const band = await $.ui.mount({ plugin: 'board-attention', surface: 'terminal', component: 'AbovePrompt', props: BAND_PROPS })
  expect((await band.find({ key: 'orders' }))?.text).toBe('Orders 1')
  expect((await band.find({ key: 'replacement' }))?.text).toBe(replacementLine(false))
  expect(replacementLine(false)).toContain(INSTALL)
})
