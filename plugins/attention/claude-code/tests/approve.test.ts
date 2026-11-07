import { expect, test } from 'claude-code/testing'

import { outcome } from '../hooks/register'

// These prove the band and the messages the agent gets, with
// `board call attention.sign` stood in by a process.run hook. They do not
// open the signing window; board-cli's attention tests sign with a test key.

const REQUEST = { path: '/orders/evals' }
const TOOL = 'mcp__board-attention__request_signature'
const SIGNED =
  'The owner signed the order /orders/evals in the personal board. ' +
  "Agents can check it with `board --personal call attention.verify --path '/orders/evals'`."
const BAND_PROPS = {
  hasSurvey: false,
  isWorking: false,
  maxRows: 10,
  bodyColumns: 80,
  scroll: { offset: 0, bodyRows: 10 },
  view: {},
} as const

const failureDocument = (code: string, fields: Record<string, unknown> = {}) =>
  JSON.stringify({ schema: 'board/failure/v1', code, message: `refused with ${code}`, fields })

test('a sign that succeeds tells the agent the owner signed', () => {
  expect(outcome(REQUEST, 0, '{"schema":"board/call-report/v2"}', '')).toEqual({ kind: 'signed', message: SIGNED })
})

test("a declined sign passes the owner's words on, and a cancelled one passes nothing", () => {
  const declined = outcome(
    REQUEST,
    1,
    failureDocument('attention::sign::declined', { path: '/orders/evals', feedback: 'Say which branches.' }),
    '',
  )
  expect(declined.kind).toBe('changes')
  expect(declined.kind === 'changes' && declined.message.includes('\n\nSay which branches.\n\n')).toBe(true)
  expect(outcome(REQUEST, 1, failureDocument('attention::sign::cancelled'), '')).toEqual({ kind: 'cancelled' })
})

test('any other refusal is a failure that names its code, and output that is not Board\'s names the exit status', () => {
  expect(outcome(REQUEST, 1, failureDocument('attention::sign::no_signer'), '')).toEqual({
    kind: 'failed',
    message: 'Signing the order /orders/evals in the personal board failed with attention::sign::no_signer: refused with attention::sign::no_signer',
  })
  expect(outcome(REQUEST, 127, '', 'sh: board: not found\n')).toEqual({
    kind: 'failed',
    message: 'Signing the order /orders/evals in the personal board failed with exit status 127:\n\nsh: board: not found',
  })
})

test('a request shows a band whose button runs attention.sign on the personal board and sends the decision to the agent', async ($, on) => {
  const ran: (readonly string[])[] = []
  const sent: string[] = []
  on('process.run', (_, e) => {
    ran.push(e.argv)
    const pinned = e.argv.includes('attention.pinned')
    const stdout = pinned
      ? JSON.stringify({ schema: 'board/call-report/v2', data: { output: { orders: [], left_out: [] } } })
      : '{"schema":"board/call-report/v2"}'
    return { value: { exitCode: 0, stdout, stderr: '', isStdoutTruncated: false, isStderrTruncated: false } }
  })
  on('prompt.submit', (_, e) => {
    sent.push(e.text)
    return { text: e.text }
  })
  on('tool.register', (_, e) => ({ value: { tool: `mcp__board-attention__${e.name}` } }))
  const toasts: string[] = []
  on('ui.toast', (_, e) => {
    toasts.push(JSON.stringify(e))
    return { value: undefined }
  })
  on('ui.render', ($, e) => $.ui.resolve(e).Box({}))
  on('session.start', (_, e) => e)
  await $.session.start({ cwd: '/Users/luke/Code/board', surface: 'terminal', isInteractive: true })
  await $.tool.call({ tool: TOOL, ...REQUEST })

  const band = await $.ui.mount({ plugin: 'board-attention', surface: 'terminal', component: 'AbovePrompt', props: BAND_PROPS })
  expect(await band.find({ text: /\/orders\/evals is waiting for your signature/ })).toBeDefined()

  await band.press({ key: 'review' })
  expect(toasts).toEqual([])
  expect(ran.at(-1)?.slice(-7)).toEqual(['board', '--personal', '--json', 'call', 'attention.sign', '--path', '/orders/evals'])
  expect(sent).toEqual([SIGNED])
  expect(await band.find({ key: 'review' })).toBeUndefined()
})

test('a request whose path is not absolute is denied, and an absolute one is accepted', async ($, on) => {
  on('tool.register', (_, e) => ({ value: { tool: `mcp__board-attention__${e.name}` } }))
  on('process.run', () => ({ value: { exitCode: 1, stdout: failureDocument('board::config::not_found'), stderr: '', isStdoutTruncated: false, isStderrTruncated: false } }))
  on('session.start', (_, e) => e)
  await $.session.start({ cwd: '/Users/luke/Code/board', surface: 'terminal', isInteractive: true })
  const denied = await $.tool.call({ tool: TOOL, path: 'orders/evals' })
  expect(JSON.stringify(denied)).toContain('path must be absolute')
  const accepted = await $.tool.call({ tool: TOOL, path: '/orders/evals' })
  expect(JSON.stringify(accepted)).toContain('Asked the owner to review /orders/evals in the personal board.')
})
