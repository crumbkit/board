import { expect, mock, test } from 'claude-code/testing'
import type { On } from 'claude-code'
import type { Engine } from 'claude-code/testing'

import { askedIds, parsePending, pendingSql, preview } from '../hooks/effects'
import type { Effect } from '../types'

// These prove what the band, the pane and the dialog show, and which `board`
// commands the mod runs. `board` is stood in by a process.run hook, because
// a test here can run no process; crates/board-cli/src/tests/effects_mod.rs
// runs the query against a real board.

const A = 'a'.repeat(64)
const B = 'b'.repeat(64)
const C = 'c'.repeat(64)
const HOME = '/Users/me'
const COLUMNS = ['id', 'board', 'plugin', 'op', 'path', 'replaces', 'head', 'bytes', 'lines']
// Rows as the query returns them, by id.
const ROWS: Record<string, unknown[]> = {
  [A]: [A, `${HOME}/boards/work`, 'notes', 'put', '/notes/plan.md', 0, 'Plan\nship it\n', 13, 2],
  [B]: [B, '/srv/work', 'tasks', 'put', '/tasks/a', 1, '{"$type":"tasks/task/v1","title":"Ship"}', 40, 1],
}
const report = (data: unknown[][], truncated = false) =>
  JSON.stringify({ schema: 'board/query-report/v2', data, truncated, summary: { columns: COLUMNS, plugins: [] } })
const failed = (code: string, message: string) => JSON.stringify({ schema: 'board/failure/v1', code, message })
// What board prints when a plugin's put asks, as the agent's Bash call sees it.
const asking = (id: string) =>
  `Error: plugin notes may not put /notes/plan.md until the owner answers pending effect ${id}\n` +
  `  help:  the owner answers outside the plugin with \`board answer ${id} approve\`\n  code:  board::settings::asked\n`
const ASKED = asking(A)
const NOTE_LINE = 'notes asks to put /notes/plan.md in ~/boards/work'

const BAND = { plugin: 'board-effects', component: 'AbovePrompt', props: { hasSurvey: false, isWorking: false, maxRows: 10, bodyColumns: 80, scroll: { offset: 0, bodyRows: 10 }, view: {} } } as const
const PANE = { plugin: 'board-effects', component: 'Pane', requestId: 'effects', props: { title: 'Pending effects', isFocused: true, bodyColumns: 60, placement: 'dock', scroll: { offset: 0, bodyRows: 30 }, view: {} } } as const

type Ran = { exitCode: number; stdout: string; isStdoutTruncated?: boolean }
type Answers = {
  query?: () => Ran
  answer?: () => Ran | 'timeout'
  /** What the Bash call prints on stderr. */
  bash?: string
  /** Whether the Bash call fails, so its result is the error text alone. */
  bashFails?: boolean
  chosen?: string
  rows?: Record<string, unknown[]>
}

/** Stands in for `board`, the Bash tool and the dialog: what each answers, and what the mod ran, asked, opened and toasted. */
function engine(on: On, answers: Answers = {}) {
  const ran: string[][] = []
  const argv: string[][] = []
  const asked: string[] = []
  const options: unknown[] = []
  const panes: string[] = []
  const toasts: string[] = []
  const rows = { ...(answers.rows ?? ROWS) }
  mock.env(on, { HOME })
  on('process.run', (_, e) => {
    argv.push([...e.argv])
    const args = e.argv.slice(e.argv.indexOf('--json') + 1)
    ran.push(args)
    const whole = { stderr: '', isStdoutTruncated: false, isStderrTruncated: false }
    if (args[0] === 'answer') {
      const answered = answers.answer?.() ?? { exitCode: 0, stdout: '{}' }
      if (answered === 'timeout') throw new Error('timed out after 30000 ms')
      if (answered.exitCode === 0) delete rows[args[1] as string]
      return { value: { ...whole, ...answered } }
    }
    if (answers.query) return { value: { ...whole, ...answers.query() } }
    const one = (args[2] ?? '').match(/WHERE id = '([0-9a-f]*)'$/)?.[1]
    const listed = Object.entries(rows).filter(([id]) => one === undefined || one === id)
    return { value: { ...whole, exitCode: 0, stdout: report(listed.map(([, row]) => row)) } }
  })
  on('tool.call', { tool: 'AskUserQuestion' }, (_, e) => {
    const q = e.questions[0]?.question ?? ''
    asked.push(q)
    options.push(e.questions[0]?.options.map(o => o.label))
    if (answers.chosen === 'dismiss') return { deny: 'dismissed' }
    return { result: { questions: e.questions, answers: { [q]: answers.chosen ?? 'Approve' } } }
  })
  on('tool.call', { tool: 'Bash' }, () =>
    answers.bashFails
      ? { result: (answers.bash ?? '') as never, isError: true }
      : { result: { stdout: '', stderr: answers.bash ?? '', interrupted: false } },
  )
  on('ui.open', (_, e) => {
    panes.push(e.id)
    return { value: { isPlaced: true } }
  })
  on('ui.toast', (_, e) => {
    toasts.push(e.text)
    return { value: undefined }
  })
  on('ui.log', () => ({ value: undefined }))
  on('ui.render', ($, e) => $.ui.resolve(e).Box({}))
  on('session.start', (_, e) => ({ cwd: e.cwd }))
  return { ran, argv, asked, options, panes, toasts }
}

const start = ($: Engine) => $.session.start({ cwd: '/work', surface: 'terminal', isInteractive: true })
const bash = ($: Engine) => $.tool.call({ tool: 'Bash', command: 'board call notes.write' })
const LIST = ['--personal', 'query', pendingSql(), '--limit', '100']
const answersRun = (ran: string[][]) => ran.filter(a => a[0] === 'answer')
const effect = (fields: Partial<Effect>): Effect => ({ id: A, board: '~/w', plugin: 'notes', op: 'put', path: '/n', replaces: false, head: null, bytes: 0, lines: 0, ...fields })

test('the one-effect query wraps the list query, and an id that is not 64 hex digits matches none', () => {
  expect(pendingSql(A)).toBe(`SELECT * FROM (${pendingSql()}) WHERE id = '${A}'`)
  expect(pendingSql("x' OR 1=1 --")).toBe(`SELECT * FROM (${pendingSql()}) WHERE id = ''`)
})

test('a row whose id is not 64 hex digits is left out, and the home directory is written as ~', () => {
  const rows = [ROWS[A] as unknown[], ['../x', ...(ROWS[A] as unknown[]).slice(1)]]
  const found = parsePending(report(rows), HOME)
  expect(found?.effects.map(e => [e.id, e.board])).toEqual([[A, '~/boards/work']])
})

test('a put shows the start of what it writes, how much is left out, and how to read it whole', () => {
  expect(preview(effect({ head: 'Plan\nship it\n', bytes: 13, lines: 2 }))).toBe('It writes a new file:\nPlan\nship it')
  const long = Array.from({ length: 154 }, (_, i) => `line ${i}`).join('\n')
  expect(preview(effect({ head: long.slice(0, 2000), bytes: 12_288, lines: 154, replaces: true }))).toBe(
    'It replaces the file there with:\nline 0\nline 1\nline 2\nline 3\nline 4\nline 5\nline 6\nline 7\n' +
      `… and 146 more lines, 12 KB in all. board --personal get /effects/pending/${A} prints it whole.`,
  )
  expect(preview(effect({ head: 'x'.repeat(2000), bytes: 5_000_000, lines: 1 }))).toContain('… and the rest of the line, 4883 KB in all.')
})

test('control characters but line feed and tab are written out, so the pane draws', async ($, on) => {
  mock.clock(on, { now: 0 })
  const head = 'a\u001b[2Jb\u0007c\u0000d\u009be\tf\ng'
  const bytes = new TextEncoder().encode(head).length
  expect(preview(effect({ head, bytes, lines: 2 }))).toBe('It writes a new file:\na\\x1B[2Jb\\x07c\\x00d\\x9Be\tf\ng')
  engine(on, { rows: { [A]: [A, '/w', 'notes', 'put', '/p\u001b', 0, head, bytes, 2] } })
  await start($)
  const pane = await $.ui.mount({ ...PANE, surface: 'terminal' })
  expect((await pane.find({ type: 'Text', text: /notes asks/ }))?.text).toBe('notes asks to put /p\\x1B in /w')
})

test('an op other than put says that an approval stands until its seen record is removed', () => {
  expect(preview(effect({ op: 'review.comment', path: '/pulls/7' }))).toBe(
    `An approval lets notes review.comment /pulls/7 again until /effects/seen/${A} is removed.`,
  )
})

test('an asked put is found in text and JSON output, each id once, and other output asks nothing', () => {
  const json = JSON.stringify({ code: 'board::settings::asked', message: `plugin notes may not put /x until the owner answers pending effect ${B}` })
  expect(askedIds(ASKED + ASKED)).toEqual([A])
  expect(askedIds(json)).toEqual([B])
  expect(askedIds(`pending effect ${A}`)).toEqual([])
})

test('board runs through sh with each value its own argument, and the band counts the pending effects', async ($, on) => {
  mock.clock(on, { now: 0 })
  const { ran, argv, panes } = engine(on)
  await start($)
  expect(ran).toEqual([LIST])
  expect(argv[0]?.slice(0, 2)).toEqual(['/bin/sh', '-c'])
  expect(argv[0]?.[2]).toMatch(/ exec board "\$@"$/)
  expect(argv[0]?.slice(3)).toEqual(['board', '--json', ...LIST])
  for (const surface of ['terminal', 'desktop'] as const) {
    const band = await $.ui.mount({ ...BAND, surface })
    expect((await band.find({ type: 'Text', text: /wait/ }))?.text).toBe('2 effects wait for your answer ')
    await band.press({ key: 'show' })
    await band.unmount()
  }
  expect(panes).toEqual(['effects', 'effects'])
})

test('with none pending, or no read that worked, the band draws nothing of the mod\'s', async ($, on) => {
  mock.clock(on, { now: 0 })
  let answer: Ran = { exitCode: 0, stdout: report([]) }
  engine(on, { query: () => answer })
  await start($)
  const band = await $.ui.mount({ ...BAND, surface: 'terminal' })
  expect(await band.find({ type: 'Text', text: /wait/ })).toBeUndefined()
  await band.unmount()
  answer = { exitCode: 1, stdout: failed('board::config::not_found', 'no personal board') }
  await start($)
  const again = await $.ui.mount({ ...BAND, surface: 'terminal' })
  expect(await again.find({ type: 'Text', text: /wait/ })).toBeUndefined()
})

test('a read that fails keeps the last count in the band, and the pane says the read failed', async ($, on) => {
  const clock = mock.clock(on, { now: 0 })
  let answer: Ran = { exitCode: 0, stdout: report([ROWS[A] as unknown[]]) }
  engine(on, { query: () => answer })
  await start($)
  answer = { exitCode: 1, stdout: failed('board::query::timeout', 'the query ran past 5 s') }
  await clock.advance(60_000)
  const band = await $.ui.mount({ ...BAND, surface: 'terminal' })
  expect((await band.find({ type: 'Text', text: /wait/ }))?.text).toBe('1 effect waits for your answer ')
  const pane = await $.ui.mount({ ...PANE, surface: 'desktop' })
  expect((await pane.find({ type: 'Text', text: /could not be read/ }))?.text).toBe(
    'The pending effects could not be read: board::query::timeout the query ran past 5 s. This list is from the last read that worked.',
  )
  expect(await pane.find({ type: 'Text', text: /notes asks/ })).toBeDefined()
})

test('a first read whose output passes the 4 MiB a run keeps is said in the pane, never as no effect', async ($, on) => {
  mock.clock(on, { now: 0 })
  engine(on, { query: () => ({ exitCode: 0, stdout: '{"schema":"board/query-report/v2","data":[["aaa', isStdoutTruncated: true }) })
  await start($)
  const pane = await $.ui.mount({ ...PANE, surface: 'terminal' })
  expect((await pane.find({ type: 'Text', text: /could not be read/ }))?.text).toBe(
    'The pending effects could not be read: board printed more than the 4 MiB a process run keeps.',
  )
  expect(await pane.find({ type: 'Text', text: /No effect waits/ })).toBeUndefined()
})

test('the pending effects are read again every minute', async ($, on) => {
  const clock = mock.clock(on, { now: 0 })
  const { ran } = engine(on)
  await start($)
  await clock.advance(60_000)
  await clock.advance(60_000)
  expect(ran).toEqual([LIST, LIST, LIST])
})

test('the pane lists each effect with its full board root, and Approve and Refuse run board answer', async ($, on) => {
  mock.clock(on, { now: 0 })
  const { ran, toasts } = engine(on)
  await start($)
  const pane = await $.ui.mount({ ...PANE, surface: 'desktop' })
  expect((await pane.find({ type: 'Text', text: /notes asks/ }))?.text).toBe(NOTE_LINE)
  expect((await pane.find({ type: 'Text', text: /tasks asks/ }))?.text).toBe('tasks asks to put /tasks/a in /srv/work')
  await pane.press({ key: `${A}#approve` })
  await pane.press({ key: `${B}#refuse` })
  expect(answersRun(ran)).toEqual([['answer', A, 'approve'], ['answer', B, 'refuse']])
  expect(toasts).toEqual([`Approved: ${NOTE_LINE}`, 'Refused: tasks asks to put /tasks/a in /srv/work'])
  expect(await pane.find({ type: 'Text', text: /No effect waits/ })).toBeDefined()
})

test('an answer board refuses says nothing was recorded, and the effect stays listed', async ($, on) => {
  mock.clock(on, { now: 0 })
  const { toasts } = engine(on, { answer: () => ({ exitCode: 1, stdout: failed('board::settings::from_plugin', 'a plugin may not answer') }) })
  await start($)
  const pane = await $.ui.mount({ ...PANE, surface: 'terminal' })
  await pane.press({ key: `${A}#approve` })
  expect(toasts).toEqual(['No answer was recorded: board::settings::from_plugin a plugin may not answer'])
  expect(await pane.find({ type: 'Text', text: /notes asks/ })).toBeDefined()
})

test('an answer that times out or is killed is said to be unknown, and the list is read again', async ($, on) => {
  mock.clock(on, { now: 0 })
  let outcome: Ran | 'timeout' = 'timeout'
  const { ran, toasts } = engine(on, { answer: () => outcome })
  await start($)
  const pane = await $.ui.mount({ ...PANE, surface: 'terminal' })
  await pane.press({ key: `${A}#approve` })
  outcome = { exitCode: 1, stdout: '' }
  await pane.press({ key: `${A}#approve` })
  const unknown = 'Whether your answer was recorded is unknown, because board answer did not finish. The list is read again.'
  expect(toasts).toEqual([unknown, unknown])
  expect(ran).toEqual([LIST, ['answer', A, 'approve'], LIST, ['answer', A, 'approve'], LIST])
})

test('an asked put opens a dialog from the effect Board holds, Later first, and an approval tells the agent to retry', async ($, on) => {
  mock.clock(on, { now: 0 })
  const { ran, asked, options } = engine(on, { bash: ASKED, chosen: 'Approve' })
  await start($)
  const result = await bash($)
  expect(asked).toEqual([`${NOTE_LINE}. It writes a new file:\nPlan\nship it\nApprove it?`])
  expect(options).toEqual([['Later', 'Refuse', 'Approve']])
  expect(ran.slice(1)).toEqual([['--personal', 'query', pendingSql(A), '--limit', '100'], ['answer', A, 'approve'], LIST])
  expect(result.context).toEqual([`The owner approved pending effect ${A}: ${NOTE_LINE}. Run the same put again; it now writes.`])
})

test('a failing Bash call, whose result is its error text, opens the dialog too', async ($, on) => {
  mock.clock(on, { now: 0 })
  const { asked } = engine(on, { bash: ASKED, bashFails: true, chosen: 'Later' })
  await start($)
  await bash($)
  expect(asked).toHaveLength(1)
})

test('output naming several ids asks about each, and parallel calls ask about an id once', async ($, on) => {
  mock.clock(on, { now: 0 })
  const { asked, ran } = engine(on, { bash: asking(A) + asking(B), chosen: 'Later' })
  await start($)
  await Promise.all([bash($), bash($)])
  await bash($)
  expect(asked.map(q => q.split(' asks')[0])).toEqual(['notes', 'tasks'])
  expect(answersRun(ran)).toEqual([])
})

test('a refusal tells the agent not to retry', async ($, on) => {
  mock.clock(on, { now: 0 })
  const { ran } = engine(on, { bash: ASKED, chosen: 'Refuse' })
  await start($)
  expect((await bash($)).context).toEqual([
    `The owner refused pending effect ${A}: ${NOTE_LINE}. The same put is now refused with board::settings::refused, so do not retry it.`,
  ])
  expect(answersRun(ran)).toEqual([['answer', A, 'refuse']])
})

for (const chosen of ['Later', 'dismiss']) {
  test(`${chosen === 'Later' ? 'Later' : 'a dismissed dialog'} answers nothing and the effect stays pending`, async ($, on) => {
    mock.clock(on, { now: 0 })
    const { ran, asked } = engine(on, { bash: ASKED, chosen })
    await start($)
    const result = await bash($)
    expect(asked).toHaveLength(1)
    expect(answersRun(ran)).toEqual([])
    expect(result.context).toBeUndefined()
  })
}

test('output naming an effect Board does not hold pending opens no dialog', async ($, on) => {
  mock.clock(on, { now: 0 })
  const { asked, ran } = engine(on, { bash: asking(C) })
  await start($)
  const result = await bash($)
  expect(asked).toEqual([])
  expect(answersRun(ran)).toEqual([])
  expect(result.context).toBeUndefined()
})
