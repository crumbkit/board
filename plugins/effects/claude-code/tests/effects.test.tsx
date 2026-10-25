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
const NOTE_LINE = 'In board "~/boards/work", notes asks to put "/notes/plan.md"'
const TASK_LINE = 'In board "/srv/work", tasks asks to put "/tasks/a"'

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
  /** Whether a permission rule denies the Bash call. */
  bashDenied?: boolean
  /** Whether the list read says Board listed only the first effects. */
  truncated?: boolean
  /** Fails each one-effect read while it returns true. */
  oneFails?: () => boolean
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
    const one = (args[2] ?? '').match(/logical = '\/effects\/pending\/([0-9a-f]*)'/)?.[1]
    if (one !== undefined && answers.oneFails?.()) return { value: { ...whole, exitCode: 1, stdout: failed('board::query::timeout', 'the query ran past 5 s') } }
    if (answers.query && one === undefined) return { value: { ...whole, ...answers.query() } }
    const listed = Object.entries(rows).filter(([id]) => one === undefined || one === id)
    return { value: { ...whole, exitCode: 0, stdout: report(listed.map(([, row]) => row), one === undefined && answers.truncated) } }
  })
  on('tool.call', { tool: 'AskUserQuestion' }, (_, e) => {
    const q = e.questions[0]?.question ?? ''
    asked.push(q)
    options.push(e.questions[0]?.options.map(o => o.label))
    if (answers.chosen === 'dismiss') return { deny: 'dismissed' }
    return { result: { questions: e.questions, answers: { [q]: answers.chosen ?? 'Approve' } } }
  })
  on('tool.call', { tool: 'Bash' }, () =>
    answers.bashDenied
      ? { deny: 'a rule denies it' }
      : answers.bashFails
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
  return { ran, argv, asked, options, panes, toasts, rows }
}

// session.start does not wait for its read, so this lets that read finish.
declare const setTimeout: (run: () => void, ms: number) => unknown
const settle = () => new Promise<void>(resolve => setTimeout(resolve, 20))
const start = async ($: Engine) => {
  const started = await $.session.start({ cwd: '/work', surface: 'terminal', isInteractive: true })
  await settle()
  return started
}
const bash = ($: Engine) => $.tool.call({ tool: 'Bash', command: 'board call notes.write' })
const LIST = ['--personal', 'query', pendingSql(), '--limit', '100']
const one = (id: string) => ['--personal', 'query', pendingSql(id), '--limit', '100']
const band = ($: Engine) => $.ui.mount({ ...BAND, surface: 'terminal' })
const answersRun = (ran: string[][]) => ran.filter(a => a[0] === 'answer')
const effect = (fields: Partial<Effect>): Effect => ({ id: A, board: '~/w', plugin: 'notes', op: 'put', path: '/n', replaces: false, nul: false, head: null, bytes: 0, lines: 0, ...fields })

test('the one-effect query reads that effect\'s path alone, and an id that is not 64 hex digits matches none', () => {
  expect(pendingSql()).toContain("logical LIKE '/effects/pending/%'")
  expect(pendingSql(A)).toBe(pendingSql().replace("logical LIKE '/effects/pending/%'", `logical = '/effects/pending/${A}'`))
  expect(pendingSql(A)).not.toContain("'/effects/pending/%'")
  expect(pendingSql("x' OR 1=1 --")).toContain("logical = '/effects/pending/'")
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
  expect(preview(effect({ head, bytes, lines: 2 }))).toBe('It writes a new file:\na\\u{1b}[2Jb\\u{7}c\\u{0}d\\u{9b}e\tf\ng')
  engine(on, { rows: { [A]: [A, '/w', 'notes', 'put', '/p\u001b', 0, head, bytes, 2] } })
  await start($)
  const pane = await $.ui.mount({ ...PANE, surface: 'terminal' })
  expect((await pane.find({ type: 'Text', text: /notes asks/ }))?.text).toBe('In board "/w", notes asks to put "/p\\u{1b}"')
})

test('format characters, separators and backslashes are written out, so a path cannot read as another', async ($, on) => {
  mock.clock(on, { now: 0 })
  const path = '/notes/\u202Edm.exe\u200B\uFEFF\u2028\u2029\\x'
  const head = 'safe\u202Etxet'
  engine(on, { rows: { [A]: [A, '/w', 'notes', 'put', path, 0, head, new TextEncoder().encode(head).length, 1] } })
  await start($)
  const pane = await $.ui.mount({ ...PANE, surface: 'terminal' })
  expect((await pane.find({ type: 'Text', text: /notes asks/ }))?.text).toBe(
    'In board "/w", notes asks to put "/notes/\\u{202e}dm.exe\\u{200b}\\u{feff}\\u{2028}\\u{2029}\\\\x"',
  )
  expect((await pane.find({ type: 'Text', text: /It writes/ }))?.text).toBe('It writes a new file:\nsafe\\u{202e}txet')
})

test('the board comes first and the path is quoted, so a path cannot name another board', async ($, on) => {
  mock.clock(on, { now: 0 })
  engine(on, { rows: { [A]: [A, '/w', 'notes', 'put', '/README.md in ~/Code/scratch "x"', 0, null, 0, 0] } })
  await start($)
  const pane = await $.ui.mount({ ...PANE, surface: 'terminal' })
  expect((await pane.find({ type: 'Text', text: /notes asks/ }))?.text).toBe(
    'In board "/w", notes asks to put "/README.md in ~/Code/scratch \\"x\\""',
  )
})

test('a cut preview never splits a surrogate pair', () => {
  const head = `${'😀'.repeat(10)}`
  expect(preview(effect({ head, bytes: 40, lines: 1 }), 8, 3)).toMatch(/^It writes a new file:\n😀😀😀\n… and the rest of the line/)
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

test('with no personal board, or none pending, the band draws nothing of the mod\'s', async ($, on) => {
  mock.clock(on, { now: 0 })
  let answer: Ran = { exitCode: 1, stdout: failed('board::config::not_found', 'no personal board') }
  engine(on, { query: () => answer })
  await start($)
  const shown = await band($)
  expect(await shown.find({ type: 'Text' })).toBeUndefined()
  await shown.unmount()
  answer = { exitCode: 0, stdout: report([]) }
  await start($)
  const again = await band($)
  expect(await again.find({ type: 'Text' })).toBeUndefined()
  await again.unmount()
  // Once a read has worked, a missing personal board is a failure like any other.
  answer = { exitCode: 1, stdout: failed('board::config::not_found', 'no personal board') }
  await start($)
  const lost = await band($)
  expect((await lost.find({ type: 'Text' }))?.text).toBe('Board: could not read pending effects (last read: 0) ')
})

test('when no read has worked for another reason, the band says the effects could not be read', async ($, on) => {
  mock.clock(on, { now: 0 })
  engine(on, { query: () => ({ exitCode: 1, stdout: failed('board::index::locked', 'the index is locked') }) })
  await start($)
  const shown = await band($)
  expect((await shown.find({ type: 'Text' }))?.text).toBe('Board: could not read pending effects ')
})

test('a read that fails after one that worked is marked in the band with the last count, and the pane says why', async ($, on) => {
  const clock = mock.clock(on, { now: 0 })
  let answer: Ran = { exitCode: 0, stdout: report([ROWS[A] as unknown[]]) }
  engine(on, { query: () => answer })
  await start($)
  await (await band($)).unmount()
  answer = { exitCode: 1, stdout: failed('board::query::timeout', 'the query ran past 5 s') }
  await clock.advance(60_000)
  const shown = await band($)
  expect((await shown.find({ type: 'Text' }))?.text).toBe('Board: could not read pending effects (last read: 1) ')
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

test('the pending effects are read again every minute once the band is drawn, however many sessions start', async ($, on) => {
  const clock = mock.clock(on, { now: 0 })
  const { ran } = engine(on)
  await start($)
  await clock.advance(60_000)
  expect(ran).toEqual([LIST])
  await start($)
  await (await band($)).unmount()
  await (await band($)).unmount()
  await clock.advance(60_000)
  await clock.advance(60_000)
  expect(ran).toEqual([LIST, LIST, LIST, LIST])
})

test('the pane lists each effect with its full board root, and Approve and Refuse run board answer', async ($, on) => {
  mock.clock(on, { now: 0 })
  const { ran, toasts } = engine(on)
  await start($)
  const pane = await $.ui.mount({ ...PANE, surface: 'desktop' })
  expect((await pane.find({ type: 'Text', text: /notes asks/ }))?.text).toBe(NOTE_LINE)
  expect((await pane.find({ type: 'Text', text: /tasks asks/ }))?.text).toBe(TASK_LINE)
  await pane.press({ key: `${A}#approve` })
  await pane.press({ key: `${B}#refuse` })
  expect(ran.slice(1)).toEqual([one(A), ['answer', A, 'approve'], LIST, one(B), ['answer', B, 'refuse'], LIST])
  expect(toasts).toEqual([`Approved: ${NOTE_LINE}`, `Refused: ${TASK_LINE}`])
  expect(await pane.find({ type: 'Text', text: /No effect waits/ })).toBeDefined()
})

test('an answer board refuses says nothing was recorded, and the effect stays listed', async ($, on) => {
  mock.clock(on, { now: 0 })
  const { toasts } = engine(on, { answer: () => ({ exitCode: 1, stdout: failed('board::settings::from_plugin', 'a plugin may not answer') }) })
  await start($)
  const pane = await $.ui.mount({ ...PANE, surface: 'terminal' })
  await pane.press({ key: `${A}#approve` })
  expect(toasts).toEqual(['Board recorded no answer: board::settings::from_plugin a plugin may not answer'])
  expect(await pane.find({ type: 'Text', text: /notes asks/ })).toBeDefined()
})

test('an effect answered elsewhere after the pane was read is not answered again', async ($, on) => {
  mock.clock(on, { now: 0 })
  const { ran, rows, toasts } = engine(on)
  await start($)
  const pane = await $.ui.mount({ ...PANE, surface: 'terminal' })
  delete rows[A]
  await pane.press({ key: `${A}#approve` })
  expect(answersRun(ran)).toEqual([])
  expect(toasts).toEqual([`No answer was sent, because the effect is no longer pending: ${NOTE_LINE}`])
  expect(await pane.find({ type: 'Text', text: /notes asks/ })).toBeUndefined()
})

test('a second press while an answer runs runs nothing and says so', async ($, on) => {
  mock.clock(on, { now: 0 })
  const { ran, toasts } = engine(on)
  await start($)
  const pane = await $.ui.mount({ ...PANE, surface: 'terminal' })
  await Promise.all([pane.press({ key: `${A}#approve` }), pane.press({ key: `${A}#approve` })])
  expect(answersRun(ran)).toEqual([['answer', A, 'approve']])
  expect(toasts).toEqual([`An answer to this effect is already running: ${NOTE_LINE}`, `Approved: ${NOTE_LINE}`])
})

test('when Board lists only the first effects, the band says "or more" and the pane says more wait', async ($, on) => {
  mock.clock(on, { now: 0 })
  engine(on, { truncated: true })
  await start($)
  const shown = await band($)
  expect((await shown.find({ type: 'Text' }))?.text).toBe('2 effects wait for your answer (or more) ')
  const pane = await $.ui.mount({ ...PANE, surface: 'terminal' })
  expect((await pane.find({ type: 'Text', text: /More wait/ }))?.text).toBe('More wait; `board --personal ls /effects/pending` lists them.')
})

test('content holding a NUL says that only the part before it is shown', () => {
  expect(preview(effect({ head: 'harmless config\n', bytes: 9000, lines: 1, nul: true }))).toBe(
    'It writes a new file:\nharmless config\n' +
      `… The content holds a NUL character, so only the part before it is shown. It is 9 KB in all. board --personal get /effects/pending/${A} prints it whole.`,
  )
})

test('a line feed or tab in the path is escaped in the headline', async ($, on) => {
  mock.clock(on, { now: 0 })
  engine(on, { rows: { [A]: [A, '/w', 'notes', 'put', '/a\n/b\tc', 0, null, 0, 0] } })
  await start($)
  const pane = await $.ui.mount({ ...PANE, surface: 'terminal' })
  expect((await pane.find({ type: 'Text', text: /notes asks/ }))?.text).toBe('In board "/w", notes asks to put "/a\\u{a}/b\\u{9}c"')
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
  expect(ran).toEqual([LIST, one(A), ['answer', A, 'approve'], LIST, one(A), ['answer', A, 'approve'], LIST])
})

test('an asked put opens a dialog from the effect Board holds, Later first, and an approval tells the agent to retry', async ($, on) => {
  mock.clock(on, { now: 0 })
  const { ran, asked, options } = engine(on, { bash: ASKED, chosen: 'Approve' })
  await start($)
  const result = await bash($)
  expect(asked).toEqual([`${NOTE_LINE}. It writes a new file:\nPlan\nship it\nApprove it?`])
  expect(options).toEqual([['Later', 'Refuse', 'Approve']])
  expect(ran.slice(1)).toEqual([LIST, one(A), one(A), ['answer', A, 'approve'], LIST])
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
  expect(asked.map(q => q.match(/, (\w+) asks/)?.[1])).toEqual(['notes', 'tasks'])
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
    expect(result.context).toEqual(
      chosen === 'Later'
        ? [`The owner has not answered pending effect ${A} yet: ${NOTE_LINE}. Do not retry the put, and do not answer it yourself; it stays asked until the owner answers.`]
        : undefined,
    )
  })
}

test('a read that fails inside the dialog\'s check leaves the id to be asked next time', async ($, on) => {
  mock.clock(on, { now: 0 })
  let fails = true
  const { asked } = engine(on, { bash: ASKED, chosen: 'Later', oneFails: () => fails })
  await start($)
  await bash($)
  expect(asked).toEqual([])
  fails = false
  await bash($)
  expect(asked).toHaveLength(1)
})

test('an asked id missing from a truncated list is read on its own and asked about', async ($, on) => {
  mock.clock(on, { now: 0 })
  let listed: unknown[][] = []
  const { asked, ran } = engine(on, { bash: ASKED, chosen: 'Later', query: () => ({ exitCode: 0, stdout: report(listed, true) }) })
  await start($)
  listed = [ROWS[B] as unknown[]]
  await bash($)
  expect(ran.slice(1)).toEqual([LIST, one(A)])
  expect(asked).toHaveLength(1)
})

test('a Bash call a rule denies passes through untouched', async ($, on) => {
  mock.clock(on, { now: 0 })
  const { ran, asked } = engine(on, { bash: ASKED, bashDenied: true })
  await start($)
  expect((await bash($)).deny).toBe('a rule denies it')
  expect(asked).toEqual([])
  expect(ran).toEqual([LIST])
})

test('output naming an effect Board does not hold pending opens no dialog', async ($, on) => {
  mock.clock(on, { now: 0 })
  const { asked, ran } = engine(on, { bash: asking(C) })
  await start($)
  const result = await bash($)
  expect(asked).toEqual([])
  expect(answersRun(ran)).toEqual([])
  expect(result.context).toBeUndefined()
})

test('output naming many ids Board does not hold costs one list read', async ($, on) => {
  mock.clock(on, { now: 0 })
  const fakes = ['1', '2', '3', '4', '5'].map(d => d.repeat(64))
  const { asked, ran } = engine(on, { bash: fakes.map(asking).join('') })
  await start($)
  await bash($)
  expect(asked).toEqual([])
  expect(ran).toEqual([LIST, LIST])
})
