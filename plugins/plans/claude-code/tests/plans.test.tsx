import { expect, mock, test } from 'claude-code/testing'
import type { On } from 'claude-code'
import type { Engine } from 'claude-code/testing'

import { bandRows } from '../hooks/band'
import { begin, countCallsForList, end, isDue, LOST_MS, schedule, tickIsDue } from '../hooks/refresh'
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
  /** What changes.ready answers; by default the board has no changes plugin. */
  ready?: () => { exitCode: number; stdout: string }
  delivered?: boolean
  report?: string
  host?: string | null
}

const NO_CHANGES = JSON.stringify({ schema: 'board/failure/v1', code: 'board::plugin::not_installed', message: 'plugin changes is not installed' })

/** Stands in for `board` and the engine: what each command answers, and what the mod ran, opened, sent, submitted, wrote and toasted. */
function engine(on: On, answers: Answers = {}) {
  const ran: string[][] = []
  const panes: string[] = []
  const sent: { to: unknown; text: string }[] = []
  const toasts: string[] = []
  let questions = [MERGE, COUNT]
  const whole = { stderr: '', isStdoutTruncated: false, isStderrTruncated: false }
  mock.env(on, { HOME: '/home/me', ...(answers.host === null ? {} : { CLAUDE_CODE_HOST_SESSION_ID: answers.host ?? 'local_cd34' }) })
  const waited: string[][] = []
  const readied: string[][] = []
  const submitted: string[] = []
  const disk = new Map<string, string>()
  const limits: (number | undefined)[] = []
  on('process.run', (_, e) => {
    const args = e.argv.slice(e.argv.indexOf('--json') + 1)
    if (args[1] === 'plans.waiting') {
      waited.push(e.argv.slice(4))
      return { value: { ...(answers.waiting?.() ?? { exitCode: 1, stdout: ABSENT }), ...whole } }
    }
    if (args[1] === 'changes.ready') {
      readied.push(e.argv.slice(4))
      return { value: { ...(answers.ready?.() ?? { exitCode: 1, stdout: NO_CHANGES }), ...whole } }
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
  on('fs.read', (_, e) => {
    const text = disk.get(e.path)
    if (text === undefined) throw new Error('missing')
    return { value: text }
  })
  on('fs.write', (_, e) => {
    disk.set(e.path, e.text)
    return { value: undefined }
  })
  on('command.run', (_, e) => (submitted.push(`/${e.command}`), { text: '' }))
  on('session.id', () => ({ value: 'local_cd34' }))
  on('command.register', (_, e) => ({ value: { command: e.name } }))
  on('ui.render', ($, e) => $.ui.resolve(e).Box({}))
  on('session.start', (_, e) => ({ cwd: e.cwd }))
  return { ran, waited, readied, limits, panes, sent, toasts, submitted, disk }
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

test('once the band is drawn, each minute it is drawn in reads the count again, in a desktop session too', { options: { board: '/boards/me' } }, async ($, on) => {
  const clock = mock.clock(on, { now: NOW })
  const { ran } = engine(on)
  // Desktop sessions run through the Agent SDK and report isInteractive false.
  await $.session.start({ cwd: '/work', surface: 'desktop', isInteractive: false })
  await (await $.ui.mount({ ...BAND, surface: 'desktop' })).unmount()
  await clock.advance(60_000)
  await (await $.ui.mount({ ...BAND, surface: 'desktop' })).unmount()
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

  // The answer reads the open list again after it.
  expect(ran.slice(1)).toEqual([['--personal', ...answerCall(MERGE.path, 'a', 'Merge', 'local_cd34')], ['--personal', 'call', 'plans.open', '--limit', '100']])
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
  expect(ran.slice(1, 2)).toEqual([['--personal', ...answerCall(COUNT.path, 'none', '-40, so ask Kechen for the exact figure.')]])
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
  expect(ran.map(r => r[2])).toEqual(['plans.open', 'plans.answer', 'plans.open'])
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
  expect(ran.filter(r => r[2] === 'changes.approve')).toEqual([['--personal', 'call', 'changes.approve', `--path=${change}`]])
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

// When the count and the list are read again. A draw here mounts the band and
// unmounts it at once, because a band left mounted is drawn again by each
// count the mod reads, which a session nobody looks at does not do.
type Clock = ReturnType<typeof mock.clock>
const draw = async ($: Engine, clock: Clock, component: typeof BAND | ReturnType<typeof pane> = BAND) => {
  await (await $.ui.mount({ ...component, surface: 'terminal' })).unmount()
  // A read the draw starts runs once the draw is done.
  await clock.advance(0)
}
const reads = (ran: string[][], waited: string[][]) => ({ waiting: waited.length, open: ran.filter(r => r.includes('plans.open')).length })

test('a minute with no draw of the band reads nothing, and its twin with a draw before each tick reads what waits each minute', async ($, on) => {
  const clock = mock.clock(on, { now: NOW })
  const { ran, waited } = engine(on, { waiting: () => ({ exitCode: 0, stdout: waitingReport(3, 1) }) })
  await start($)
  await draw($, clock)
  await clock.advance(60_000)
  expect(reads(ran, waited)).toEqual({ waiting: 2, open: 1 })
  for (let i = 0; i < 10; i++) await clock.advance(60_000)
  expect(reads(ran, waited)).toEqual({ waiting: 2, open: 1 })
  for (let i = 0; i < 10; i++) {
    await draw($, clock)
    await clock.advance(60_000)
  }
  // The first of these draws comes ten minutes after the last read, so it reads at once.
  expect(reads(ran, waited)).toEqual({ waiting: 13, open: 1 })
})

test('a draw after skipped minutes reads at once, and a second draw within the minute does not', async ($, on) => {
  const clock = mock.clock(on, { now: NOW })
  const { ran, waited } = engine(on, { waiting: () => ({ exitCode: 0, stdout: waitingReport(3, 1) }) })
  await start($)
  await draw($, clock)
  for (let i = 0; i < 6; i++) await clock.advance(60_000)
  expect(reads(ran, waited)).toEqual({ waiting: 2, open: 1 })
  await clock.advance(10_000)
  await draw($, clock)
  expect(reads(ran, waited)).toEqual({ waiting: 3, open: 1 })
  await clock.advance(30_000)
  await draw($, clock)
  expect(reads(ran, waited)).toEqual({ waiting: 3, open: 1 })
})

test('the open list is read again only when the count of what waits changes', async ($, on) => {
  const clock = mock.clock(on, { now: NOW })
  let total = 3
  const { ran, waited } = engine(on, { waiting: () => ({ exitCode: 0, stdout: waitingReport(total, 1) }) })
  await start($)
  for (let i = 0; i < 10; i++) {
    await draw($, clock)
    await clock.advance(60_000)
  }
  expect(reads(ran, waited)).toEqual({ waiting: 11, open: 1 })
  total = 4
  await draw($, clock)
  await clock.advance(60_000)
  expect(reads(ran, waited)).toEqual({ waiting: 12, open: 2 })
  await draw($, clock)
  await clock.advance(60_000)
  expect(reads(ran, waited)).toEqual({ waiting: 13, open: 2 })
})

test('the questions pane reads the open list when it is older than a minute, and not again within the minute', async ($, on) => {
  const clock = mock.clock(on, { now: NOW })
  const { ran, waited } = engine(on, { waiting: () => ({ exitCode: 0, stdout: waitingReport(3, 1) }) })
  await start($)
  await draw($, clock, pane('questions'))
  expect(reads(ran, waited)).toEqual({ waiting: 1, open: 1 })
  await clock.advance(61_000)
  await draw($, clock, pane('questions'))
  expect(reads(ran, waited)).toEqual({ waiting: 1, open: 2 })
  await clock.advance(30_000)
  await draw($, clock, pane('questions'))
  expect(reads(ran, waited)).toEqual({ waiting: 1, open: 2 })
})

test('without plans.waiting each drawn minute reads the open list, because the button counts it', async ($, on) => {
  const clock = mock.clock(on, { now: NOW })
  const { ran, waited } = engine(on)
  await start($)
  for (let i = 0; i < 3; i++) {
    await draw($, clock)
    await clock.advance(60_000)
  }
  expect(reads(ran, waited)).toEqual({ waiting: 1, open: 4 })
})

test('an answer reads what waits and the open list again', async ($, on) => {
  mock.clock(on, { now: NOW })
  const { ran, waited } = engine(on, { waiting: () => ({ exitCode: 0, stdout: waitingReport(2, 1) }) })
  await start($)
  const ui = await $.ui.mount({ ...pane('questions'), surface: 'terminal' })
  await ui.press({ key: `${MERGE.path}#a` })
  expect(ran.map(r => r[2])).toEqual(['plans.open', 'plans.answer', 'plans.open'])
  expect(waited).toHaveLength(2)
})

test('a failed count reads no open list, and a count read beside no earlier list reads it', () => {
  const s = schedule()
  expect(countCallsForList(s, { kind: 'failed' }, NOW)).toBe(false)
  expect(countCallsForList(s, { kind: 'read', total: 0 }, NOW)).toBe(true)
  end(s, 'list', { worked: true, total: 0 })
  expect(countCallsForList(s, { kind: 'read', total: 0 }, NOW)).toBe(false)
  // A list read that failed leaves the total it was read beside, so the changed count retries it.
  expect(countCallsForList(s, { kind: 'read', total: 1 }, NOW)).toBe(true)
  end(s, 'list', { worked: false, total: 1 })
  expect(countCallsForList(s, { kind: 'read', total: 1 }, NOW)).toBe(true)
})

test('a read whose callback never ran stops later reads only until it counts as lost', () => {
  const s = schedule()
  begin(s, 'count', NOW)
  s.drawn = true
  expect(tickIsDue(s, NOW + LOST_MS - 1)).toBe(false)
  expect(tickIsDue(s, NOW + LOST_MS)).toBe(true)
  expect(isDue(s.count, NOW + 60_000)).toBe(true)
  // The twin: a read that ended is not running, so the tick reads at once.
  end(s, 'count')
  expect(tickIsDue(s, NOW + 1)).toBe(true)
})

test('a drawn minute whose plans.waiting fails reads no open list and keeps the last count', async ($, on) => {
  const clock = mock.clock(on, { now: NOW })
  let fails = false
  const down = { exitCode: 1, stdout: JSON.stringify({ schema: 'board/failure/v1', code: 'board::query::timeout', message: 'the query ran longer than 1s' }) }
  const { ran, waited } = engine(on, { waiting: () => (fails ? down : { exitCode: 0, stdout: waitingReport(3, 1) }) })
  await start($)
  fails = true
  for (let i = 0; i < 3; i++) {
    await draw($, clock)
    await clock.advance(60_000)
  }
  expect(reads(ran, waited)).toEqual({ waiting: 4, open: 1 })
  const band = await $.ui.mount({ ...BAND, surface: 'terminal' })
  expect((await band.find({ key: 'waiting' }))?.text).toBe('Waiting 3 · 1 blocking: the last read failed')
})

test('a band that stays on screen is drawn again by each read, so it reads what waits every minute', async ($, on) => {
  const clock = mock.clock(on, { now: NOW })
  const { ran, waited } = engine(on, { waiting: () => ({ exitCode: 0, stdout: waitingReport(3, 1) }) })
  await start($)
  const band = await $.ui.mount({ ...BAND, surface: 'terminal' })
  for (let i = 0; i < 5; i++) await clock.advance(60_000)
  expect(reads(ran, waited)).toEqual({ waiting: 6, open: 1 })
  await band.unmount()
  for (let i = 0; i < 5; i++) await clock.advance(60_000)
  expect(reads(ran, waited)).toEqual({ waiting: 7, open: 1 })
})

/** A change ready to land at `/changes/<name>`, as changes.ready lists it, with `fields`. */
const landing = (name: string, fields: Record<string, unknown> = {}) => ({
  path: `/changes/${name}`,
  title: `The ${name} change`,
  pull: '/gh/crumbkit/board/pulls/288',
  head: 'a3f9c0de1234567890abcdef1234567890abcdef',
  kind: 'recheck',
  check: 'rechecks',
  waits_on: 'your recheck of two reworded statements',
  questions: [MERGE.path],
  days: 1,
  failing: null,
  ...fields,
})
const readyOf = (changes: object[]) => () => ({
  exitCode: 0,
  stdout: JSON.stringify({ schema: 'board/call-report/v2', data: { plugin: 'changes', capability: 'ready', output: { changes, total: changes.length, truncated: false, unreadable: 0 } } }),
})
const STATE = '/home/me/.local/state/board/ready-failing.jsonl'

/** The keys of the band row's parts. */
const partKeys = async (band: { drawn: () => Promise<unknown> }) => {
  const row = bandRows((await band.drawn()) as Parameters<typeof bandRows>[0])[0]
  return row?.children?.map(c => (typeof c === 'object' && c.type === 'Box' ? c.props?.key : c)) ?? []
}

test('the band draws Ready to land N just before Waiting, green, and its button opens the Ready to land pane', async ($, on) => {
  mock.clock(on, { now: NOW })
  const { panes, readied } = engine(on, { waiting: () => ({ exitCode: 0, stdout: waitingReport(11, 8) }), ready: readyOf([landing('t'), landing('u'), landing('v')]) })
  await start($)
  const band = await $.ui.mount({ ...BAND, surface: 'terminal' })
  expect((await band.find({ key: 'ready' }))?.text).toBe('Ready to land 3')
  expect((await band.find({ type: 'Text', text: 'Ready to land 3' }))?.props?.color).toBe('success')
  expect(await partKeys(band)).toEqual(['band:b3-ready', 'band:b3-waiting'])
  await band.press({ key: 'ready' })
  expect(panes).toEqual(['ready'])
  expect(readied).toEqual([['--personal', '--json', 'call', 'changes.ready']])
})

test('with nothing ready, or no changes plugin, the band draws no Ready to land button, and a board without the plugin is not asked again', async ($, on) => {
  const clock = mock.clock(on, { now: NOW })
  const { readied } = engine(on, { waiting: () => ({ exitCode: 0, stdout: waitingReport(8, 5) }) })
  await start($)
  const band = await $.ui.mount({ ...BAND, surface: 'terminal' })
  expect(await band.find({ key: 'ready' })).toBeUndefined()
  expect((await band.find({ key: 'waiting' }))?.text).toBe('Waiting 8 · 5 blocking')
  await clock.advance(60_000)
  expect(readied).toHaveLength(1)
})

test('a ready list with no change draws no button', async ($, on) => {
  mock.clock(on, { now: NOW })
  engine(on, { ready: readyOf([]) })
  await start($)
  const band = await $.ui.mount({ ...BAND, surface: 'terminal' })
  expect(await band.find({ key: 'ready' })).toBeUndefined()
})

test('while the red part is silent a failing change counts in N and draws nothing red, and with the option shown it draws the red part', async ($, on) => {
  mock.clock(on, { now: NOW })
  engine(on, { ready: readyOf([landing('t'), landing('u', { failing: 'ci_failed' })]) })
  await start($)
  const band = await $.ui.mount({ ...BAND, surface: 'terminal' })
  expect((await band.find({ key: 'ready' }))?.text).toBe('Ready to land 2')
  const drawn = await $.ui.mount({ ...pane('ready'), surface: 'terminal' })
  expect(await drawn.find({ type: 'Text', text: /failing|CI failed/ })).toBeUndefined()
})

test('with the failing option shown the button and the pane draw the failing change in red, last, with the step to see why', { options: { failing: 'shown' } }, async ($, on) => {
  mock.clock(on, { now: NOW })
  engine(on, { ready: readyOf([landing('t'), landing('u', { failing: 'ci_failed', kind: 'data-sign-off', days: 2 })]) })
  await start($)
  const band = await $.ui.mount({ ...BAND, surface: 'terminal' })
  expect((await band.find({ key: 'ready' }))?.text).toBe('Ready to land 1 · 1 failing')
  const shown = await $.ui.mount({ ...pane('ready'), surface: 'terminal' })
  expect((await shown.findAll({ type: 'Text' })).map(t => t.text)).toEqual([
    'Ready to land · 1',
    '· 1 failing',
    'crumbkit/board#288',
    'ready 1d',
    ' Recheck ',
    'The t change',
    'Waits on: your recheck of two reworded statements',
    'Clears it:',
    'crumbkit/board#288',
    'ready 2d',
    ' Data sign-off ',
    ' CI failed ',
    'The u change',
    'Waits on: your recheck of two reworded statements. Its CI failed, and the orchestrator is fixing it first. Nothing is asked of you until CI passes.',
    'To see why:',
    'board changes next --path /changes/u',
  ])
  // The kind is a tag and the failing reason a red one.
  expect((await shown.find({ type: 'Text', text: ' Data sign-off ' }))?.props?.inverse).toBe(true)
  const red = (await shown.find({ type: 'Text', text: ' CI failed ' }))?.props
  expect([red?.inverse, red?.color]).toEqual([true, 'red'])
})

test('a failed read keeps the last count and says the last read failed', async ($, on) => {
  const clock = mock.clock(on, { now: NOW })
  let fails = false
  const down = { exitCode: 1, stdout: JSON.stringify({ schema: 'board/failure/v1', code: 'board::query::timeout', message: 'the query ran longer than 1s' }) }
  const ok = readyOf([landing('t'), landing('u'), landing('v')])
  engine(on, { waiting: () => ({ exitCode: 0, stdout: waitingReport(11, 8) }), ready: () => (fails ? down : ok()) })
  await start($)
  await (await $.ui.mount({ ...BAND, surface: 'terminal' })).unmount()
  fails = true
  await clock.advance(60_000)
  const band = await $.ui.mount({ ...BAND, surface: 'terminal' })
  expect((await band.find({ key: 'ready' }))?.text).toBe('Ready to land 3: the last read failed')
  expect((await band.find({ type: 'Text', text: 'Ready to land 3: the last read failed' }))?.props?.color).toBe('red')
})

test('the pane lists each change with its pull request, age, kind and check, and Ask these now and Answer in /ask send /ask', async ($, on) => {
  mock.clock(on, { now: NOW })
  const { submitted } = engine(on, { ready: readyOf([landing('t', { kind: 'land-choice', check: null, waits_on: 'Land it now or after #297?', days: 0 })]) })
  await start($)
  const shown = await $.ui.mount({ ...pane('ready'), surface: 'desktop' })
  expect((await shown.findAll({ type: 'Text' })).map(t => t.text)).toEqual([
    'Ready to land · 1',
    'crumbkit/board#288',
    'ready today',
    ' Land choice ',
    'The t change',
    'Waits on: Land it now or after #297?',
    'Clears it:',
  ])
  expect((await shown.find({ key: 'ask-all' }))?.text).toBe('Ask these now (/ask)')
  await shown.press({ key: 'ask-all' })
  await shown.press({ key: '/changes/t#ask' })
  expect(submitted).toEqual(['/ask', '/ask'])
})

test('each read records a change that starts or stops failing in the state file, while the red part is silent', async ($, on) => {
  const clock = mock.clock(on, { now: NOW })
  let rows = [landing('t'), landing('u', { failing: 'conflict' })]
  const { disk } = engine(on, { ready: () => readyOf(rows)() })
  await start($)
  const lines = () => (disk.get(STATE) ?? '').split('\n').filter(l => l !== '').map(l => JSON.parse(l) as Record<string, unknown>)
  expect(lines()).toEqual([
    { at: new Date(NOW).toISOString(), change: '/changes/u', pull: '/gh/crumbkit/board/pulls/288', head: landing('u').head, reason: 'conflict', session: 'local_cd34' },
  ])
  // A second read with nothing changed appends nothing.
  await (await $.ui.mount({ ...BAND, surface: 'terminal' })).unmount()
  await clock.advance(60_000)
  expect(lines()).toHaveLength(1)
  rows = [landing('t'), landing('u')]
  await (await $.ui.mount({ ...BAND, surface: 'terminal' })).unmount()
  await clock.advance(60_000)
  expect(lines().map(l => [l.change, l.reason])).toEqual([['/changes/u', 'conflict'], ['/changes/u', null]])
})

test('Waiting leaves out the questions Ready to land counts, so a check is counted once, and its pane is drawn as before', async ($, on) => {
  mock.clock(on, { now: NOW })
  engine(on, { waiting: () => ({ exitCode: 0, stdout: waitingReport(11, 8) }), ready: readyOf([landing('t', { questions: [COUNT.path] })]) })
  await start($)
  const band = await $.ui.mount({ ...BAND, surface: 'terminal' })
  expect((await band.find({ key: 'waiting' }))?.text).toBe('Waiting 10 · 7 blocking')
  const ui = await $.ui.mount({ ...pane('questions'), surface: 'terminal' })
  expect((await ui.findAll({ type: 'Text' })).map(t => t.text)).toEqual([
    'home · 15h · blocks home#201',
    'Merge home#201?',
    'core · 1h',
    'How many users have a blank email?',
  ])
})
