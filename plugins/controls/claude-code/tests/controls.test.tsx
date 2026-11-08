import { expect, test } from 'claude-code/testing'
import type { On } from 'claude-code'
import type { Engine } from 'claude-code/testing'

import { SLOT } from '../hooks/band'
import { badgeValue, boardName, contextText, controlsBadge, parseCheck, parseConfig, sections } from '../hooks/controls'

// These prove what a session is told about controls and when, with git and
// board stood in by a process.run hook. They do not run board: the Python
// tests prove what controls.check and controls.for-path answer.

const ROOT = '/work/crumbkit'
const DOCS = { control: '/controls/crumbkit/docs-1', requirement: 'AGENTS.md has at most 200 words.', budget: 'words', used: 196, max: 200, files: [] }
const README = { control: '/controls/example/readme', kind: 'standing', requirement: 'Keep the usage section of the README current.', files: 3, paths: [], purpose: 'p' }
const OWED = { control: '/controls/crumbkit/docs-sweep', kind: 'obligation', requirement: 'Sweep the docs for #12.', files: 1, paths: [], purpose: 'p' }
const PARSER = { control: '/controls/crumbkit/rust-10', requirement: 'A hand-written parser says what it misses.', purpose: 'p', rule: null }
const report = (output: object) => JSON.stringify({ schema: 'board/call-report/v2', data: { plugin: 'controls', output } })
const RUST = { control: '/controls/toolchains/rust', requirement: 'Pin the latest Rust.', at_least: '1.99.1', files: [], behind: [] }
const CHECK = report({ measured: [DOCS], stated: [README] })
const PANE_PROPS = { title: 'Controls', isFocused: false, bodyColumns: 80, placement: 'dock', scroll: { offset: 0, bodyRows: 40 }, view: {} } as const
const BAND = {
  plugin: 'board-controls',
  component: 'AbovePrompt',
  props: { hasSurvey: false, isWorking: false, maxRows: 10, bodyColumns: 80, scroll: { offset: 0, bodyRows: 10 }, view: {} },
} as const

type Answers = {
  check?: { exitCode: number; stdout: string }
  forPath?: Record<string, object[]> | 'fails'
  /** The known boards file's text; none when absent. */
  config?: string
  /** What each configured command prints, by its first argument; 'fails' exits 1, 'hangs' rejects. */
  commands?: Record<string, string>
  /** The repository `board ls /` names; none when no board holds the directory. */
  repository?: string
}

const CONFIG = '/home/me/.config/board/config.json'
const PERSONAL = '/home/me/Code/board'

/** Stands in for the engine beneath the plugin: git, board, the tool list, the status line and the transcript. */
function engine(on: On, answers: Answers) {
  const events: string[] = []
  const appended: string[] = []
  const run = (exitCode: number, stdout: string) => ({
    value: { exitCode, stdout, stderr: '', isStdoutTruncated: false, isStderrTruncated: false },
  })
  on('env.get', (_, e) => ({ value: e.name === 'HOME' ? '/home/me' : undefined }))
  on('fs.read', (_, e) => {
    if (e.path === CONFIG && answers.config !== undefined) return { value: answers.config }
    throw new Error(`ENOENT: ${e.path}`)
  })
  on('process.run', (_, e) => {
    const line = e.argv.join(' ')
    if (line.includes('rev-parse')) return run(0, `${ROOT}\n`)
    if (line.includes('ls / --limit 1')) {
      if (answers.repository === undefined) return run(1, JSON.stringify({ code: 'board::config::not_found', message: 'no board' }))
      return run(0, JSON.stringify({ schema: 'board/ls-report/v2', data: [], summary: { location: { repository: answers.repository } } }))
    }
    const command = answers.commands?.[e.argv[0] ?? '']
    if (command !== undefined) {
      events.push(`badge ${e.argv[0]} in ${e.init?.cwd}`)
      if (command === 'hangs') throw new Error('timed out')
      return command === 'fails' ? run(1, 'x\n') : run(0, command)
    }
    if (line.includes('controls.check')) {
      events.push('check')
      const check = answers.check ?? { exitCode: 0, stdout: CHECK }
      return run(check.exitCode, check.stdout)
    }
    const file = (e.argv.at(-1) ?? '').replace(/^\.\//, '')
    if (answers.forPath === 'fails') return run(2, '')
    events.push(`for-path ${e.init?.cwd}/${file}`)
    return run(0, report({ controls: answers.forPath?.[file] ?? [] }))
  })
  on('ui.open', () => ({ value: { isPlaced: true } }))
  on('session.append', (_, e, next) => {
    const block = e.message.content[0]
    appended.push(block?.type === 'text' ? String(block.text) : '')
    events.push('append')
    return next(e)
  })
  on('tool.call', () => {
    events.push('edit')
    return { result: {} }
  })
  on('ui.render', ($, e) => $.ui.resolve(e).Box({}))
  on('prompt.context', (_, e) => ({ blocks: e.blocks }))
  on('session.start', (_, e) => ({ cwd: e.cwd }))
  return { events, appended }
}

const start = ($: Engine) => $.session.start({ cwd: `${ROOT}/src`, surface: 'terminal', isInteractive: true })
const edit = ($: Engine, file: string) =>
  $.tool.call({ tool: 'Edit', file_path: `${ROOT}/${file}`, old_string: 'a', new_string: 'b', replace_all: false })

test('the context block lists each control with its requirement, a budget with its usage and a version rule with its floor', () => {
  const checked = parseCheck(ROOT, 0, report({ measured: [DOCS], versions: [RUST], stated: [README] }), '')
  expect(checked === null ? null : contextText(checked)).toBe(
    `The board's controls state requirements on files in ${ROOT}. Follow each one that applies to a file you ` +
      'change. `board get <control>` shows which files a control names.\n\n' +
      '- /controls/crumbkit/docs-1: AGENTS.md has at most 200 words. (now 196 of 200 words)\n' +
      '- /controls/toolchains/rust: Pin the latest Rust. (now every pin at least 1.99.1)\n' +
      '- /controls/example/readme: Keep the usage section of the README current.',
  )
})

test('a version control with pins behind and a budget over its max with an unreadable file say so in the block', () => {
  const behind = { ...RUST, behind: [{ file: '/r/rust-toolchain.toml', line: 2, found: '1.98.0' }] }
  const unread = { ...DOCS, used: 300, unreadable: [{ file: '/r/AGENTS.md', code: 'board::path::shadowed' }] }
  const checked = parseCheck(ROOT, 0, report({ passed: false, measured: [unread], versions: [behind], stated: [] }), '')
  expect(checked === null ? null : contextText(checked)).toContain(
    '- /controls/crumbkit/docs-1: AGENTS.md has at most 200 words. (now 300 of 200 words, over budget, 1 unreadable)\n' +
      '- /controls/toolchains/rust: Pin the latest Rust. (now 1 older than 1.99.1)',
  )
})

test('a repository no board holds has no controls, and a check that fails is said plainly', () => {
  const missing = JSON.stringify({ schema: 'board/failure/v1', code: 'board::config::not_found', message: 'no board' })
  expect(parseCheck(ROOT, 1, missing, '')).toBeNull()
  const over = JSON.stringify({ schema: 'board/failure/v1', code: 'controls::control::degraded', message: 'docs-1 is degraded' })
  expect(parseCheck(ROOT, 1, over, '')).toEqual({ kind: 'failed', root: ROOT, code: 'controls::control::degraded', message: 'docs-1 is degraded' })
})

test('session start checks the repository root once, and the first message carries its controls as one block', async ($, on) => {
  const { events } = engine(on, {})
  await start($)
  const context = await $.prompt.context({ blocks: [{ name: 'claudeMd', text: 'instructions' }] })
  expect(events).toEqual(['check'])
  expect(context.blocks.map(b => b.name)).toEqual(['claudeMd', 'boardControls'])
  expect(context.blocks[1]?.text).toContain('/controls/example/readme')
})

test('with no board the first message carries no block', async ($, on) => {
  const missing = JSON.stringify({ schema: 'board/failure/v1', code: 'board::config::not_found', message: 'no board' })
  engine(on, { check: { exitCode: 1, stdout: missing } })
  await start($)
  const context = await $.prompt.context({ blocks: [] })
  expect(context.blocks).toEqual([])
})

test('an edit of a file with a control the session was not told about appends a row naming it before the edit runs', async ($, on) => {
  const { events, appended } = engine(on, { forPath: { 'src/listing.rs': [PARSER] } })
  await start($)
  await edit($, 'src/listing.rs')
  expect(events).toEqual(['check', `for-path ${ROOT}/src/listing.rs`, 'append', 'edit'])
  expect(appended).toEqual([
    `This board control applies to ${ROOT}/src/listing.rs, which you are editing now. Follow each one in this ` +
      'edit and any that follows.\n\n- /controls/crumbkit/rust-10: A hand-written parser says what it misses.',
  ])
})

test('an edit of a file whose controls the session was told about appends nothing, and a file is asked about once', async ($, on) => {
  const { events } = engine(on, { forPath: { 'AGENTS.md': [DOCS], 'src/listing.rs': [PARSER] } })
  await start($)
  await edit($, 'AGENTS.md')
  await edit($, 'src/listing.rs')
  await edit($, 'src/listing.rs')
  expect(events).toEqual([
    'check',
    `for-path ${ROOT}/AGENTS.md`,
    'edit',
    `for-path ${ROOT}/src/listing.rs`,
    'append',
    'edit',
    'edit',
  ])
})

test('the Controls button counts the controls, and a failed check shows its code in red', () => {
  const checked = parseCheck(ROOT, 0, CHECK, '')
  expect(checked === null ? null : controlsBadge(checked)).toEqual({ label: 'Controls 2', problem: false })
  expect(controlsBadge({ kind: 'failed', root: ROOT, code: 'controls::control::degraded', message: 'm' })).toEqual({
    label: 'Controls: controls::control::degraded',
    problem: true,
  })
  expect(controlsBadge({ kind: 'read', root: ROOT, controls: [] })).toBeNull()
})

test('the pane groups the controls as standing rules, obligations owed and budgets', () => {
  const checked = parseCheck(ROOT, 0, report({ measured: [DOCS], versions: [RUST], stated: [README, OWED] }), '')
  const grouped = checked?.kind === 'read' ? sections(checked.controls) : []
  expect(grouped.map(g => [g.heading, g.controls.map(c => c.control)])).toEqual([
    ['Standing rules', ['/controls/toolchains/rust', '/controls/example/readme']],
    ['Obligations owed', ['/controls/crumbkit/docs-sweep']],
    ['Budgets', ['/controls/crumbkit/docs-1']],
  ])
})

test('the known boards file names the configured badges, and an entry without a label or a command is left out', () => {
  const text = JSON.stringify({
    personal: PERSONAL,
    badges: [{ label: 'Home', argv: ['home-name'] }, { label: '', argv: ['x'] }, { label: 'Empty', argv: [] }, { argv: ['y'] }, 'z'],
  })
  expect(parseConfig(text)).toEqual({ badges: [{ label: 'Home', argv: ['home-name'] }], personal: PERSONAL })
  expect(parseConfig('{"boards": []}')).toEqual({ badges: [], personal: null })
  expect(parseConfig('not json')).toEqual({ badges: [], personal: null })
})

test('a badge shows the first line of its output, and a command that fails or prints nothing shows no badge', () => {
  expect(badgeValue(0, 'vq\nmore\n')).toBe('vq')
  expect(badgeValue(0, '  vq  ')).toBe('vq')
  expect(badgeValue(0, '\nvq')).toBeNull()
  expect(badgeValue(0, '')).toBeNull()
  expect(badgeValue(1, 'vq')).toBeNull()
})

test('the Board badge names the personal board as personal, and any other board by its directory', () => {
  const listing = (repository: string) => JSON.stringify({ schema: 'board/ls-report/v2', summary: { location: { repository } } })
  expect(boardName(0, listing(PERSONAL), PERSONAL)).toBe('personal')
  expect(boardName(0, listing(`${PERSONAL}/`), PERSONAL)).toBe('personal')
  expect(boardName(0, listing(`${PERSONAL}/vq`), PERSONAL)).toBe('vq')
  expect(boardName(0, listing(`${PERSONAL}/vq`), null)).toBe('vq')
  expect(boardName(1, JSON.stringify({ code: 'board::config::not_found', message: 'no board' }), PERSONAL)).toBeNull()
})

test('the band shows the configured badges, the Board badge and the Controls button, and the button opens the grouped pane', async ($, on) => {
  const config = JSON.stringify({
    personal: PERSONAL,
    badges: [
      { label: 'Home', argv: ['home-name'] },
      { label: 'Broken', argv: ['broken'] },
      { label: 'Slow', argv: ['slow'] },
      { label: 'Quiet', argv: ['quiet'] },
    ],
  })
  const { events } = engine(on, {
    config,
    commands: { 'home-name': 'vq\n', broken: 'fails', slow: 'hangs', quiet: '' },
    repository: `${PERSONAL}/vq`,
  })
  await start($)
  expect(events.filter(e => e.startsWith('badge')).sort()).toEqual([
    `badge broken in ${ROOT}/src`,
    `badge home-name in ${ROOT}/src`,
    `badge quiet in ${ROOT}/src`,
    `badge slow in ${ROOT}/src`,
  ])
  for (const surface of ['terminal', 'desktop'] as const) {
    const band = await $.ui.mount({ ...BAND, surface })
    const texts = (await band.findAll({ type: 'Text' })).map(t => t.text)
    expect(texts).toEqual(['Home', 'vq', 'Board', 'vq', '|'])
    expect((await band.find({ key: 'controls' }))?.text).toBe('Controls 2')
    await band.press({ key: 'controls' })
    await band.unmount()
    const pane = await $.ui.mount({ plugin: 'board-controls', surface, component: 'Pane', requestId: 'board-controls', props: PANE_PROPS })
    expect(await pane.find({ text: 'Controls on crumbkit' })).toBeDefined()
    expect(await pane.find({ text: 'Standing rules' })).toBeDefined()
    expect(await pane.find({ text: '/controls/example/readme: Keep the usage section of the README current.' })).toBeDefined()
    expect(await pane.find({ text: 'Obligations owed' })).toBeDefined()
    expect(await pane.find({ text: 'None.' })).toBeDefined()
    expect(await pane.find({ text: '/controls/crumbkit/docs-1: AGENTS.md has at most 200 words. (now 196 of 200 words)' })).toBeDefined()
    expect(await pane.find({ text: 'Editing a file adds a reminder for each control on that file that this session was not told about.' })).toBeDefined()
    await pane.unmount()
  }
})

test('the parts sit in their own slots in the band row', async ($, on) => {
  engine(on, { repository: PERSONAL, config: JSON.stringify({ personal: PERSONAL }) })
  await start($)
  const band = await $.ui.mount({ ...BAND, surface: 'terminal' })
  const drawn = await band.drawn()
  const row = drawn.type === 'Box' ? drawn.children?.[0] : undefined
  const keys = typeof row === 'object' && row.type === 'Box' ? row.children?.map(c => (typeof c === 'object' && c.type === 'Box' ? c.props?.key : 'bar')) : []
  expect(keys).toEqual([SLOT.state, 'bar', SLOT.controls])
  expect((await band.findAll({ type: 'Text' })).map(t => t.text)).toEqual(['Board', 'personal', '|'])
})

test('a failed check shows the Controls button in red with its code', async ($, on) => {
  const failed = JSON.stringify({ schema: 'board/failure/v1', code: 'controls::control::degraded', message: 'docs-1 is degraded' })
  engine(on, { check: { exitCode: 1, stdout: failed } })
  await start($)
  const band = await $.ui.mount({ ...BAND, surface: 'terminal' })
  expect((await band.find({ key: 'controls' }))?.text).toBe('Controls: controls::control::degraded')
})

test('with no board and no configured badges the band draws nothing of the mod\'s', async ($, on) => {
  const missing = JSON.stringify({ schema: 'board/failure/v1', code: 'board::config::not_found', message: 'no board' })
  engine(on, { check: { exitCode: 1, stdout: missing } })
  await start($)
  const band = await $.ui.mount({ ...BAND, surface: 'terminal' })
  expect(await band.find({ key: 'controls' })).toBeUndefined()
  expect(await band.findAll({ type: 'Text' })).toEqual([])
})

test('a compaction forgets the rows, so the next edit names its control again', async ($, on) => {
  const { appended } = engine(on, { forPath: { 'src/listing.rs': [PARSER] } })
  await start($)
  await $.prompt.context({ blocks: [] })
  await edit($, 'src/listing.rs')
  await $.prompt.context({ blocks: [] })
  await edit($, 'src/listing.rs')
  expect(appended.length).toBe(2)
})

test('a subagent is told on its own, and a file outside the repository runs nothing', async ($, on) => {
  const { events } = engine(on, { forPath: { 'src/listing.rs': [PARSER] } })
  await start($)
  await edit($, 'src/listing.rs')
  await $.tool.call({ tool: 'Edit', file_path: `${ROOT}/src/listing.rs`, old_string: 'a', new_string: 'b', replace_all: false, agentId: 'a1' })
  await $.tool.call({ tool: 'Write', file_path: '/tmp/notes.md', content: 'x' })
  expect(events).toEqual(['check', `for-path ${ROOT}/src/listing.rs`, 'append', 'edit', 'append', 'edit', 'edit'])
})

test('a for-path run that fails lets the edit run and appends nothing', async ($, on) => {
  const { events } = engine(on, { forPath: 'fails' })
  await start($)
  await edit($, 'src/listing.rs')
  expect(events).toEqual(['check', 'edit'])
})
