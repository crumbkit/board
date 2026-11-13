import { expect, test } from 'claude-code/testing'
import type { On } from 'claude-code'
import type { Engine } from 'claude-code/testing'

import { SLOT } from '../hooks/band'
import { badgeValue, boardName, contextText, parseBadges, parseCheck, parseForPath, parsePersonal, rulesBadge, sections } from '../hooks/rules'

// These prove what a session is told about rules and when, with git and
// board stood in by a process.run hook. They do not run board: the Python
// tests prove what rules.check and rules.for-path answer.

const ROOT = '/work/crumbkit'
const DOCS = { rule: '/rules/crumbkit/docs-1', requirement: 'AGENTS.md has at most 200 words.', budget: 'words', used: 196, max: 200, files: [] }
const README = { rule: '/rules/example/readme', kind: 'standing', requirement: 'Keep the usage section of the README current.', files: 3, paths: [], purpose: 'p' }
const SWEEP = { rule: '/rules/crumbkit/docs-sweep', kind: 'obligation', requirement: 'Sweep the docs for #12.', files: 1, paths: [], purpose: 'p' }
const DESCRIBED = { rule: '/rules/example/tone', requirement: 'Write plainly.', purpose: 'p', kind: 'standing', files: 0, paths: [], state: 'described' }
const PARSER = { rule: '/rules/crumbkit/rust-10', requirement: 'A hand-written parser says what it misses.', purpose: 'p', checker: null }
const report = (output: object) => JSON.stringify({ schema: 'board/call-report/v2', data: { plugin: 'rules', output } })
const RUST = { rule: '/rules/toolchains/rust', requirement: 'Pin the latest Rust.', at_least: '1.99.1', files: [], behind: [] }
const CHECK = report({ measured: [DOCS], stated: [README] })
const PANE_PROPS = { title: 'Rules', isFocused: false, bodyColumns: 80, placement: 'dock', scroll: { offset: 0, bodyRows: 40 }, view: {} } as const
const BAND = {
  plugin: 'board-rules',
  component: 'AbovePrompt',
  props: { hasSurvey: false, isWorking: false, maxRows: 10, bodyColumns: 80, scroll: { offset: 0, bodyRows: 10 }, view: {} },
} as const

type Answers = {
  check?: { exitCode: number; stdout: string }
  forPath?: Record<string, object[]> | 'fails'
  /** The known boards file's text; none when absent. */
  config?: string
  /** The badges file's text; none when absent, 'unreadable' when it exists and cannot be read. */
  badges?: string
  /** What each configured command prints, by its first argument; 'fails' exits 1, 'hangs' rejects. */
  commands?: Record<string, string>
  /** The repository `board ls /` names; none when no board holds the directory. */
  repository?: string
}

const CONFIG = '/home/me/.config/board/config.json'
const BADGES = '/home/me/.config/board/badges.json'
const PERSONAL = '/home/me/Code/board'

/** Stands in for the engine beneath the plugin: git, board, the tool list, the status line and the transcript. */
function engine(on: On, answers: Answers) {
  const events: string[] = []
  const appended: string[] = []
  const logs: string[] = []
  const run = (exitCode: number, stdout: string) => ({
    value: { exitCode, stdout, stderr: '', isStdoutTruncated: false, isStderrTruncated: false },
  })
  on('env.get', (_, e) => ({ value: e.name === 'HOME' ? '/home/me' : undefined }))
  on('fs.exists', (_, e) => ({ value: e.path === BADGES ? answers.badges !== undefined : e.path === CONFIG && answers.config !== undefined }))
  on('fs.read', (_, e) => {
    if (e.path === CONFIG && answers.config !== undefined) return { value: answers.config }
    if (e.path === BADGES && answers.badges !== undefined && answers.badges !== 'unreadable') return { value: answers.badges }
    throw new Error(`EACCES: ${e.path}`)
  })
  on('process.run', (_, e) => {
    const line = e.argv.join(' ')
    if (line.includes('rev-parse')) return run(0, `${ROOT}\n`)
    if (line.includes('ls / --limit 1')) {
      if (answers.repository === undefined) return run(1, JSON.stringify({ code: 'board::config::not_found', message: 'no board' }))
      return run(0, JSON.stringify({ schema: 'board/ls-report/v2', data: [], summary: { location: { repository: answers.repository } } }))
    }
    const name = e.argv[3] === 'badge' ? (e.argv[4] ?? '') : ''
    const command = answers.commands?.[name]
    if (command !== undefined) {
      events.push(`badge ${name} in ${e.init?.cwd}`)
      if (!String(e.argv[2]).includes('$HOME/.local/bin')) throw new Error('the badge command ran without the board PATH')
      if (command === 'hangs') throw new Error('timed out')
      return command === 'fails' ? run(1, 'x\n') : run(0, command)
    }
    if (line.includes('rules.check')) {
      events.push('check')
      const check = answers.check ?? { exitCode: 0, stdout: CHECK }
      return run(check.exitCode, check.stdout)
    }
    const file = (e.argv.at(-1) ?? '').replace(/^\.\//, '')
    if (answers.forPath === 'fails') return run(2, '')
    events.push(`for-path ${e.init?.cwd}/${file}`)
    return run(0, report({ rules: answers.forPath?.[file] ?? [] }))
  })
  on('ui.open', () => ({ value: { isPlaced: true } }))
  on('ui.log', (_, e) => {
    logs.push(e.text)
    return { value: undefined }
  })
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
  return { events, appended, logs }
}

const start = ($: Engine) => $.session.start({ cwd: `${ROOT}/src`, surface: 'terminal', isInteractive: true })
const edit = ($: Engine, file: string) =>
  $.tool.call({ tool: 'Edit', file_path: `${ROOT}/${file}`, old_string: 'a', new_string: 'b', replace_all: false })

test('the context block lists each rule with its requirement, a budget with its usage and a version rule with its floor', () => {
  const checked = parseCheck(ROOT, 0, report({ measured: [DOCS], versions: [RUST], stated: [README] }), '')
  expect(checked === null ? null : contextText(checked)).toBe(
    `The board's rules state requirements on files in ${ROOT}. Follow each one that applies to a file you ` +
      'change. `board get <rule>` shows which files a rule names.\n\n' +
      '- /rules/crumbkit/docs-1: AGENTS.md has at most 200 words. (now 196 of 200 words)\n' +
      '- /rules/toolchains/rust: Pin the latest Rust. (now every pin at least 1.99.1)\n' +
      '- /rules/example/readme: Keep the usage section of the README current.',
  )
})

const OWED = {
  rule: '/rules/crumbkit/docs-owed-9',
  requirement: 'The merge of #9 owes a docs sweep.',
  files: 1,
  paths: [],
  purpose: 'p',
  kind: 'obligation',
  close:
    'Once the work this obligation names is committed, delete it with ' +
    "`board rm /rules/crumbkit/docs-owed-9 -m '<message naming the work>'`; merging the work does not clear it.",
}

test('the context block gives an obligation its close line and a standing rule none', () => {
  const checked = parseCheck(ROOT, 0, report({ measured: [], stated: [OWED, README] }), '')
  expect(checked === null ? null : contextText(checked)).toContain(
    '- /rules/crumbkit/docs-owed-9: The merge of #9 owes a docs sweep.\n' +
      `  ${OWED.close}\n` +
      '- /rules/example/readme: Keep the usage section of the README current.',
  )
  expect(checked === null ? null : contextText(checked)).toMatch(/current\.$/)
})

test('a version rule with pins behind and a budget over its max with an unreadable file say so in the block', () => {
  const behind = { ...RUST, behind: [{ file: '/r/rust-toolchain.toml', line: 2, found: '1.98.0' }] }
  const unread = { ...DOCS, used: 300, unreadable: [{ file: '/r/AGENTS.md', code: 'board::path::shadowed' }] }
  const checked = parseCheck(ROOT, 0, report({ passed: false, measured: [unread], versions: [behind], stated: [] }), '')
  expect(checked === null ? null : contextText(checked)).toContain(
    '- /rules/crumbkit/docs-1: AGENTS.md has at most 200 words. (now 300 of 200 words, over budget, 1 unreadable)\n' +
      '- /rules/toolchains/rust: Pin the latest Rust. (now 1 older than 1.99.1)',
  )
})

test('the context block shows a rule with no checker as described, and a rule with one as plain', () => {
  const checked = parseCheck(ROOT, 0, report({ measured: [], stated: [DESCRIBED, README] }), '')
  expect(checked === null ? null : contextText(checked)).toContain(
    '- /rules/example/tone: Write plainly. (described)\n' +
      '- /rules/example/readme: Keep the usage section of the README current.',
  )
})

test('output in the former names is refused: a check without rule fails, and a for-path without rules reads as nothing', () => {
  const former = { control: '/controls/example/readme', kind: 'standing', requirement: 'Keep it.', files: 3, paths: [], purpose: 'p' }
  expect(parseCheck(ROOT, 0, report({ measured: [], stated: [former] }), '')?.kind).toBe('failed')
  expect(parseForPath(0, report({ controls: [former] }))).toBeNull()
  expect(parseForPath(0, report({ rules: [former] }))).toBeNull()
})

test('a repository no board holds has no rules, and a check that fails is said plainly', () => {
  const missing = JSON.stringify({ schema: 'board/failure/v1', code: 'board::config::not_found', message: 'no board' })
  expect(parseCheck(ROOT, 1, missing, '')).toBeNull()
  const over = JSON.stringify({ schema: 'board/failure/v1', code: 'rules::rule::degraded', message: 'docs-1 is degraded' })
  expect(parseCheck(ROOT, 1, over, '')).toEqual({ kind: 'failed', root: ROOT, code: 'rules::rule::degraded', message: 'docs-1 is degraded' })
})

test('session start checks the repository root once, and the first message carries its rules as one block', async ($, on) => {
  const { events } = engine(on, {})
  await start($)
  const context = await $.prompt.context({ blocks: [{ name: 'claudeMd', text: 'instructions' }] })
  expect(events).toEqual(['check'])
  expect(context.blocks.map(b => b.name)).toEqual(['claudeMd', 'boardRules'])
  expect(context.blocks[1]?.text).toContain('/rules/example/readme')
})

test('with no board the first message carries no block', async ($, on) => {
  const missing = JSON.stringify({ schema: 'board/failure/v1', code: 'board::config::not_found', message: 'no board' })
  engine(on, { check: { exitCode: 1, stdout: missing } })
  await start($)
  const context = await $.prompt.context({ blocks: [] })
  expect(context.blocks).toEqual([])
})

test('an edit of a file with a rule the session was not told about appends a row naming it before the edit runs', async ($, on) => {
  const { events, appended } = engine(on, { forPath: { 'src/listing.rs': [PARSER] } })
  await start($)
  await edit($, 'src/listing.rs')
  expect(events).toEqual(['check', `for-path ${ROOT}/src/listing.rs`, 'append', 'edit'])
  expect(appended).toEqual([
    `This board rule applies to ${ROOT}/src/listing.rs, which you are editing now. Follow each one in this ` +
      'edit and any that follows.\n\n- /rules/crumbkit/rust-10: A hand-written parser says what it misses.',
  ])
})

test('the row before an edit gives an obligation its close line and a standing rule none', async ($, on) => {
  const { appended } = engine(on, { forPath: { 'docs/guide.md': [OWED, PARSER] } })
  await start($)
  await edit($, 'docs/guide.md')
  expect(appended).toEqual([
    `These board rules apply to ${ROOT}/docs/guide.md, which you are editing now. Follow each one in this ` +
      'edit and any that follows.\n\n' +
      '- /rules/crumbkit/docs-owed-9: The merge of #9 owes a docs sweep.\n' +
      `  ${OWED.close}\n` +
      '- /rules/crumbkit/rust-10: A hand-written parser says what it misses.',
  ])
})

test('an edit of a file whose rules the session was told about appends nothing, and a file is asked about once', async ($, on) => {
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

test('the Rules button counts the rules, and a failed check shows its code in red', () => {
  const checked = parseCheck(ROOT, 0, CHECK, '')
  expect(checked === null ? null : rulesBadge(checked)).toEqual({ label: 'Rules 2', problem: false })
  expect(rulesBadge({ kind: 'failed', root: ROOT, code: 'rules::rule::degraded', message: 'm' })).toEqual({
    label: 'Rules: rules::rule::degraded',
    problem: true,
  })
  expect(rulesBadge({ kind: 'read', root: ROOT, rules: [] })).toBeNull()
})

test('the pane groups the rules as standing rules, obligations owed and budgets', () => {
  const checked = parseCheck(ROOT, 0, report({ measured: [DOCS], versions: [RUST], stated: [README, SWEEP] }), '')
  const grouped = checked?.kind === 'read' ? sections(checked.rules) : []
  expect(grouped.map(g => [g.heading, g.rules.map(c => c.rule)])).toEqual([
    ['Standing rules', ['/rules/toolchains/rust', '/rules/example/readme']],
    ['Obligations owed', ['/rules/crumbkit/docs-sweep']],
    ['Budgets', ['/rules/crumbkit/docs-1']],
  ])
})

test('the badges file lists each badge\'s label and command, and a malformed file says what is wrong', () => {
  expect(parseBadges(JSON.stringify({ badges: [{ label: ' Home ', argv: ['home-name', '--short'] }] }))).toEqual({
    badges: [{ label: 'Home', argv: ['home-name', '--short'] }],
  })
  expect(parseBadges(JSON.stringify({ badges: [] }))).toEqual({ badges: [] })
  expect(parseBadges('not json')).toEqual({ problem: 'is not a JSON object' })
  expect(parseBadges('{}')).toEqual({ problem: 'has no badges list' })
  expect(parseBadges(JSON.stringify({ badges: [{ label: 'Home', argv: ['x'] }, { label: '', argv: ['y'] }] }))).toEqual({
    problem: 'entry 2 needs a label and an argv of strings',
  })
  expect(parseBadges(JSON.stringify({ badges: [{ label: 'Home', argv: [] }] }))).toEqual({ problem: 'entry 1 needs a label and an argv of strings' })
  expect(parsePersonal(JSON.stringify({ personal: PERSONAL, boards: [] }))).toBe(PERSONAL)
  expect(parsePersonal('{}')).toBeNull()
})

test('a badge shows the first line of its output, and a command that fails or prints nothing shows no badge', () => {
  expect(badgeValue(0, 'work\nmore\n')).toEqual({ value: 'work' })
  expect(badgeValue(0, '  work  ')).toEqual({ value: 'work' })
  expect(badgeValue(0, '\nwork')).toEqual({ why: 'printed nothing on its first line' })
  expect(badgeValue(0, '')).toEqual({ why: 'printed nothing on its first line' })
  expect(badgeValue(1, 'work')).toEqual({ why: 'exited with status 1' })
})

test('the Board badge names the personal board as personal, and any other board by its directory', () => {
  const listing = (repository: string) => JSON.stringify({ schema: 'board/ls-report/v2', summary: { location: { repository } } })
  expect(boardName(0, listing(PERSONAL), PERSONAL)).toBe('personal')
  expect(boardName(0, listing(`${PERSONAL}/`), PERSONAL)).toBe('personal')
  expect(boardName(0, listing(`${PERSONAL}/team`), PERSONAL)).toBe('team')
  expect(boardName(0, listing(`${PERSONAL}/team`), null)).toBe('team')
  expect(boardName(1, JSON.stringify({ code: 'board::config::not_found', message: 'no board' }), PERSONAL)).toBeNull()
})

test('the band shows the configured badges, the Board badge and the Rules button, and the button opens the grouped pane', async ($, on) => {
  const config = JSON.stringify({ personal: PERSONAL })
  const badges = JSON.stringify({
    badges: [
      { label: 'Home', argv: ['home-name'] },
      { label: 'Broken', argv: ['broken'] },
      { label: 'Slow', argv: ['slow'] },
      { label: 'Quiet', argv: ['quiet'] },
    ],
  })
  const { events, logs } = engine(on, {
    config,
    badges,
    commands: { 'home-name': 'work\n', broken: 'fails', slow: 'hangs', quiet: '' },
    repository: `${PERSONAL}/team`,
  })
  await start($)
  expect(events.filter(e => e.startsWith('badge')).sort()).toEqual([
    `badge broken in ${ROOT}/src`,
    `badge home-name in ${ROOT}/src`,
    `badge quiet in ${ROOT}/src`,
    `badge slow in ${ROOT}/src`,
  ])
  // The test engine words the rejected run its own way, so the slow badge's reason is matched up to it.
  expect(logs.filter(l => l.includes('badge')).sort()).toEqual([
    'board: the Broken badge is hidden, because its command exited with status 1',
    'board: the Quiet badge is hidden, because its command printed nothing on its first line',
    expect.stringMatching(/^board: the Slow badge is hidden, because its command did not finish within 2000 ms: /),
  ])
  for (const surface of ['terminal', 'desktop'] as const) {
    const band = await $.ui.mount({ ...BAND, surface })
    const texts = (await band.findAll({ type: 'Text' })).map(t => t.text)
    expect(texts).toEqual(['Home', ' work ', 'Board', ' team ', '|'])
    expect((await band.find({ key: 'rules' }))?.text).toBe('Rules 2')
    await band.press({ key: 'rules' })
    await band.unmount()
    const pane = await $.ui.mount({ plugin: 'board-rules', surface, component: 'Pane', requestId: 'board-rules', props: PANE_PROPS })
    expect(await pane.find({ text: 'Rules on crumbkit' })).toBeDefined()
    expect(await pane.find({ text: 'Standing rules' })).toBeDefined()
    expect(await pane.find({ text: '/rules/example/readme: Keep the usage section of the README current.' })).toBeDefined()
    expect(await pane.find({ text: 'Obligations owed' })).toBeDefined()
    expect(await pane.find({ text: 'None.' })).toBeDefined()
    expect(await pane.find({ text: '/rules/crumbkit/docs-1: AGENTS.md has at most 200 words. (now 196 of 200 words)' })).toBeDefined()
    expect(await pane.find({ text: 'Editing a file adds a reminder for each rule on that file that this session was not told about.' })).toBeDefined()
    await pane.unmount()
  }
})

test('the pane lists an obligation under Obligations owed with its close line beneath it', async ($, on) => {
  engine(on, { repository: PERSONAL, check: { exitCode: 0, stdout: report({ measured: [], stated: [OWED] }) } })
  await start($)
  for (const surface of ['terminal', 'desktop'] as const) {
    const pane = await $.ui.mount({ plugin: 'board-rules', surface, component: 'Pane', requestId: 'board-rules', props: PANE_PROPS })
    expect(await pane.find({ text: '/rules/crumbkit/docs-owed-9: The merge of #9 owes a docs sweep.' })).toBeDefined()
    expect(await pane.find({ text: `  ${OWED.close}` })).toBeDefined()
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
  expect((await band.findAll({ type: 'Text' })).map(t => t.text)).toEqual(['Board', ' personal ', '|'])
})

test('a badges file that is malformed or cannot be read is one red badge naming the file and the problem', async ($, on) => {
  const answers: Answers = { badges: '{"badges": "Home"}', repository: PERSONAL, config: JSON.stringify({ personal: PERSONAL }) }
  engine(on, answers)
  await start($)
  const band = await $.ui.mount({ ...BAND, surface: 'terminal' })
  expect((await band.findAll({ type: 'Text' })).map(t => t.text)).toEqual([`${BADGES} has no badges list`, 'Board', ' personal ', '|'])
  await band.unmount()
  answers.badges = 'unreadable'
  await start($)
  const again = await $.ui.mount({ ...BAND, surface: 'terminal' })
  // The test engine words the read's failure its own way; a session shows the file system's error.
  expect((await again.find({ type: 'Text' }))?.text?.startsWith(`${BADGES} could not be read: `)).toBe(true)
})

test('a failed check shows the Rules button in red with its code', async ($, on) => {
  const failed = JSON.stringify({ schema: 'board/failure/v1', code: 'rules::rule::degraded', message: 'docs-1 is degraded' })
  engine(on, { check: { exitCode: 1, stdout: failed } })
  await start($)
  const band = await $.ui.mount({ ...BAND, surface: 'terminal' })
  expect((await band.find({ key: 'rules' }))?.text).toBe('Rules: rules::rule::degraded')
})

test('with no board and no configured badges the band draws nothing of the mod\'s', async ($, on) => {
  const missing = JSON.stringify({ schema: 'board/failure/v1', code: 'board::config::not_found', message: 'no board' })
  engine(on, { check: { exitCode: 1, stdout: missing } })
  await start($)
  const band = await $.ui.mount({ ...BAND, surface: 'terminal' })
  expect(await band.find({ key: 'rules' })).toBeUndefined()
  expect(await band.findAll({ type: 'Text' })).toEqual([])
})

test('a compaction forgets the rows, so the next edit names its rule again', async ($, on) => {
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
