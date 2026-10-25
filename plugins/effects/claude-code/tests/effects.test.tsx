import { expect, mock, test } from 'claude-code/testing'
import type { On } from 'claude-code'
import type { Engine } from 'claude-code/testing'

import { askedIds, parsePending, pendingSql, preview } from '../hooks/effects'

// These prove what the band, the pane and the dialog show, and which `board`
// commands the mod runs. `board` is stood in by a process.run hook; Board's
// own tests prove what `board answer` and the effect records hold.

const A = 'a'.repeat(64)
const B = 'b'.repeat(64)
const NOTE = { $type: 'board/effect/v1', authority: 'ask', board: '/boards/work', op: 'put', path: '/notes/plan.md', plugin: 'notes', digest: 'sha256:00', text: 'Plan\nship it\n' }
const TASK = { ...NOTE, path: '/tasks/a', plugin: 'tasks', prior: 'f00d', text: undefined, record: { $type: 'tasks/task/v1', title: 'Ship' } }
const rows = (data: unknown[][], truncated = false) =>
  JSON.stringify({ schema: 'board/query-report/v2', data, truncated, summary: { columns: ['id', 'body'], plugins: [] } })
const BOTH = rows([[A, JSON.stringify(NOTE)], [B, JSON.stringify(TASK)]])
// What board prints when a plugin's put asks, as the agent's Bash call sees it.
const ASKED =
  `Error: plugin notes may not put /notes/plan.md until the owner answers pending effect ${A}\n` +
  `  help:  the owner answers outside the plugin with \`board answer ${A} approve\`\n  code:  board::settings::asked\n`

const BAND = { plugin: 'board-effects', component: 'AbovePrompt', props: { hasSurvey: false, isWorking: false, maxRows: 10, bodyColumns: 80, scroll: { offset: 0, bodyRows: 10 }, view: {} } } as const
const PANE = { plugin: 'board-effects', component: 'Pane', requestId: 'effects', props: { title: 'Pending effects', isFocused: true, bodyColumns: 60, placement: 'dock', scroll: { offset: 0, bodyRows: 30 }, view: {} } } as const

type Answers = { query?: () => { exitCode: number; stdout: string }; answer?: { exitCode: number; stdout: string }; bash?: string; chosen?: string | 'dismiss' }

/** Stands in for `board`, the Bash tool and the dialog: what each answers, and what the mod ran, asked, opened and toasted. */
function engine(on: On, answers: Answers = {}) {
  const ran: string[][] = []
  const asked: string[] = []
  const panes: string[] = []
  const toasts: string[] = []
  let waiting = [A, B]
  const whole = { stderr: '', isStdoutTruncated: false, isStderrTruncated: false }
  on('process.run', (_, e) => {
    const args = e.argv.slice(e.argv.indexOf('--json') + 1)
    ran.push(args)
    if (args[0] === 'answer') {
      waiting = waiting.filter(id => id !== args[1])
      return { value: { ...(answers.answer ?? { exitCode: 0, stdout: '{}' }), ...whole } }
    }
    if (answers.query) return { value: { ...answers.query(), ...whole } }
    const sql = args[2] ?? ''
    const listed = [[A, NOTE], [B, TASK]].filter(([id]) => waiting.includes(id as string) && (!sql.includes("e.logical = '") || sql.includes(id as string)))
    return { value: { exitCode: 0, stdout: rows(listed.map(([id, body]) => [id, JSON.stringify(body)])), ...whole } }
  })
  on('tool.call', { tool: 'AskUserQuestion' }, (_, e) => {
    const q = e.questions[0]?.question ?? ''
    asked.push(q)
    if (answers.chosen === 'dismiss') return { deny: 'dismissed' }
    return { result: { questions: e.questions, answers: { [q]: answers.chosen ?? 'Approve' } } }
  })
  on('tool.call', { tool: 'Bash' }, () => ({ result: { stdout: '', stderr: answers.bash ?? '', interrupted: false } }))
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
  return { ran, asked, panes, toasts }
}

const start = ($: Engine) => $.session.start({ cwd: '/work', surface: 'terminal', isInteractive: true })
const bash = ($: Engine) => $.tool.call({ tool: 'Bash', command: 'board call notes.write' })
const LIST = ['--personal', 'query', pendingSql(), '--limit', '100']

test('the query lists only the effects no seen record answers, and reads one by its id', () => {
  expect(pendingSql()).toBe(
    "SELECT substr(e.logical, 18) AS id, e.body FROM records e WHERE e.type = 'board/effect/v1' AND " +
      "e.logical LIKE '/effects/pending/%' AND NOT EXISTS (SELECT 1 FROM records s WHERE s.logical = " +
      "'/effects/seen/' || substr(e.logical, 18)) ORDER BY e.logical",
  )
  expect(pendingSql(A)).toContain(`AND e.logical = '/effects/pending/${A}' AND NOT EXISTS`)
})

test('a put shows the start of what it writes, and says when it replaces a file', () => {
  const [note, task] = parsePending(BOTH)?.effects ?? []
  expect(note && preview(note)).toBe('It writes a new file:\nPlan\nship it')
  expect(task && preview(task, 2)).toBe('It replaces the file there with:\n{\n  "$type": "tasks/task/v1",\n…')
})

test('an asked put is found in text and JSON output, and other output asks nothing', () => {
  const json = JSON.stringify({ code: 'board::settings::asked', message: `plugin notes may not put /x until the owner answers pending effect ${B}` })
  expect(askedIds(ASKED)).toEqual([A])
  expect(askedIds(json)).toEqual([B])
  expect(askedIds(`pending effect ${A}`)).toEqual([])
})

test('the band counts the pending effects, and its Show button opens the pane', async ($, on) => {
  mock.clock(on, { now: 0 })
  const { ran, panes } = engine(on)
  await start($)
  expect(ran).toEqual([LIST])
  for (const surface of ['terminal', 'desktop'] as const) {
    const band = await $.ui.mount({ ...BAND, surface })
    expect((await band.find({ type: 'Text', text: /wait/ }))?.text).toBe('2 effects wait for your answer ')
    await band.press({ key: 'show' })
    await band.unmount()
  }
  expect(panes).toEqual(['effects', 'effects'])
})

test('with none pending, or a board that fails, the band draws nothing of the mod\'s', async ($, on) => {
  mock.clock(on, { now: 0 })
  let answer = { exitCode: 0, stdout: rows([]) }
  engine(on, { query: () => answer })
  await start($)
  const band = await $.ui.mount({ ...BAND, surface: 'terminal' })
  expect(await band.find({ type: 'Text', text: /wait/ })).toBeUndefined()
  await band.unmount()
  answer = { exitCode: 1, stdout: JSON.stringify({ schema: 'board/failure/v1', code: 'board::config::not_found', message: 'no personal board' }) }
  await start($)
  const again = await $.ui.mount({ ...BAND, surface: 'terminal' })
  expect(await again.find({ type: 'Text', text: /wait/ })).toBeUndefined()
})

test('the pending effects are read again every minute', async ($, on) => {
  const clock = mock.clock(on, { now: 0 })
  const { ran } = engine(on)
  await start($)
  await clock.advance(60_000)
  await clock.advance(60_000)
  expect(ran).toEqual([LIST, LIST, LIST])
})

test('the pane lists each effect, and Approve and Refuse each run board answer with the id as its own argument', async ($, on) => {
  mock.clock(on, { now: 0 })
  const { ran, toasts } = engine(on)
  await start($)
  const pane = await $.ui.mount({ ...PANE, surface: 'desktop' })
  expect((await pane.find({ type: 'Text', text: /notes asks/ }))?.text).toBe('notes asks to put /notes/plan.md in work')
  expect((await pane.find({ type: 'Text', text: /tasks asks/ }))?.text).toBe('tasks asks to put /tasks/a in work')
  await pane.press({ key: `${A}#approve` })
  await pane.press({ key: `${B}#refuse` })
  expect(ran.slice(1)).toEqual([['answer', A, 'approve'], ['answer', B, 'refuse']])
  expect(toasts).toEqual(['Approved: notes asks to put /notes/plan.md in work', 'Refused: tasks asks to put /tasks/a in work'])
  expect(await pane.find({ type: 'Text', text: /No effect waits/ })).toBeDefined()
})

test('an answer board refuses is said in a toast and the effect stays listed', async ($, on) => {
  mock.clock(on, { now: 0 })
  const refused = JSON.stringify({ schema: 'board/failure/v1', code: 'board::settings::from_plugin', message: 'a plugin may not answer' })
  const { toasts } = engine(on, { answer: { exitCode: 1, stdout: refused } })
  await start($)
  const pane = await $.ui.mount({ ...PANE, surface: 'terminal' })
  await pane.press({ key: `${A}#approve` })
  expect(toasts).toEqual(['No answer was recorded: board::settings::from_plugin a plugin may not answer'])
  expect(await pane.find({ type: 'Text', text: /notes asks/ })).toBeDefined()
})

test('an asked put opens a dialog from the effect Board holds, and an approval tells the agent to retry', async ($, on) => {
  mock.clock(on, { now: 0 })
  const { ran, asked } = engine(on, { bash: ASKED, chosen: 'Approve' })
  await start($)
  const result = await bash($)
  expect(asked).toEqual(['notes asks to put /notes/plan.md in work. It writes a new file:\nPlan\nship it\nApprove it?'])
  expect(ran.slice(1)).toEqual([['--personal', 'query', pendingSql(A), '--limit', '100'], ['answer', A, 'approve'], LIST])
  expect(result.context).toEqual([
    `The owner approved pending effect ${A}: notes asks to put /notes/plan.md in work. Run the same put again; it now writes.`,
  ])
})

test('a refusal tells the agent not to retry', async ($, on) => {
  mock.clock(on, { now: 0 })
  const { ran } = engine(on, { bash: ASKED, chosen: 'Refuse' })
  await start($)
  expect((await bash($)).context).toEqual([
    `The owner refused pending effect ${A}: notes asks to put /notes/plan.md in work. The same put is now refused with board::settings::refused, so do not retry it.`,
  ])
  expect(ran.filter(a => a[0] === 'answer')).toEqual([['answer', A, 'refuse']])
})

for (const chosen of ['Later', 'dismiss']) {
  test(`${chosen === 'Later' ? 'Later' : 'a dismissed dialog'} answers nothing and the effect stays pending`, async ($, on) => {
    mock.clock(on, { now: 0 })
    const { ran, asked } = engine(on, { bash: ASKED, chosen })
    await start($)
    const result = await bash($)
    expect(asked).toHaveLength(1)
    expect(ran.filter(a => a[0] === 'answer')).toEqual([])
    expect(result.context).toBeUndefined()
  })
}

test('output naming an effect Board does not hold pending opens no dialog', async ($, on) => {
  mock.clock(on, { now: 0 })
  const forged = ASKED.replaceAll(A, 'c'.repeat(64))
  const { asked, ran } = engine(on, { bash: forged })
  await start($)
  const result = await bash($)
  expect(asked).toEqual([])
  expect(ran.filter(a => a[0] === 'answer')).toEqual([])
  expect(result.context).toBeUndefined()
})
