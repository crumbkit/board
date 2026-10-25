import { expect, test } from 'claude-code/testing'
import type { On } from 'claude-code'
import type { Engine } from 'claude-code/testing'

import { contextText, parseCheck } from '../hooks/controls'

// These prove what a session is told about controls and when, with git and
// board stood in by a process.run hook. They do not run board: the Python
// tests prove what controls.check and controls.for-path answer.

const ROOT = '/work/crumbkit'
const DOCS = { control: '/controls/crumbkit/docs-1', requirement: 'AGENTS.md has at most 200 words.', budget: 'words', used: 196, max: 200, files: [] }
const SCAN = { control: '/controls/crumbkit/scan', requirement: 'Run the confidentiality scan before a push.', files: 3, paths: [], purpose: 'p' }
const PARSER = { control: '/controls/crumbkit/rust-10', requirement: 'A hand-written parser says what it misses.', purpose: 'p', rule: null }
const report = (output: object) => JSON.stringify({ schema: 'board/call-report/v2', data: { plugin: 'controls', output } })
const CHECK = report({ measured: [DOCS], stated: [SCAN] })
const BAND = {
  plugin: 'board-controls',
  component: 'AbovePrompt',
  props: { hasSurvey: false, isWorking: false, maxRows: 10, bodyColumns: 80, scroll: { offset: 0, bodyRows: 10 }, view: {} },
} as const

type Answers = { check?: { exitCode: number; stdout: string }; forPath?: Record<string, object[]> | 'fails'; tools?: string[] }

/** Stands in for the engine beneath the plugin: git, board, the tool list, the status line and the transcript. */
function engine(on: On, answers: Answers) {
  const events: string[] = []
  const appended: string[] = []
  const statuses: (string | undefined)[] = []
  const run = (exitCode: number, stdout: string) => ({
    value: { exitCode, stdout, stderr: '', isStdoutTruncated: false, isStderrTruncated: false },
  })
  on('process.run', (_, e) => {
    const line = e.argv.join(' ')
    if (line.includes('rev-parse')) return run(0, `${ROOT}\n`)
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
  on('tool.list', () => ({ value: (answers.tools ?? []).map(name => ({ name, description: '', mcp: name.startsWith('mcp__') })) }))
  on('ui.status', (_, e) => {
    statuses.push(e.text as string | undefined)
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
  return { events, appended, statuses }
}

const start = ($: Engine) => $.session.start({ cwd: `${ROOT}/src`, surface: 'terminal', isInteractive: true })
const edit = ($: Engine, file: string) =>
  $.tool.call({ tool: 'Edit', file_path: `${ROOT}/${file}`, old_string: 'a', new_string: 'b', replace_all: false })

test('the context block lists each control with its requirement, and a budget with its usage', () => {
  const checked = parseCheck(ROOT, 0, CHECK, '')
  expect(checked === null ? null : contextText(checked)).toBe(
    `The board's controls state requirements on files in ${ROOT}. Follow each one that applies to a file you ` +
      'change. `board get <control>` shows which files a control names.\n\n' +
      '- /controls/crumbkit/docs-1: AGENTS.md has at most 200 words. (now 196 of 200 words)\n' +
      '- /controls/crumbkit/scan: Run the confidentiality scan before a push.',
  )
})

test('a repository no board holds has no controls, and a check that fails is said plainly', () => {
  const missing = JSON.stringify({ schema: 'board/failure/v1', code: 'board::config::not_found', message: 'no board' })
  expect(parseCheck(ROOT, 1, missing, '')).toBeNull()
  const over = JSON.stringify({ schema: 'board/failure/v1', code: 'controls::budget::exceeded', message: 'docs-1 is over' })
  expect(parseCheck(ROOT, 1, over, '')).toEqual({ kind: 'failed', root: ROOT, code: 'controls::budget::exceeded', message: 'docs-1 is over' })
})

test('session start checks the repository root once, and the first message carries its controls as one block', async ($, on) => {
  const { events } = engine(on, {})
  await start($)
  const context = await $.prompt.context({ blocks: [{ name: 'claudeMd', text: 'instructions' }] })
  expect(events).toEqual(['check'])
  expect(context.blocks.map(b => b.name)).toEqual(['claudeMd', 'boardControls'])
  expect(context.blocks[1]?.text).toContain('/controls/crumbkit/scan')
})

test('with no board the first message carries no block and the status line stays empty', async ($, on) => {
  const missing = JSON.stringify({ schema: 'board/failure/v1', code: 'board::config::not_found', message: 'no board' })
  const { statuses } = engine(on, { check: { exitCode: 1, stdout: missing } })
  await start($)
  const context = await $.prompt.context({ blocks: [] })
  expect(context.blocks).toEqual([])
  expect(statuses.filter(s => s !== undefined)).toEqual([])
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

test('with the attention mod present the controls join the band, and without it they go to the status line', async ($, on) => {
  const { statuses } = engine(on, { tools: ['mcp__attention-approve__request_signature'] })
  await start($)
  for (const surface of ['terminal', 'desktop'] as const) {
    const band = await $.ui.mount({ ...BAND, surface })
    expect(await band.find({ text: /2 controls on crumbkit/ })).toBeDefined()
    await band.unmount()
  }
  expect(statuses.filter(s => s !== undefined)).toEqual([])
})

test('without the attention mod the status line names the controls and the band draws nothing of the mod\'s', async ($, on) => {
  const { statuses } = engine(on, { tools: ['Bash'] })
  await start($)
  expect(statuses).toEqual(['board: 2 controls on crumbkit'])
  const band = await $.ui.mount({ ...BAND, surface: 'terminal' })
  expect(await band.find({ text: /controls on crumbkit/ })).toBeUndefined()
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
