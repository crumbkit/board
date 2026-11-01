import { expect, mock, test } from 'claude-code/testing'
import type { On } from 'claude-code'
import type { Engine } from 'claude-code/testing'

import { questionSql, report, REPORT_SQL } from '../hooks/plans'

// These prove what the band and the panes draw, which `board` commands the
// mod runs, the batch it hands `board put`, and what the asking session is
// sent. `board` is stood in by a process.run hook, so the records are not
// checked against the shipped schemas here; tests/test_schema.py in the plans
// plugin does that for the answer's `written_by`.

const NOW = Date.UTC(2026, 9, 25, 9, 30, 0)
const MERGE = {
  path: '/projects/home/questions/20261024T1830Z-k3',
  question: 'Merge home#201?',
  asked: '2026-10-24T18:30:00Z',
  session: 'local_ab12',
  options: [
    { id: 'a', text: 'Merge' },
    { id: 'b', text: 'Hold' },
  ],
  recommendation: 'a',
  blocks: 'home#201',
}
const COUNT = { ...MERGE, path: '/projects/core/questions/20261025T0800Z-x1', asked: '2026-10-25T08:00:00Z', question: 'How many users have a blank email?', options: [], recommendation: null, blocks: null }
const BODY = JSON.stringify({ $type: 'plans/question/v1', question: MERGE.question, session: MERGE.session, asked: MERGE.asked, status: 'open' })

const opened = (questions: object[]) =>
  JSON.stringify({ schema: 'board/call-report/v2', data: { plugin: 'plans', capability: 'open', output: { questions, total: questions.length, truncated: false, unreadable: 0 } } })
const rows = (columns: string[], data: unknown[][]) => JSON.stringify({ schema: 'board/query-report/v2', data, truncated: false, summary: { columns, plugins: [] } })

const BAND = { plugin: 'board-plans', component: 'AbovePrompt', props: { hasSurvey: false, isWorking: false, maxRows: 10, bodyColumns: 80, scroll: { offset: 0, bodyRows: 10 }, view: {} } } as const
const pane = (requestId: string) =>
  ({ plugin: 'board-plans', component: 'Pane', requestId, props: { title: requestId, isFocused: true, bodyColumns: 60, placement: 'dock', scroll: { offset: 0, bodyRows: 30 }, view: {} } }) as const

type Answers = {
  open?: () => { exitCode: number; stdout: string }
  body?: string
  answered?: string[]
  put?: { exitCode: number; stdout: string }
  raced?: string[]
  delivered?: boolean
  report?: string
  host?: string | null
}

/** Stands in for `board` and the engine: what each command answers, and what the mod ran, opened, sent and toasted. */
function engine(on: On, answers: Answers = {}) {
  const ran: string[][] = []
  const stdins: string[] = []
  const panes: string[] = []
  const sent: { to: unknown; text: string }[] = []
  const toasts: string[] = []
  let questions = [MERGE, COUNT]
  let answered = answers.answered ?? []
  const whole = { stderr: '', isStdoutTruncated: false, isStderrTruncated: false }
  mock.env(on, answers.host === null ? {} : { CLAUDE_CODE_HOST_SESSION_ID: answers.host ?? 'local_cd34' })
  on('process.run', (_, e) => {
    const args = e.argv.slice(e.argv.indexOf('--json') + 1)
    ran.push([...e.argv.slice(4, e.argv.indexOf('--json')), ...args])
    if (e.init?.stdin !== undefined) stdins.push(e.init.stdin)
    if (args[0] === 'call') return { value: { ...(answers.open?.() ?? { exitCode: 0, stdout: opened(questions) }), ...whole } }
    if (args[0] === 'put') {
      questions = questions.filter(q => q.path !== MERGE.path)
      answered = answers.raced ?? answered
      return { value: { ...(answers.put ?? { exitCode: 0, stdout: '{}' }), ...whole } }
    }
    if (args[1]?.startsWith("SELECT 'question'")) {
      const found = [['question', MERGE.path, 'b10b', answers.body ?? BODY], ...answered.map(path => ['answer', path, 'a5a5', null])]
      return { value: { exitCode: 0, stdout: rows(['kind', 'path', 'blob', 'body'], found), ...whole } }
    }
    return { value: { exitCode: 0, stdout: answers.report ?? rows(['kind', 'path', 'at', 'title', 'detail'], [['since', null, '2026-10-24 07:00:00', null, null]]), ...whole } }
  })
  on('ui.open', (_, e) => {
    panes.push(e.id)
    return { value: { isPlaced: true } }
  })
  on('session.send', (_, e) => {
    sent.push({ to: e.to, text: e.text })
    return answers.delivered === false ? { isDelivered: false, reason: 'no such session' } : { isDelivered: true }
  })
  on('ui.toast', (_, e) => {
    toasts.push(e.text)
    return { value: undefined }
  })
  on('ui.log', () => ({ value: undefined }))
  on('command.register', (_, e) => ({ value: { command: e.name } }))
  on('ui.render', ($, e) => $.ui.resolve(e).Box({}))
  on('session.start', (_, e) => ({ cwd: e.cwd }))
  return { ran, stdins, panes, sent, toasts }
}

const start = ($: Engine) => $.session.start({ cwd: '/work', surface: 'terminal', isInteractive: true })

test('the band counts the open questions, and its Show button opens the pane', async ($, on) => {
  mock.clock(on, { now: NOW })
  const { ran, panes } = engine(on)
  await start($)
  expect(ran).toEqual([['--personal', 'call', 'plans.open', '--limit', '100']])
  for (const surface of ['terminal', 'desktop'] as const) {
    const band = await $.ui.mount({ ...BAND, surface })
    expect((await band.find({ type: 'Text', text: /open questions/ }))?.text).toBe('2 open questions for you ')
    await band.press({ key: 'show' })
    await band.unmount()
  }
  expect(panes).toEqual(['questions', 'questions'])
})

test('with no open question, or no plans plugin, the band draws nothing of the mod\'s', async ($, on) => {
  mock.clock(on, { now: NOW })
  let stdout = opened([])
  const missing = JSON.stringify({ schema: 'board/failure/v1', code: 'board::plugin::not_installed', message: 'plugin plans is not installed' })
  engine(on, { open: () => ({ exitCode: stdout === missing ? 1 : 0, stdout }) })
  await start($)
  const band = await $.ui.mount({ ...BAND, surface: 'terminal' })
  expect(await band.find({ type: 'Text', text: /open question/ })).toBeUndefined()
  await band.unmount()
  stdout = missing
  await start($)
  const again = await $.ui.mount({ ...BAND, surface: 'terminal' })
  expect(await again.find({ type: 'Text', text: /open question/ })).toBeUndefined()
})

test('once the band is drawn, the count is read again every minute, in a desktop session too', { options: { board: '/boards/me' } }, async ($, on) => {
  const clock = mock.clock(on, { now: NOW })
  const { ran } = engine(on)
  // Desktop sessions run through the Agent SDK and report isInteractive false.
  await $.session.start({ cwd: '/work', surface: 'desktop', isInteractive: false })
  await (await $.ui.mount({ ...BAND, surface: 'desktop' })).unmount()
  await (await $.ui.mount({ ...BAND, surface: 'desktop' })).unmount()
  await clock.advance(60_000)
  await clock.advance(60_000)
  expect(ran).toEqual(Array(3).fill(['-C', '/boards/me', 'call', 'plans.open', '--limit', '100']))
})

test('a session that never draws the band, such as a -p run, reads the count once and does not poll', async ($, on) => {
  const clock = mock.clock(on, { now: NOW })
  const { ran } = engine(on)
  await start($)
  await clock.advance(120_000)
  expect(ran).toHaveLength(1)
})

test('a failed refresh keeps the last count read', async ($, on) => {
  const clock = mock.clock(on, { now: NOW })
  let fails = false
  const down = { exitCode: 1, stdout: JSON.stringify({ schema: 'board/failure/v1', code: 'board::lock::timeout', message: 'the board is busy' }) }
  engine(on, { open: () => (fails ? down : { exitCode: 0, stdout: opened([MERGE, COUNT]) }) })
  await start($)
  await (await $.ui.mount({ ...BAND, surface: 'terminal' })).unmount()
  fails = true
  await clock.advance(60_000)
  const band = await $.ui.mount({ ...BAND, surface: 'terminal' })
  expect((await band.find({ type: 'Text', text: /open questions/ }))?.text).toBe('2 open questions for you ')
})

test('the pane lists each question with its options, and an option answers it in one batch sent to the asking session', async ($, on) => {
  mock.clock(on, { now: NOW })
  const { ran, stdins, sent, toasts } = engine(on)
  await start($)
  const ui = await $.ui.mount({ ...pane('questions'), surface: 'desktop' })
  expect((await ui.findAll({ type: 'Text' })).map(t => t.text)).toEqual([
    'home · 15h · blocks home#201',
    'Merge home#201?',
    'core · 1h',
    'How many users have a blank email?',
  ])
  expect((await ui.findAll({ type: 'Button' })).map(b => b.key)).toEqual([`${MERGE.path}#a`, `${MERGE.path}#b`])

  await ui.press({ key: `${MERGE.path}#a` })

  expect(ran.slice(1).map(r => r.slice(1))).toEqual([
    ['query', questionSql(MERGE.path)],
    ['put', '--batch', '--wait', '2s'],
  ])
  expect(stdins[0]?.split('\n').map(line => JSON.parse(line))).toEqual([
    {
      path: '/projects/home/answers/20261024T1830Z-k3',
      expect: 'none',
      record: {
        $type: 'plans/answer/v1',
        question: MERGE.path,
        choice: 'a',
        words: 'Merge',
        answered: '2026-10-25T09:30:00Z',
        by: 'local_cd34',
        written_by: 'plans/pane-answer',
      },
    },
    { path: MERGE.path, expect: 'b10b', record: { ...JSON.parse(BODY), status: 'answered' } },
  ])
  expect(sent).toEqual([
    {
      to: 'local_ab12',
      text: `The owner answered ${MERGE.path}: choice a, in these words: Merge\nThe answer is recorded at /projects/home/answers/20261024T1830Z-k3.`,
    },
  ])
  expect(toasts).toEqual(['Answered, and sent to local_ab12'])
  expect(await ui.find({ type: 'Text', text: 'Merge home#201?' })).toBeUndefined()
  const band = await $.ui.mount({ ...BAND, surface: 'desktop' })
  expect((await band.find({ type: 'Text', text: /open question/ }))?.text).toBe('1 open question for you ')
})

test('the owner answers in words where the surface has a field, and none of the options is chosen', async ($, on) => {
  mock.clock(on, { now: NOW })
  // Outside the desktop app no `local_` id names this session, so `by` is left out.
  const { stdins } = engine(on, { host: null })
  await start($)
  for (const surface of ['terminal', 'desktop'] as const) {
    const ui = await $.ui.mount({ ...pane('questions'), surface })
    expect(await ui.find({ key: `${COUNT.path}#words` })).toBeDefined()
    await ui.unmount()
  }
  const ui = await $.ui.mount({ ...pane('questions'), surface: 'terminal' })
  await ui.input({ key: `${COUNT.path}#words`, text: 'About 40; ask Kechen for the exact figure.' })
  const record = JSON.parse(stdins[0]?.split('\n')[0] ?? '{}').record
  expect(record).toMatchObject({ choice: 'none', words: 'About 40; ask Kechen for the exact figure.' })
  expect('by' in record).toBe(false)
})

test('a send the asking session cannot take still records the answer, and says the message was not delivered', async ($, on) => {
  mock.clock(on, { now: NOW })
  const { stdins, toasts } = engine(on, { delivered: false })
  await start($)
  const ui = await $.ui.mount({ ...pane('questions'), surface: 'terminal' })
  await ui.press({ key: `${MERGE.path}#b` })
  expect(stdins).toHaveLength(1)
  expect(toasts).toEqual(['Answered and recorded in Board. The message to local_ab12 was not delivered'])
})

test('a question that is no longer open is refused before anything is written or sent', async ($, on) => {
  mock.clock(on, { now: NOW })
  const withdrawn = JSON.stringify({ ...JSON.parse(BODY), status: 'withdrawn' })
  const { ran, stdins, sent, toasts } = engine(on, { body: withdrawn })
  await start($)
  const ui = await $.ui.mount({ ...pane('questions'), surface: 'terminal' })
  await ui.press({ key: `${MERGE.path}#a` })
  expect(ran.map(r => r[1])).toEqual(['call', 'query'])
  expect(stdins).toEqual([])
  expect(sent).toEqual([])
  expect(toasts).toEqual([`${MERGE.path} is withdrawn now, so nothing was recorded`])
})

test('a question that already has an answer under another id is refused, and the toast names that answer', async ($, on) => {
  mock.clock(on, { now: NOW })
  const elsewhere = '/projects/home/answers/20261024T1900Z-k3b'
  const { stdins, sent, toasts } = engine(on, { answered: [elsewhere] })
  await start($)
  const ui = await $.ui.mount({ ...pane('questions'), surface: 'terminal' })
  await ui.press({ key: `${MERGE.path}#a` })
  expect(stdins).toEqual([])
  expect(sent).toEqual([])
  expect(toasts).toEqual([`${MERGE.path} already has an answer at ${elsewhere}, so nothing was recorded`])
})

test('a put refused because an answer appeared meanwhile says that answer exists', async ($, on) => {
  mock.clock(on, { now: NOW })
  const stale = JSON.stringify({ schema: 'board/failure/v1', code: 'board::put::stale', message: 'the batch is refused' })
  const same = '/projects/home/answers/20261024T1830Z-k3'
  const { ran, sent, toasts } = engine(on, { put: { exitCode: 1, stdout: stale }, raced: [same] })
  await start($)
  const ui = await $.ui.mount({ ...pane('questions'), surface: 'terminal' })
  await ui.press({ key: `${MERGE.path}#a` })
  expect(ran.map(r => r[1])).toEqual(['call', 'query', 'put', 'query'])
  expect(sent).toEqual([])
  expect(toasts).toEqual([`${MERGE.path} already has an answer at ${same}, so nothing was recorded`])
})

test('a question that changes between the read and the put is refused whole, and nothing is sent', async ($, on) => {
  mock.clock(on, { now: NOW })
  const stale = JSON.stringify({ schema: 'board/failure/v1', code: 'board::put::stale', message: 'the batch is refused' })
  const { sent, toasts } = engine(on, { put: { exitCode: 1, stdout: stale } })
  await start($)
  const ui = await $.ui.mount({ ...pane('questions'), surface: 'terminal' })
  await ui.press({ key: `${MERGE.path}#a` })
  expect(sent).toEqual([])
  expect(toasts).toEqual(['No answer was recorded: board::put::stale the batch is refused'])
})

test('/yesterday reads the day from 07:00 in one query, opens the report pane and prints its totals', async ($, on) => {
  mock.clock(on, { now: NOW })
  const day = rows(
    ['kind', 'path', 'at', 'title', 'detail'],
    [
      ['since', null, '2026-10-24 07:00:00', null, null],
      ['decision', '/projects/board/decisions/d1', '2026-10-24T10:00:00Z', 'Keep the types at v1.', null],
      ['run', '/suites/claude-code/runs/r1', '2026-10-24T11:00:00Z', 'The parser lives in src/parse.rs.', 'Explore'],
    ],
  )
  const { ran, panes } = engine(on, { report: day })
  await start($)
  const result = await $.command.run({ command: 'yesterday', args: '', origin: { kind: 'composer' }, presentation: { isFullscreen: true, columns: 160 } })
  const sql = ran[1]?.[2] ?? ''
  expect(sql).toContain("'localtime', '-7 hours', 'start of day', '+7 hours', '-1 day', 'utc'")
  expect(ran[1]?.slice(3)).toEqual(['--limit', '1000'])
  expect(panes).toEqual(['yesterday'])
  expect(result.text).toBe('board: 1 decision, 0 questions asked, 0 answered, and 1 run, since 2026-10-24 07:00.')
  const ui = await $.ui.mount({ ...pane('yesterday'), surface: 'desktop' })
  expect((await ui.find({ type: 'Markdown' }))?.text).toContain('- Keep the types at v1. `/projects/board/decisions/d1`')
})

test('the report lists decisions of the decisions plugin alone', () => {
  expect(REPORT_SQL).toContain("FROM w, records r WHERE r.type = 'decisions/decision/v1' AND")
  expect(REPORT_SQL).not.toContain('plans/decision/v1')
})

test('the report lists each kind of record under its heading, and says none where there is none', () => {
  const { markdown, line } = report(
    [
      { kind: 'since', path: null, at: '2026-10-24 07:00:00', title: null, detail: null },
      { kind: 'asked', path: '/projects/core/questions/q1', at: '2026-10-24T09:00:00Z', title: 'Which region?', detail: 'answered' },
      { kind: 'answered', path: '/projects/core/answers/q1', at: '2026-10-24T10:00:00Z', title: 'Which region?', detail: 'eu-west-1' },
    ],
    true,
  )
  expect(markdown).toBe(
    [
      'Records from 2026-10-24 07:00 until 07:00 today.',
      '## Decisions made (0)\n\n- none',
      '## Questions asked (1)\n\n- Which region? (answered) `/projects/core/questions/q1`',
      '## Questions answered (1)\n\n- Which region? Answer: eu-west-1 `/projects/core/answers/q1`',
      '## Runs recorded (0)\n\n- none',
      'Board returned only the first rows; `board query` shows the rest.',
    ].join('\n\n'),
  )
  expect(line).toBe('board: 0 decisions, 1 question asked, 1 answered, and 0 runs, since 2026-10-24 07:00, counting only the first 1000 records.')
})
