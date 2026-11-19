import { expect, test } from 'claude-code/testing'
import type { On } from 'claude-code'
import type { Engine } from 'claude-code/testing'

import { changeIn, noLine, notRendered, objectsIn } from '../hooks/brief'
import PRINTED from './brief.golden'

// These prove what the Agent hook advises a lead whose builder prompt names a
// change in build, and that the advice reaches both the builder's prompt and
// the lead's result. `board` is stood in by a process.run hook and the board
// config by fs hooks. They do not run board: brief.golden.ts holds what
// `changes.brief` prints, which a Rust test in board-cli pins.

const HOME = '/home/me'
const BOARD = '/home/me/board'
const CONFIG = `${HOME}/.config/board/config.json`
const PATH = '/changes/t'
const LINE = `board-brief: ${PATH}`
const RENDERED = JSON.parse(PRINTED) as Record<string, unknown>
const report = (data: unknown) => JSON.stringify({ schema: 'board/get-report/v2', data })
const change = (phase: string) => report([{ record: { $type: 'changes/change/v2', title: 't', mount: '/repos/x', phase } }])
const MISSING = JSON.stringify({
  schema: 'board/get-report/v2',
  data: [],
  failures: [{ key: PATH, code: 'board::path::not_found', message: `nothing is at ${PATH}` }],
})
const called = (output: unknown) =>
  JSON.stringify({ schema: 'board/call-report/v2', data: { capability: 'brief', output } })

/** The brief as a lead should send it: the brief line first, then the rendered brief. */
const GOOD = `${LINE}\n\n${PRINTED}\nBuild it.`

type Answers = {
  /** What `board get` prints for the change; in build when left out. */
  change?: string
  /** What `board call changes.brief` prints, or 'fails'; the golden when left out. */
  rendered?: string | 'fails'
}

/** Stands in for the board config, `board` and the Agent tool; `ran` lists the board commands, `prompts` what each builder got. */
function engine(on: On, answers: Answers) {
  const ran: string[] = []
  const prompts: string[] = []
  const run = (exitCode: number, stdout: string) => ({
    value: { exitCode, stdout, stderr: '', isStdoutTruncated: false, isStderrTruncated: false },
  })
  on('env.get', (_, e) => ({ value: e.name === 'HOME' ? HOME : undefined }))
  on('fs.exists', (_, e) => ({ value: e.path === CONFIG }))
  on('fs.read', (_, e) => {
    if (e.path !== CONFIG) throw new Error(`ENOENT: ${e.path}`)
    return { value: JSON.stringify({ boards: [BOARD] }) }
  })
  on('process.run', (_, e) => {
    const script = e.argv[2] ?? ''
    const args = e.argv.slice(4)
    if (script.includes('changes.brief')) {
      ran.push(`brief ${args.join(' ')}`)
      const rendered = answers.rendered ?? called(RENDERED)
      return rendered === 'fails' ? run(1, '') : run(0, rendered)
    }
    ran.push(`get ${args.join(' ')}`)
    const printed = answers.change ?? change('build')
    return run(printed === MISSING ? 1 : 0, printed)
  })
  on('tool.call', (_, e) => {
    if (e.tool === 'Agent') prompts.push(e.prompt)
    return { result: {} } as never
  })
  return { ran, prompts }
}

const agent = ($: Engine, prompt: string) => $.tool.call({ tool: 'Agent', description: 'Build it', prompt })

const NO_LINE = (first: string) =>
  `changes::brief::no_line: this prompt names ${PATH}, and its first line is ${first}. The edit guard reads ` +
  `the builder's first line, so put \`${LINE}\` first.`
const NOT_RENDERED = (what: string) =>
  `changes::brief::not_rendered: ${what}, so it is not what changes.brief renders for ${PATH} now. Render it ` +
  `with \`board -C ${BOARD} call changes.brief --path ${PATH}\` and send its whole output after the brief line.`

test('a prompt names the first change it mentions', () => {
  expect(changeIn(GOOD)).toBe(PATH)
  expect(changeIn('Build /changes/fix-it.')).toBe('/changes/fix-it')
  expect(changeIn('Build /changes/fix-it/plans/1.md, then /changes/other')).toBe('/changes/fix-it')
  expect(changeIn('Build the widget.')).toBeNull()
})

test('a brief line that is not first is advised, and the same brief with it first is not', () => {
  expect(noLine(`${PRINTED}\n${LINE}`, PATH)).toBe(NO_LINE('`{`'))
  expect(noLine(`Please build it.\n${LINE}`, PATH)).toBe(NO_LINE('`Please build it.`'))
  expect(noLine(`board-brief: /changes/other\n${PRINTED}`, PATH)).toBe(NO_LINE('`board-brief: /changes/other`'))
  expect(noLine(`\n  ${LINE}  \n${PRINTED}`, PATH)).toBeNull()
})

test('every JSON object that opens on a line of its own is read, and a broken one is skipped', () => {
  expect(objectsIn(GOOD)).toEqual([RENDERED])
  expect(objectsIn(`${PRINTED}\n${PRINTED.replace('"path":', '"path"')}\n${PRINTED}`)).toEqual([RENDERED, RENDERED])
  expect(objectsIn('No JSON here.')).toEqual([])
})

test('a brief with one key changed is advised with that key, and the verbatim brief is not', () => {
  const changed = JSON.stringify({ ...RENDERED, phase: 'build' }, null, 2)
  expect(notRendered(`${LINE}\n${changed}`, RENDERED, PATH, BOARD)).toBe(NOT_RENDERED('its brief differs at `phase`'))
  const dropped = { ...RENDERED }
  delete dropped.warnings
  expect(notRendered(`${LINE}\n${JSON.stringify(dropped, null, 2)}`, RENDERED, PATH, BOARD)).toBe(
    NOT_RENDERED('its brief differs at `warnings`'),
  )
  expect(notRendered(GOOD, RENDERED, PATH, BOARD)).toBeNull()
})

test('a brief in another key order is the rendered brief, and a prompt with no JSON brief is advised', () => {
  const reordered = JSON.stringify(Object.fromEntries(Object.entries(RENDERED).reverse()), null, 2)
  expect(notRendered(`${LINE}\n${reordered}`, RENDERED, PATH, BOARD)).toBeNull()
  expect(notRendered(`${LINE}\nBuild it.`, RENDERED, PATH, BOARD)).toBe(NOT_RENDERED('the prompt holds no brief as JSON'))
})

test('a JSON-first prompt is advised no_line, and the same brief with the line first runs untouched', async ($, on) => {
  const { ran, prompts } = engine(on, {})
  const advised = await agent($, `${PRINTED}\nBuild it.`)
  expect(advised.context).toEqual([NO_LINE('`{`')])
  expect(prompts[0]).toBe(`${PRINTED}\nBuild it.\n\n${NO_LINE('`{`')}`)
  expect(ran).toEqual([`get ${BOARD} ${PATH}`, `brief ${BOARD} ${PATH}`])
  const plain = await agent($, GOOD)
  expect(plain.context).toBeUndefined()
  expect(prompts[1]).toBe(GOOD)
})

test('a brief with a changed key is advised not_rendered in the prompt and the result, and the verbatim one is not', async ($, on) => {
  const { prompts } = engine(on, {})
  const stale = `${LINE}\n${JSON.stringify({ ...RENDERED, left_out: 3 }, null, 2)}`
  const advice = NOT_RENDERED('its brief differs at `left_out`')
  expect((await agent($, stale)).context).toEqual([advice])
  expect(prompts[0]).toBe(`${stale}\n\n${advice}`)
  expect((await agent($, GOOD)).context).toBeUndefined()
})

test('both failed checks advise together', async ($, on) => {
  const { prompts } = engine(on, {})
  const advice = `${NO_LINE('`Build /changes/t.`')}\n${NOT_RENDERED('the prompt holds no brief as JSON')}`
  expect((await agent($, 'Build /changes/t.')).context).toEqual([advice])
  expect(prompts[0]).toBe(`Build /changes/t.\n\n${advice}`)
})

test('a prompt naming no change is untouched and reads nothing, and one naming a change in build is checked', async ($, on) => {
  const { ran, prompts } = engine(on, {})
  expect((await agent($, 'Find the widget code.')).context).toBeUndefined()
  expect(ran).toEqual([])
  expect((await agent($, 'Build /changes/t.')).context).toHaveLength(1)
  expect(prompts[0]).toBe('Find the widget code.')
})

test('a change in plan is untouched, and the same change in build is advised', async ($, on) => {
  const answers: Answers = { change: change('plan') }
  const { ran } = engine(on, answers)
  expect((await agent($, 'Build /changes/t.')).context).toBeUndefined()
  expect(ran).toEqual([`get ${BOARD} ${PATH}`])
  answers.change = change('build')
  expect((await agent($, 'Build /changes/t.')).context).toHaveLength(1)
})

test('a change no board holds is untouched, and the change in build is advised', async ($, on) => {
  const answers: Answers = { change: MISSING }
  engine(on, answers)
  expect((await agent($, 'Build /changes/t.')).context).toBeUndefined()
  answers.change = change('build')
  expect((await agent($, 'Build /changes/t.')).context).toHaveLength(1)
})

test('a render that fails leaves the call untouched, and one that renders advises', async ($, on) => {
  const answers: Answers = { rendered: 'fails' }
  const { prompts } = engine(on, answers)
  expect((await agent($, 'Build /changes/t.')).context).toBeUndefined()
  expect(prompts[0]).toBe('Build /changes/t.')
  answers.rendered = undefined
  expect((await agent($, 'Build /changes/t.')).context).toHaveLength(1)
})
