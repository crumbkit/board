import { expect, mock, test } from 'claude-code/testing'
import type { On } from 'claude-code'
import type { Engine } from 'claude-code/testing'

import { bandRows } from '../hooks/band'
import { approvalText, blocking, countBadge, NAMES, namesWhole, parseWaiting, questionsLabel, report, REPORT_SQL, waitingLabel } from '../hooks/plans'

// These prove what the band and the panes draw, which `board` commands the
// mod runs with which flags, and what the asking sessions are sent. `board`
// is stood in by a process.run hook, so plans.answer's own checks and the
// record it writes are not exercised here; the plans plugin's crate tests them.

const NOW = Date.UTC(2026, 9, 25, 9, 30, 0)
const MERGE = {
  path: '/projects/home/questions/20261024T1830Z-k3',
  question: 'Merge home#201?',
  asked: '2026-10-24T18:30:00Z',
  session: 'local_ab12',
  also_asked: [] as string[],
  options: [
    { id: 'a', text: 'Merge' },
    { id: 'b', text: 'Hold' },
  ],
  recommendation: 'a',
  blocks: null,
  blocked_work: 'home#201',
}
const COUNT = { ...MERGE, path: '/projects/core/questions/20261025T0800Z-x1', asked: '2026-10-25T08:00:00Z', question: 'How many users have a blank email?', options: [], recommendation: null, blocks: null, blocked_work: null }

const ANSWER = '/projects/home/answers/20261024T1830Z-k3'
/** What plans.answer prints when it records the answer at `answer`, with `attached` when it names one. */
const answered = (answer = ANSWER, attached?: object) =>
  JSON.stringify({ schema: 'board/call-report/v2', data: { plugin: 'plans', capability: 'answer', output: { answer, question: MERGE.path, ...(attached === undefined ? {} : { attached }) } } })
const opened = (questions: object[]) =>
  JSON.stringify({ schema: 'board/call-report/v2', data: { plugin: 'plans', capability: 'open', output: { questions, total: questions.length, truncated: false, unreadable: 0 } } })
const rows = (columns: string[], data: unknown[][]) => JSON.stringify({ schema: 'board/query-report/v2', data, truncated: false, summary: { columns, plugins: [] } })

const ABSENT = JSON.stringify({
  schema: 'board/failure/v1',
  code: 'board::plugin::no_capability',
  message: 'plugin plans has no capability waiting; it offers open, answer, withdraw',
  fields: { capability: 'waiting', offered: ['open', 'answer', 'withdraw'], plugin: 'plans' },
})
const waitingReport = (total: number, blocking: number) =>
  JSON.stringify({
    schema: 'board/call-report/v2',
    data: {
      plugin: 'plans',
      capability: 'waiting',
      output: { total, blocking, line: `${total} wait on you`, questions: [], gates: [], effects: [], truncated: false, unreadable: 0 },
    },
  })

const BAND = { plugin: 'board-plans', component: 'AbovePrompt', props: { hasSurvey: false, isCollapsed: false, isWorking: false, maxRows: 10, bodyColumns: 80, scroll: { offset: 0, bodyRows: 10 }, view: {} } } as const
const pane = (requestId: string) =>
  ({ plugin: 'board-plans', component: 'Pane', requestId, props: { title: requestId, isFocused: true, bodyColumns: 60, placement: 'dock', scroll: { offset: 0, bodyRows: 30 }, view: {} } }) as const

type Answers = {
  open?: () => { exitCode: number; stdout: string }
  /** What plans.waiting answers; by default the plans plugin has no such capability. */
  waiting?: () => { exitCode: number; stdout: string }
  /** What plans.answer answers, or `timeout` when its run rejects; by default it records the answer. */
  answer?: () => { exitCode: number; stdout: string } | 'timeout'
  /** What changes.approve answers; by default it asks the owner. */
  approve?: () => { exitCode: number; stdout: string }
  delivered?: boolean
  report?: string
  host?: string | null
}

/** Stands in for `board` and the engine: what each command answers, and what the mod ran, opened, sent and toasted. */
function engine(on: On, answers: Answers = {}) {
  const ran: string[][] = []
  const panes: string[] = []
  const sent: { to: unknown; text: string }[] = []
  const toasts: string[] = []
  let questions = [MERGE, COUNT]
  const whole = { stderr: '', isStdoutTruncated: false, isStderrTruncated: false }
  mock.env(on, answers.host === null ? {} : { CLAUDE_CODE_HOST_SESSION_ID: answers.host ?? 'local_cd34' })
  const waited: string[][] = []
  const limits: (number | undefined)[] = []
  on('process.run', (_, e) => {
    const args = e.argv.slice(e.argv.indexOf('--json') + 1)
    if (args[1] === 'plans.waiting') {
      waited.push(e.argv.slice(4))
      return { value: { ...(answers.waiting?.() ?? { exitCode: 1, stdout: ABSENT }), ...whole } }
    }
    ran.push([...e.argv.slice(4, e.argv.indexOf('--json')), ...args])
    if (args[1] === 'plans.answer') {
      limits.push(e.init?.timeoutMs)
      const result = answers.answer?.() ?? { exitCode: 0, stdout: answered() }
      if (result === 'timeout') throw new Error('timed out after 15000 ms')
      if (result.exitCode === 0) questions = questions.filter(q => q.path !== MERGE.path)
      return { value: { ...result, ...whole } }
    }
    if (args[1] === 'changes.approve') {
      return { value: { ...(answers.approve?.() ?? { exitCode: 0, stdout: JSON.stringify({ schema: 'board/call-report/v2', data: { output: { state: 'asked' } } }) }), ...whole } }
    }
    if (args[0] === 'call') return { value: { ...(answers.open?.() ?? { exitCode: 0, stdout: opened(questions) }), ...whole } }
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
  return { ran, waited, limits, panes, sent, toasts }
}

const start = ($: Engine) => $.session.start({ cwd: '/work', surface: 'terminal', isInteractive: true })

test('plans.waiting is read as its total and blocking count, and a board without it is told apart from a failure', () => {
  expect(parseWaiting(0, waitingReport(14, 11), '')).toEqual({ kind: 'read', waiting: { total: 14, blocking: 11 } })
  expect(parseWaiting(1, ABSENT, '')).toEqual({ kind: 'absent' })
  const older = JSON.stringify({ ...JSON.parse(ABSENT), code: 'board::plugin::not_installed' })
  expect(parseWaiting(1, older, '')).toEqual({ kind: 'absent' })
  const locked = JSON.stringify({ schema: 'board/failure/v1', code: 'board::lock', message: 'the board is locked' })
  expect(parseWaiting(1, locked, '')).toEqual({ kind: 'failed', why: 'board::lock the board is locked' })
  const other = JSON.stringify({ ...JSON.parse(ABSENT), fields: { capability: 'open' } })
  expect(parseWaiting(1, other, '').kind).toBe('failed')
})

test('the button says what waits and how much blocks, or the open questions until plans.waiting exists', () => {
  expect(waitingLabel({ total: 14, blocking: 11 })).toBe('Waiting 14 · 11 blocking')
  expect(waitingLabel({ total: 3, blocking: 0 })).toBe('Waiting 3')
  expect(waitingLabel({ total: 0, blocking: 0 })).toBeNull()
  expect(questionsLabel(2)).toBe('Questions 2')
  expect(questionsLabel(0)).toBeNull()
})

test('without plans.waiting the button counts the open questions, asks for waiting once, and opens the pane', async ($, on) => {
  const clock = mock.clock(on, { now: NOW })
  const { ran, waited, panes } = engine(on)
  await start($)
  expect(ran).toEqual([['--personal', 'call', 'plans.open', '--limit', '100']])
  for (const surface of ['terminal', 'desktop'] as const) {
    const band = await $.ui.mount({ ...BAND, surface })
    expect((await band.find({ key: 'waiting' }))?.text).toBe('Questions 2')
    await band.press({ key: 'waiting' })
    await band.unmount()
  }
  await clock.advance(60_000)
  expect(waited).toEqual([['--personal', '--json', 'call', 'plans.waiting']])
  expect(panes).toEqual(['questions', 'questions'])
})

test('with plans.waiting the button says what waits and how much blocks, and its part sits in the waiting slot', async ($, on) => {
  mock.clock(on, { now: NOW })
  engine(on, { waiting: () => ({ exitCode: 0, stdout: waitingReport(14, 11) }) })
  await start($)
  const band = await $.ui.mount({ ...BAND, surface: 'terminal' })
  expect((await band.find({ key: 'waiting' }))?.text).toBe('Waiting 14 · 11 blocking')
  const drawn = await band.drawn()
  const row = bandRows(drawn)[0]
  expect(row !== undefined ? row.children?.map(c => (typeof c === 'object' && c.type === 'Box' ? c.props?.key : '')) : []).toEqual(['band:b3-waiting'])
})

test('when nothing waits the band draws nothing of the mod\'s, even with open questions counted elsewhere', async ($, on) => {
  mock.clock(on, { now: NOW })
  engine(on, { waiting: () => ({ exitCode: 0, stdout: waitingReport(0, 0) }) })
  await start($)
  const band = await $.ui.mount({ ...BAND, surface: 'terminal' })
  expect(await band.find({ key: 'waiting' })).toBeUndefined()
})

test('with no open question, or no plans plugin, the band draws nothing of the mod\'s', async ($, on) => {
  mock.clock(on, { now: NOW })
  let stdout = opened([])
  const missing = JSON.stringify({ schema: 'board/failure/v1', code: 'board::plugin::not_installed', message: 'plugin plans is not installed' })
  engine(on, { open: () => ({ exitCode: stdout === missing ? 1 : 0, stdout }) })
  await start($)
  const band = await $.ui.mount({ ...BAND, surface: 'terminal' })
  expect(await band.find({ key: 'waiting' })).toBeUndefined()
  await band.unmount()
  stdout = missing
  await start($)
  const again = await $.ui.mount({ ...BAND, surface: 'terminal' })
  expect(await again.find({ key: 'waiting' })).toBeUndefined()
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

test('a failed refresh keeps the last count read and marks it stale, and a read that works clears the mark', async ($, on) => {
  const clock = mock.clock(on, { now: NOW })
  let fails = false
  const down = { exitCode: 1, stdout: JSON.stringify({ schema: 'board/failure/v1', code: 'board::lock::timeout', message: 'the board is busy' }) }
  engine(on, { open: () => (fails ? down : { exitCode: 0, stdout: opened([MERGE, COUNT]) }) })
  await start($)
  const first = await $.ui.mount({ ...BAND, surface: 'terminal' })
  expect((await first.find({ key: 'waiting' }))?.text).toBe('Questions 2')
  await first.unmount()
  fails = true
  await clock.advance(60_000)
  const band = await $.ui.mount({ ...BAND, surface: 'terminal' })
  expect((await band.find({ key: 'waiting' }))?.text).toBe('Questions 2: the last read failed')
  await band.unmount()
  fails = false
  await clock.advance(60_000)
  const again = await $.ui.mount({ ...BAND, surface: 'terminal' })
  expect((await again.find({ key: 'waiting' }))?.text).toBe('Questions 2')
})

const TIMEOUT = {
  exitCode: 1,
  stdout: JSON.stringify({
    schema: 'board/failure/v1',
    code: 'plans::board::refused',
    message: '`board query` refused with board::query::timeout: the query ran longer than 1s',
    fields: { code: 'board::query::timeout', plugin: 'plans' },
  }),
}

test('a session whose first read of the questions fails shows that they could not be read, and the pane says why', async ($, on) => {
  mock.clock(on, { now: NOW })
  engine(on, { open: () => TIMEOUT })
  await start($)
  const band = await $.ui.mount({ ...BAND, surface: 'terminal' })
  expect((await band.find({ key: 'waiting' }))?.text).toBe('Questions could not be read')
  const shown = await $.ui.mount({ ...pane('questions'), surface: 'terminal' })
  expect((await shown.find({ type: 'Text', text: /could not be read/ }))?.text).toBe(
    'The open questions could not be read: plans::board::refused `board query` refused with board::query::timeout: the query ran longer than 1s.',
  )
})

test('the band says nothing of a failure that only means there is no personal board or no plans plugin', async ($, on) => {
  mock.clock(on, { now: NOW })
  const none = { exitCode: 1, stdout: JSON.stringify({ schema: 'board/failure/v1', code: 'board::config::not_found', message: 'no personal board' }) }
  engine(on, { open: () => none })
  await start($)
  const band = await $.ui.mount({ ...BAND, surface: 'terminal' })
  expect(await band.find({ key: 'waiting' })).toBeUndefined()
})

test('a failed plans.waiting is marked beside the last count of what waits', async ($, on) => {
  const clock = mock.clock(on, { now: NOW })
  let fails = false
  const down = { exitCode: 1, stdout: JSON.stringify({ schema: 'board/failure/v1', code: 'board::query::timeout', message: 'the query ran longer than 1s' }) }
  engine(on, { waiting: () => (fails ? down : { exitCode: 0, stdout: waitingReport(14, 11) }) })
  await start($)
  await (await $.ui.mount({ ...BAND, surface: 'terminal' })).unmount()
  fails = true
  await clock.advance(60_000)
  const band = await $.ui.mount({ ...BAND, surface: 'terminal' })
  expect((await band.find({ key: 'waiting' }))?.text).toBe('Waiting 14 · 11 blocking: the last read failed')
})

test('the count button names a stale or missing count, and a zero count with no failure draws nothing', () => {
  const none = { open: null, waiting: null }
  expect(countBadge(null, { questions: [], total: 3 }, none)).toEqual({ label: 'Questions 3', problem: false })
  expect(countBadge(null, { questions: [], total: 3 }, { ...none, open: 'x' })).toEqual({ label: 'Questions 3: the last read failed', problem: true })
  expect(countBadge(null, { questions: [], total: 0 }, { ...none, open: 'x' })).toEqual({ label: 'Questions 0: the last read failed', problem: true })
  expect(countBadge(null, null, { ...none, open: 'x' })).toEqual({ label: 'Questions could not be read', problem: true })
  expect(countBadge(null, { questions: [], total: 0 }, none)).toBeNull()
})

test('the failure marked on the count button is the failure of the read whose count is drawn', () => {
  const none = { open: null, waiting: null }
  const waits = { total: 14, blocking: 0 }
  const open = { questions: [], total: 3 }
  // plans.waiting works and plans.open fails: the drawn count is fresh.
  expect(countBadge(waits, open, { ...none, open: 'board::query::timeout the query ran longer than 1s' })).toEqual({ label: 'Waiting 14', problem: false })
  // plans.waiting fails and plans.open works: the drawn count is the stale one.
  expect(countBadge(waits, open, { ...none, waiting: 'board::query::timeout the query ran longer than 1s' })).toEqual({ label: 'Waiting 14: the last read failed', problem: true })
  // plans.waiting has only ever failed quietly, so the open count is drawn and its real failure shows.
  const quiet = 'board::plugin::not_installed plugin plans is not installed'
  const real = 'board::query::timeout the query ran longer than 1s'
  expect(countBadge(null, open, { open: real, waiting: quiet })).toEqual({ label: 'Questions 3: the last read failed', problem: true })
  expect(countBadge(null, null, { open: real, waiting: quiet })).toEqual({ label: 'Questions could not be read', problem: true })
  // The twin: the same quiet failure beside a quiet one draws no mark.
  expect(countBadge(null, open, { open: quiet, waiting: quiet })).toEqual({ label: 'Questions 3', problem: false })
})

test('a plans.waiting that comes back absent clears the failure that an earlier read left', async ($, on) => {
  const clock = mock.clock(on, { now: NOW })
  const timeout = { exitCode: 1, stdout: JSON.stringify({ schema: 'board/failure/v1', code: 'board::query::timeout', message: 'the query ran longer than 1s' }) }
  let answer = { exitCode: 0, stdout: waitingReport(14, 11) }
  engine(on, { waiting: () => answer })
  await start($)
  await (await $.ui.mount({ ...BAND, surface: 'terminal' })).unmount()
  answer = timeout
  await clock.advance(60_000)
  const failing = await $.ui.mount({ ...BAND, surface: 'terminal' })
  expect((await failing.find({ key: 'waiting' }))?.text).toBe('Waiting 14 · 11 blocking: the last read failed')
  await failing.unmount()
  answer = { exitCode: 1, stdout: ABSENT }
  await clock.advance(60_000)
  const band = await $.ui.mount({ ...BAND, surface: 'terminal' })
  expect((await band.find({ key: 'waiting' }))?.text).toBe('Waiting 14 · 11 blocking')
})

/** The plans.answer call the pane runs for `path`, after the board flag. */
const answerCall = (path: string, option: string, words: string, session?: string) => [
  'call',
  'plans.answer',
  `--path=${path}`,
  `--option=${option}`,
  `--words=${words}`,
  ...(session === undefined ? [] : [`--session=${session}`]),
  '--written_by=plans/pane-answer',
]

test('the pane lists each question with its options, and an option answers it through plans.answer and is sent to the asking session', async ($, on) => {
  mock.clock(on, { now: NOW })
  const { ran, limits, sent, toasts } = engine(on)
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

  expect(ran.slice(1)).toEqual([['--personal', ...answerCall(MERGE.path, 'a', 'Merge', 'local_cd34')]])
  // plans.answer may wait for board's lock twice, so it gets longer than a read's 5 seconds.
  expect(limits).toEqual([15_000])
  expect(sent).toEqual([
    {
      to: 'local_ab12',
      text: `The owner answered ${MERGE.path}: choice a, in these words: Merge\nThe answer is recorded at ${ANSWER}.`,
    },
  ])
  expect(toasts).toEqual(['Answered, and sent to local_ab12'])
  expect(await ui.find({ type: 'Text', text: 'Merge home#201?' })).toBeUndefined()
  const band = await $.ui.mount({ ...BAND, surface: 'desktop' })
  expect((await band.find({ key: 'waiting' }))?.text).toBe('Questions 1')
})

test('the owner answers in words where the surface has a field, as option none, and words that start with a dash stay the value', async ($, on) => {
  mock.clock(on, { now: NOW })
  // Outside the desktop app no `local_` id names this session, so no --session is passed.
  const { ran } = engine(on, { host: null })
  await start($)
  for (const surface of ['terminal', 'desktop'] as const) {
    const ui = await $.ui.mount({ ...pane('questions'), surface })
    expect(await ui.find({ key: `${COUNT.path}#words` })).toBeDefined()
    await ui.unmount()
  }
  const ui = await $.ui.mount({ ...pane('questions'), surface: 'terminal' })
  await ui.input({ key: `${COUNT.path}#words`, text: '-40, so ask Kechen for the exact figure.' })
  expect(ran.slice(1)).toEqual([['--personal', ...answerCall(COUNT.path, 'none', '-40, so ask Kechen for the exact figure.')]])
})

test('the answer is also sent to each session that asked the same question', async ($, on) => {
  mock.clock(on, { now: NOW })
  const merged = { ...MERGE, also_asked: ['local_ef56', 'local_gh78'] }
  const { sent, toasts } = engine(on, { open: () => ({ exitCode: 0, stdout: opened([merged, COUNT]) }) })
  await start($)
  const ui = await $.ui.mount({ ...pane('questions'), surface: 'terminal' })
  await ui.press({ key: `${MERGE.path}#b` })
  expect(sent.map(s => s.to)).toEqual(['local_ab12', 'local_ef56', 'local_gh78'])
  expect(new Set(sent.map(s => s.text)).size).toBe(1)
  expect(toasts).toEqual(['Answered, and sent to local_ab12'])
})

test('a question a person asked without a session is answered and sends nothing, and its twin with a session sends to it', async ($, on) => {
  mock.clock(on, { now: NOW })
  const { session: _, ...asked } = MERGE
  const byPerson = { ...asked, person: 'ana' }
  const { sent, toasts } = engine(on, { open: () => ({ exitCode: 0, stdout: opened([byPerson, COUNT]) }) })
  await start($)
  const ui = await $.ui.mount({ ...pane('questions'), surface: 'terminal' })
  await ui.press({ key: `${MERGE.path}#a` })
  expect(sent).toEqual([])
  expect(toasts).toEqual(['Answered and recorded in Board; ana asked without a Claude Code session, so nothing was sent.'])
  await ui.input({ key: `${COUNT.path}#words`, text: 'About forty.' })
  expect(sent.map(s => s.to)).toEqual(['local_ab12'])
  expect(toasts.pop()).toBe('Answered, and sent to local_ab12')
})

test('a question a person asked without a session is sent to the sessions merged into it, and the toast names them', async ($, on) => {
  mock.clock(on, { now: NOW })
  const { session: _, ...asked } = MERGE
  const byPerson = { ...asked, person: 'ana', also_asked: ['local_ef56', 'local_gh78'] }
  const { sent, toasts } = engine(on, { open: () => ({ exitCode: 0, stdout: opened([byPerson, COUNT]) }) })
  await start($)
  const ui = await $.ui.mount({ ...pane('questions'), surface: 'terminal' })
  await ui.press({ key: `${MERGE.path}#a` })
  expect(sent.map(s => s.to)).toEqual(['local_ef56', 'local_gh78'])
  expect(toasts).toEqual(['Answered and recorded in Board; ana asked without a Claude Code session, so it was sent to local_ef56 and local_gh78, which asked the same.'])
})

test('a send the asking session cannot take still records the answer, and says the message was not delivered', async ($, on) => {
  mock.clock(on, { now: NOW })
  const { ran, toasts } = engine(on, { delivered: false })
  await start($)
  const ui = await $.ui.mount({ ...pane('questions'), surface: 'terminal' })
  await ui.press({ key: `${MERGE.path}#b` })
  expect(ran.map(r => r[2])).toEqual(['plans.open', 'plans.answer'])
  expect(toasts).toEqual(['Answered and recorded in Board. The message to local_ab12 was not delivered'])
})

test('an answer plans.answer refuses sends nothing, keeps the question listed, and the toast gives the code', async ($, on) => {
  mock.clock(on, { now: NOW })
  const refused = JSON.stringify({
    schema: 'board/failure/v1',
    code: 'plans::answer::already_answered',
    message: `${MERGE.path} already has an answer at /projects/home/answers/20261024T1900Z-k3b`,
  })
  const { sent, toasts } = engine(on, { answer: () => ({ exitCode: 1, stdout: refused }) })
  await start($)
  const ui = await $.ui.mount({ ...pane('questions'), surface: 'terminal' })
  await ui.press({ key: `${MERGE.path}#a` })
  expect(sent).toEqual([])
  expect(toasts).toEqual([`No answer was recorded: plans::answer::already_answered ${MERGE.path} already has an answer at /projects/home/answers/20261024T1900Z-k3b`])
  expect(await ui.find({ type: 'Text', text: 'Merge home#201?' })).toBeDefined()
})

test('a plans.answer that times out or ends without a failure report may have recorded the answer, so nothing is sent', async ($, on) => {
  mock.clock(on, { now: NOW })
  const maybe = 'The answer may have been recorded, because board did not finish. The question stays in the list until plans.open stops listing it.'
  let outcome: { exitCode: number; stdout: string } | 'timeout' = 'timeout'
  const { sent, toasts } = engine(on, { answer: () => outcome })
  await start($)
  const ui = await $.ui.mount({ ...pane('questions'), surface: 'terminal' })
  await ui.press({ key: `${MERGE.path}#a` })
  outcome = { exitCode: 137, stdout: '' }
  await ui.press({ key: `${MERGE.path}#a` })
  expect(sent).toEqual([])
  expect(toasts).toEqual([maybe, maybe])
  expect(await ui.find({ type: 'Text', text: 'Merge home#201?' })).toBeDefined()
})

test('an answer whose attach was refused says it is recorded but not attached, and names the change and the code; an attached one says nothing of it', async ($, on) => {
  mock.clock(on, { now: NOW })
  const elsewhere = '/vendor/projects/home/answers/20261024T1830Z-k3'
  const cases = [
    { attached: { change: '/changes/fix-the-sync', refused: 'changes::attach::no_plan' }, toast: 'Answered, and sent to local_ab12. The answer is recorded but was not attached to /changes/fix-the-sync: changes::attach::no_plan' },
    { attached: { change: '/changes/fix-the-sync' }, toast: 'Answered, and sent to local_ab12' },
  ]
  let attached: object = cases[0]?.attached ?? {}
  const { sent, toasts } = engine(on, { answer: () => ({ exitCode: 0, stdout: answered(elsewhere, attached) }) })
  await start($)
  for (const [i, each] of cases.entries()) {
    attached = each.attached
    const ui = await $.ui.mount({ ...pane('questions'), surface: 'terminal' })
    // The first answer takes MERGE off the list, so the twin answers COUNT in words.
    if (i === 0) await ui.press({ key: `${MERGE.path}#a` })
    else await ui.input({ key: `${COUNT.path}#words`, text: 'About 40.' })
    expect(toasts.pop()).toBe(each.toast)
    // The message names the answer's path as plans.answer returned it.
    expect(sent.pop()?.text).toContain(`The answer is recorded at ${elsewhere}.`)
    await ui.unmount()
  }
})

test('an answer whose attach was refused and whose message was not delivered says once that it is recorded', async ($, on) => {
  mock.clock(on, { now: NOW })
  const attached = { change: '/changes/fix-the-sync', refused: 'changes::attach::no_plan' }
  const { toasts } = engine(on, { delivered: false, answer: () => ({ exitCode: 0, stdout: answered(ANSWER, attached) }) })
  await start($)
  const ui = await $.ui.mount({ ...pane('questions'), surface: 'terminal' })
  await ui.press({ key: `${MERGE.path}#a` })
  expect(toasts).toEqual([
    'Answered and recorded in Board, but not attached to /changes/fix-the-sync: changes::attach::no_plan. The message to local_ab12 was not delivered',
  ])
  expect(toasts[0]?.match(/recorded/g)).toHaveLength(1)
})

test('/yesterday reads the day from 07:00 in one query, opens the report pane and prints its totals', async ($, on) => {
  mock.clock(on, { now: NOW })
  const day = rows(
    ['kind', 'path', 'at', 'title', 'detail'],
    [
      ['since', null, '2026-10-24 07:00:00', null, null],
      ['decision', '/projects/board/decisions/d1', '2026-10-24T10:00:00Z', 'Keep the types at v1.', 'wanted'],
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
  expect(result.text).toBe('board: 1 decision, 0 questions asked, 0 answered, 1 run and 0 decision stubs that want a why, since 2026-10-24 07:00.')
  const ui = await $.ui.mount({ ...pane('yesterday'), surface: 'desktop' })
  expect((await ui.find({ type: 'Markdown' }))?.text).toContain('- Keep the types at v1. `/projects/board/decisions/d1`, review wanted')
})

test("a question's detail in the report is answered when an answer names it, and its status otherwise", () => {
  expect(REPORT_SQL).toContain(
    "CASE WHEN EXISTS (SELECT 1 FROM records a, records q WHERE q.logical = r.logical AND a.type = 'plans/answer/v1' AND " + NAMES + ')',
  )
  expect(REPORT_SQL).toContain("THEN 'answered' ELSE json_extract(r.body, '$.status') END")
  expect(REPORT_SQL).toContain(`LEFT JOIN records q ON q.type = 'plans/question/v1' AND ${NAMES}`)
})

test("a stub's detail in the report is the first of its urls, where the runs mod writes the pull request", () => {
  expect(REPORT_SQL).toContain("json_extract(r.body, '$.urls[0]')")
  expect(REPORT_SQL).not.toContain('$.refs')
})

test('the report lists each kind of record under its heading, and says none where there is none', () => {
  const { markdown, line } = report(
    [
      { kind: 'since', path: null, at: '2026-10-24 07:00:00', title: null, detail: null },
      { kind: 'asked', path: '/projects/core/questions/q1', at: '2026-10-24T09:00:00Z', title: 'Which region?', detail: 'answered' },
      { kind: 'answered', path: '/projects/core/answers/q1', at: '2026-10-24T10:00:00Z', title: 'Which region?', detail: 'eu-west-1' },
      { kind: 'stub', path: '/projects/widgets/decisions/s1', at: '2026-10-20T09:00:00Z', title: 'Widgets ship in blue', detail: 'https://github.com/acme/widgets/pull/42' },
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
      '## Decision stubs that still want a why (1)\n\n- Widgets ship in blue https://github.com/acme/widgets/pull/42 `/projects/widgets/decisions/s1`',
      'Board returned only the first rows; `board query` shows the rest.',
    ].join('\n\n'),
  )
  expect(line).toBe('board: 0 decisions, 1 question asked, 1 answered, 0 runs and 1 decision stub that want a why, since 2026-10-24 07:00, counting only the first 1000 records.')
})

test('a question shows the change it blocks by name, or the work in words, or nothing', () => {
  expect(blocking({ blocks: '/changes/fix-the-sync', blocked_work: 'the release' })).toBe(' · blocks fix-the-sync')
  expect(blocking({ blocks: null, blocked_work: 'the release' })).toBe(' · blocks the release')
  expect(blocking({ blocks: null, blocked_work: null })).toBe('')
})

test('an answer whose chosen option names the change it was attached to asks the owner to approve that change, and one that names no change does not', async ($, on) => {
  mock.clock(on, { now: NOW })
  const change = '/changes/merge-home'
  const approving = { ...MERGE, options: [{ id: 'a', text: `Merge, approving ${change}` }] }
  const longer = { ...MERGE, path: '/projects/home/questions/20261024T1900Z-k4', options: [{ id: 'b', text: `Hold ${change}-two` }] }
  const { ran, toasts } = engine(on, {
    open: () => ({ exitCode: 0, stdout: opened([approving, longer]) }),
    answer: () => ({ exitCode: 0, stdout: answered(ANSWER, { change }) }),
  })
  await start($)
  const ui = await $.ui.mount({ ...pane('questions'), surface: 'terminal' })
  await ui.press({ key: `${MERGE.path}#a` })
  expect(ran.slice(-1)).toEqual([['--personal', 'call', 'changes.approve', `--path=${change}`]])
  expect(toasts.pop()).toBe(`Answered, and sent to local_ab12. The approval of ${change} waits for Touch ID in the effects band`)
  await ui.unmount()
  // The twin: its option names only a longer path, so nothing is asked.
  const twin = await $.ui.mount({ ...pane('questions'), surface: 'terminal' })
  await twin.press({ key: `${longer.path}#b` })
  expect(ran.filter(r => r[2] === 'changes.approve')).toHaveLength(1)
  expect(toasts.pop()).toBe('Answered, and sent to local_ab12')
})

test('a changes.approve that fails keeps the answer and the toast says no approval was asked and how to ask', async ($, on) => {
  mock.clock(on, { now: NOW })
  const change = '/changes/merge-home'
  const approving = { ...MERGE, options: [{ id: 'a', text: `Approve ${change}.` }] }
  const refusal = JSON.stringify({ schema: 'board/failure/v1', code: 'changes::approve::not_chosen', message: 'no answer' })
  const { toasts } = engine(on, {
    open: () => ({ exitCode: 0, stdout: opened([approving, COUNT]) }),
    answer: () => ({ exitCode: 0, stdout: answered(ANSWER, { change }) }),
    approve: () => ({ exitCode: 1, stdout: refusal }),
  })
  await start($)
  const ui = await $.ui.mount({ ...pane('questions'), surface: 'terminal' })
  await ui.press({ key: `${MERGE.path}#a` })
  expect(toasts).toEqual([
    `Answered, and sent to local_ab12. No approval of ${change} was asked (changes::approve::not_chosen no answer); run board call changes.approve --path ${change}`,
  ])
})

test('the toast words the approval by the state changes.approve returns', async ($, on) => {
  mock.clock(on, { now: NOW })
  const change = '/changes/merge-home'
  const approving = { ...MERGE, options: [{ id: 'a', text: `Approve ${change}.` }] }
  let state = 'approved'
  const { toasts } = engine(on, {
    open: () => ({ exitCode: 0, stdout: opened([approving, { ...approving, path: '/projects/home/questions/20261024T1900Z-k4' }]) }),
    answer: () => ({ exitCode: 0, stdout: answered(ANSWER, { change }) }),
    approve: () => ({ exitCode: 0, stdout: JSON.stringify({ schema: 'board/call-report/v2', data: { output: { state, effect: 'e1' } } }) }),
  })
  await start($)
  const ui = await $.ui.mount({ ...pane('questions'), surface: 'terminal' })
  await ui.press({ key: `${MERGE.path}#a` })
  expect(toasts.pop()).toBe(`Answered, and sent to local_ab12. The owner has already approved ${change} as its plan stands`)
  state = 'refused'
  await ui.press({ key: '/projects/home/questions/20261024T1900Z-k4#a' })
  expect(toasts.pop()).toBe(`Answered, and sent to local_ab12. The owner refused the approval of ${change} earlier; delete /effects/seen/e1 in the personal board to ask again`)
  expect(approvalText(change, { state: 'asked' })).toBe(`. The approval of ${change} waits for Touch ID in the effects band`)
})

test('a path is named whole only when the code points on either side do not join it, a letter outside the BMP included', () => {
  expect(namesWhole('Approve /changes/t.', '/changes/t')).toBe(true)
  expect(namesWhole('\u{1D400}/changes/t', '/changes/t')).toBe(false)
  expect(namesWhole('/changes/t\u{1D400}', '/changes/t')).toBe(false)
  expect(namesWhole('\u{1F600} /changes/t', '/changes/t')).toBe(true)
  expect(namesWhole('/changes/t-two', '/changes/t')).toBe(false)
})

test('a path followed by a fragment or a query is part of a longer name, and a sentence ending after it is not', () => {
  expect(namesWhole('/changes/t#suite-run', '/changes/t')).toBe(false)
  expect(namesWhole('https://claude.ai/artifact/m1?v=2', 'https://claude.ai/artifact/m1')).toBe(false)
  expect(namesWhole('Approve /changes/t?', '/changes/t')).toBe(true)
  expect(namesWhole('Waive /changes/t#suite-run', '/changes/t#suite-run')).toBe(true)
  expect(namesWhole('Waive /changes/t#suite-runs', '/changes/t#suite-run')).toBe(false)
})

test('a path followed by a dot and a letter or digit is part of a longer name, and a dot that ends a sentence is not', () => {
  expect(namesWhole('Agree https://claude.ai/artifact/m1.png', 'https://claude.ai/artifact/m1')).toBe(false)
  expect(namesWhole('Agree /mockups/x.html', '/mockups/x')).toBe(false)
  expect(namesWhole('Agree /mockups/x.', '/mockups/x')).toBe(true)
  expect(namesWhole('Approve /changes/t. Then build it.', '/changes/t')).toBe(true)
  expect(namesWhole('Waive /changes/t#suite-run.', '/changes/t#suite-run')).toBe(true)
})
