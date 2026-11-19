import { expect, test } from 'claude-code/testing'
import type { On } from 'claude-code'
import type { Engine } from 'claude-code/testing'

import { changesIn, noLine, notRendered, objectsIn } from '../hooks/brief'
import type { Expected } from '../hooks/brief'
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
const PLAN = '/changes/t/plans/1'
// The golden's change is in plan; a builder starts from the brief of a change in build.
const BUILT = JSON.stringify({ ...(JSON.parse(PRINTED) as Record<string, unknown>), phase: 'build' }, null, 2)
const EXPECTED: Expected = { brief_line: LINE, path: PATH, phase: 'build', plan: '/changes/t/plan.md' }
const report = (data: unknown) => JSON.stringify({ schema: 'board/get-report/v2', data })
const change = (phase: string) =>
  report([{ record: { $type: 'changes/change/v2', title: 't', mount: '/repos/x', phase, plan: PLAN } }])
const PLANNED = report([{ record: { $type: 'changes/plan/v1', text: '/changes/t/plan.md', uses: [] } }])
const notFound = (key: string) =>
  JSON.stringify({
    schema: 'board/get-report/v2',
    data: [],
    failures: [{ key, code: 'board::path::not_found', message: `nothing is at ${key}` }],
  })

/** The brief as a lead should send it: the brief line first, then the rendered brief. */
const GOOD = `${LINE}\n\n${BUILT}\nBuild it.`

type Answers = {
  /** What `board get` prints for each change; /changes/t in build when left out, and nothing at any other. */
  changes?: Record<string, string>
  /** What `board get` prints for the plan record, or 'fails'; PLANNED when left out. */
  plan?: string | 'fails'
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
    const [board, key = ''] = e.argv.slice(4)
    ran.push(`get ${board} ${key}`)
    if (key === PLAN) return answers.plan === 'fails' ? run(1, '') : run(0, answers.plan ?? PLANNED)
    const held = (answers.changes ?? { [PATH]: change('build') })[key]
    return held === undefined ? run(1, notFound(key)) : run(0, held)
  })
  on('tool.call', (_, e) => {
    if (e.tool === 'Agent') prompts.push(e.prompt)
    return { result: {} } as never
  })
  return { ran, prompts }
}

const agent = ($: Engine, prompt: string) => $.tool.call({ tool: 'Agent', description: 'Build it', prompt })

const NO_LINE = (first: string, path = PATH) =>
  `changes::brief::no_line: if this agent builds ${path}, start its prompt with \`board-brief: ${path}\`, ` +
  `not \`${first}\`.`
const NOT_RENDERED = (what: string) =>
  `changes::brief::not_rendered: if this agent builds ${PATH}, send it the brief ` +
  `\`board -C ${BOARD} call changes.brief --path ${PATH}\` prints now; ${what}.`

test('a brief line names the change first, and otherwise only a path that starts a word does', () => {
  expect(changesIn(GOOD)).toEqual([PATH])
  expect(changesIn(`Read /changes/other first.\n${LINE}`)).toEqual([PATH, '/changes/other'])
  expect(changesIn('Build /changes/fix-it. Then `/changes/b` and "/changes/c" (/changes/d)')).toEqual([
    '/changes/fix-it',
    '/changes/b',
    '/changes/c',
    '/changes/d',
  ])
  expect(changesIn('Edit plugins/changes/claude-code and ~/Code/board/changes/x/plans/1.md.')).toEqual([])
  expect(changesIn('Build the widget.')).toEqual([])
})

test('a brief line that is not first is advised, and the same brief with it first is not', () => {
  expect(noLine(`${BUILT}\n${LINE}`, PATH)).toBe(NO_LINE('{'))
  expect(noLine(`Please build it.\n${LINE}`, PATH)).toBe(NO_LINE('Please build it.'))
  expect(noLine(`board-brief: /changes/other\n${BUILT}`, PATH)).toBe(NO_LINE('board-brief: /changes/other'))
  expect(noLine(`\n  ${LINE}  \n${BUILT}`, PATH)).toBeNull()
})

test('every JSON object that opens on a line of its own is read, and a broken one is skipped', () => {
  const built = JSON.parse(BUILT) as unknown
  expect(objectsIn(GOOD)).toEqual([built])
  expect(objectsIn(`${BUILT}\n${BUILT.replace('"path":', '"path"')}\n${BUILT}`)).toEqual([built, built])
  expect(objectsIn('No JSON here.')).toEqual([])
})

test('a brief whose path, phase or plan differs is advised with that field, and the current brief is not', () => {
  const sent = (fields: object) => `${LINE}\n${JSON.stringify({ ...(JSON.parse(BUILT) as object), ...fields }, null, 2)}`
  expect(notRendered(sent({ phase: 'plan' }), EXPECTED, BOARD)).toBe(
    NOT_RENDERED('its `phase` is "plan", and the change\'s is "build"'),
  )
  expect(notRendered(sent({ plan: '/changes/t/plans/0.md' }), EXPECTED, BOARD)).toBe(
    NOT_RENDERED('its `plan` is "/changes/t/plans/0.md", and the change\'s is "/changes/t/plan.md"'),
  )
  expect(notRendered(sent({ path: '/changes/u', brief_line: 'board-brief: /changes/u' }), EXPECTED, BOARD)).toBe(
    NOT_RENDERED('its `brief_line` is "board-brief: /changes/u", and the change\'s is "board-brief: /changes/t"'),
  )
  expect(notRendered(GOOD, EXPECTED, BOARD)).toBeNull()
})

test('a prompt with no JSON brief is advised, and one whose second brief object matches is not', () => {
  expect(notRendered(`${LINE}\nBuild it.`, EXPECTED, BOARD)).toBe(NOT_RENDERED('the prompt holds no brief as JSON'))
  const stale = JSON.stringify({ ...(JSON.parse(BUILT) as object), phase: 'plan' }, null, 2)
  expect(notRendered(`${LINE}\n${stale}\n${BUILT}`, EXPECTED, BOARD)).toBeNull()
})

test('a JSON-first prompt is advised no_line, and the same brief with the line first runs untouched', async ($, on) => {
  const { ran, prompts } = engine(on, {})
  const advised = await agent($, `${BUILT}\nBuild it.`)
  expect(advised.context).toEqual([NO_LINE('{')])
  expect(prompts[0]).toBe(`${BUILT}\nBuild it.\n\n${NO_LINE('{')}`)
  expect(ran).toEqual([`get ${BOARD} ${PATH}`, `get ${BOARD} ${PLAN}`])
  const plain = await agent($, GOOD)
  expect(plain.context).toBeUndefined()
  expect(prompts[1]).toBe(GOOD)
})

test('a brief rendered in another phase is advised not_rendered in the prompt and the result, and the current one is not', async ($, on) => {
  const { prompts } = engine(on, {})
  const stale = `${LINE}\n${PRINTED}`
  const advice = NOT_RENDERED('its `phase` is "plan", and the change\'s is "build"')
  expect((await agent($, stale)).context).toEqual([advice])
  expect(prompts[0]).toBe(`${stale}\n\n${advice}`)
  expect((await agent($, GOOD)).context).toBeUndefined()
})

test('both failed checks advise together', async ($, on) => {
  const { prompts } = engine(on, {})
  const advice = `${NO_LINE('Build /changes/t.')}\n${NOT_RENDERED('the prompt holds no brief as JSON')}`
  expect((await agent($, 'Build /changes/t.')).context).toEqual([advice])
  expect(prompts[0]).toBe(`Build /changes/t.\n\n${advice}`)
})

test('a prompt naming no change is untouched and reads nothing, and one naming a change in build is checked', async ($, on) => {
  const { ran, prompts } = engine(on, {})
  expect((await agent($, 'Find the widget code in plugins/changes/claude-code.')).context).toBeUndefined()
  expect(ran).toEqual([])
  expect((await agent($, 'Build /changes/t.')).context).toHaveLength(1)
  expect(prompts[0]).toBe('Find the widget code in plugins/changes/claude-code.')
})

test('a change in plan is untouched, and the same change in build is advised', async ($, on) => {
  const answers: Answers = { changes: { [PATH]: change('plan') } }
  const { ran } = engine(on, answers)
  expect((await agent($, 'Build /changes/t.')).context).toBeUndefined()
  expect(ran).toEqual([`get ${BOARD} ${PATH}`])
  answers.changes = undefined
  expect((await agent($, 'Build /changes/t.')).context).toHaveLength(1)
})

test('a later candidate in build is checked when an earlier one is not a change', async ($, on) => {
  const { ran } = engine(on, {})
  const prompt = 'See /changes/missing, then build /changes/t.'
  expect((await agent($, prompt)).context).toEqual([
    `${NO_LINE(prompt)}\n${NOT_RENDERED('the prompt holds no brief as JSON')}`,
  ])
  expect(ran).toEqual([`get ${BOARD} /changes/missing`, `get ${BOARD} ${PATH}`, `get ${BOARD} ${PLAN}`])
})

test('a change named fourth is not read, and the same change named third is checked', async ($, on) => {
  const { ran } = engine(on, {})
  const fourth = 'See /changes/a, /changes/b and /changes/c, then build /changes/t.'
  expect((await agent($, fourth)).context).toBeUndefined()
  expect(ran).toEqual(['/changes/a', '/changes/b', '/changes/c'].map(path => `get ${BOARD} ${path}`))
  expect((await agent($, 'See /changes/a and /changes/b, then build /changes/t.')).context).toHaveLength(1)
})

test('a change whose lookup fails is skipped for the next one named, and alone it leaves the call untouched', async ($, on) => {
  const failed = JSON.stringify({
    schema: 'board/get-report/v2',
    data: [],
    failures: [{ key: '/changes/bad', code: 'board::record::invalid', message: '/changes/bad fails its schema' }],
  })
  engine(on, { changes: { '/changes/bad': failed, [PATH]: change('build') } })
  expect((await agent($, 'See /changes/bad, then build /changes/t.')).context).toHaveLength(1)
  expect((await agent($, 'See /changes/bad.')).context).toBeUndefined()
})

test('a plan record that cannot be read leaves the call untouched, and one that reads advises', async ($, on) => {
  const answers: Answers = { plan: 'fails' }
  const { prompts } = engine(on, answers)
  expect((await agent($, 'Build /changes/t.')).context).toBeUndefined()
  expect(prompts[0]).toBe('Build /changes/t.')
  answers.plan = undefined
  expect((await agent($, 'Build /changes/t.')).context).toHaveLength(1)
})
