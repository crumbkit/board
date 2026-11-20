import { expect, test } from 'claude-code/testing'
import type { On } from 'claude-code'
import type { Engine } from 'claude-code/testing'

import { briefLineIn, noLine, notRendered, objectsIn } from '../hooks/brief'
import type { Expected } from '../hooks/brief'
import PRINTED from './brief.golden'

// These prove what the Agent hook advises a lead whose builder prompt has a
// brief line for a change in build, that the advice reaches only the lead's
// result, and that the builder gets the prompt unchanged. `board` is stood in by a process.run hook and the board
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
const NO_BRIEF = 'the prompt holds no JSON object with a `brief_line` field, such as `changes.brief` prints'

const reordered = (json: string) => {
  const brief = JSON.parse(json) as Record<string, unknown>
  return JSON.stringify(Object.fromEntries(Object.keys(brief).reverse().map(key => [key, brief[key]])), null, 2)
}
const COMPACT = JSON.stringify(JSON.parse(BUILT))

test('the change is the first board-brief line, and a path named any other way is none', () => {
  expect(briefLineIn(GOOD)).toBe(PATH)
  expect(briefLineIn(`Read /changes/other first.\n  ${LINE}  \nboard-brief: /changes/u`)).toBe(PATH)
  expect(briefLineIn('Review /changes/t and `board-brief: /changes/t` in the docs.')).toBeNull()
  expect(briefLineIn('Build the widget.')).toBeNull()
})

test('a brief line with trailing punctuation names the change without it', () => {
  expect(briefLineIn(`${LINE}.`)).toBe(PATH)
  expect(briefLineIn(`Build it from this.\n${LINE},\nThen report.`)).toBe(PATH)
})

test('a prompt that starts with the rendered brief and has no brief line names its change', () => {
  expect(briefLineIn(`${BUILT}\nBuild it.`)).toBe(PATH)
  expect(briefLineIn(`  ${COMPACT}\nBuild it.`)).toBe(PATH)
})

test('a first brief line with punctuation after its path is advised no_line, since the edit guard reads it as written', () => {
  expect(noLine(`${LINE}.\nBuild it.`, PATH)).toBe(NO_LINE(`${LINE}.`))
})

test('a brief line that is not first is advised, and the same brief with it first is not', () => {
  expect(noLine(`${BUILT}\n${LINE}`, PATH)).toBe(NO_LINE('{'))
  expect(noLine(`Please build it.\n${LINE}`, PATH)).toBe(NO_LINE('Please build it.'))
  expect(noLine(`\n  ${LINE}  \n${BUILT}`, PATH)).toBeNull()
})

test('every JSON object in the text is read in any layout, and a broken one is skipped', () => {
  expect(objectsIn('Use {x} here: {"a":1} then {"b":{"c":2}} and {"d":')).toEqual([{ a: 1 }, { b: { c: 2 } }])
  expect(objectsIn('A brace in a string: {"s":"}{"} done.')).toEqual([{ s: '}{' }])
  expect(objectsIn('{"s":"a\\"}{"} x')).toEqual([{ s: 'a"}{' }])
  const built = JSON.parse(BUILT) as unknown
  expect(objectsIn(GOOD)).toEqual([built])
  expect(objectsIn(`Before.${COMPACT}After.`)).toEqual([built])
  const broken = objectsIn(`${BUILT}\n${BUILT.replace('"path":', '"path"')}\n${BUILT}`)
  expect(broken.filter(sent => typeof sent === 'object' && sent !== null && 'brief_line' in sent)).toEqual([built, built])
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

test('a prompt with no JSON brief is advised what was looked for, and one whose second brief object matches is not', () => {
  expect(notRendered(`${LINE}\nBuild it.`, EXPECTED, BOARD)).toBe(NOT_RENDERED(NO_BRIEF))
  const stale = JSON.stringify({ ...(JSON.parse(BUILT) as object), phase: 'plan' }, null, 2)
  expect(notRendered(`${LINE}\n${stale}\n${BUILT}`, EXPECTED, BOARD)).toBeNull()
})

test('an advised call reaches the agent with its prompt unchanged and the advice only in the result, and a correct brief gets none', async ($, on) => {
  const { ran, prompts } = engine(on, {})
  const prompt = `Please build it.\n${LINE}\nBuild it.`
  const advised = await agent($, prompt)
  expect(advised.context).toEqual([`${NO_LINE('Please build it.')}\n${NOT_RENDERED(NO_BRIEF)}`])
  expect(prompts[0]).toBe(prompt)
  expect(ran).toEqual([`get ${BOARD} ${PATH}`, `get ${BOARD} ${PLAN}`])
  const plain = await agent($, GOOD)
  expect(plain.context).toBeUndefined()
  expect(prompts[1]).toBe(GOOD)
})

test('a prompt naming a change in build without a brief line runs untouched and reads nothing, and with the line it is checked', async ($, on) => {
  const { ran, prompts } = engine(on, {})
  const review = 'Review /changes/t against its plan at `/changes/t/plans/1`.'
  expect((await agent($, review)).context).toBeUndefined()
  expect(ran).toEqual([])
  expect(prompts[0]).toBe(review)
  expect((await agent($, `${LINE}\n${review}`)).context).toEqual([NOT_RENDERED(NO_BRIEF)])
})

test('a prompt that starts with the rendered brief and has no brief line is advised no_line', async ($, on) => {
  const { prompts } = engine(on, {})
  const prompt = `${BUILT}\nBuild it.`
  expect((await agent($, prompt)).context).toEqual([NO_LINE('{')])
  expect(prompts[0]).toBe(prompt)
})

test('the agent starts before the board lookups finish, with the event it was given', async ($, on) => {
  let started: () => void = () => undefined
  const agentStarted = new Promise<void>(resolve => (started = resolve))
  const order: string[] = []
  on('env.get', (_, e) => ({ value: e.name === 'HOME' ? HOME : undefined }))
  on('fs.exists', (_, e) => ({ value: e.path === CONFIG }))
  on('fs.read', () => ({ value: JSON.stringify({ boards: [BOARD] }) }))
  on('process.run', async (_, e) => {
    const key = e.argv.slice(4)[1] ?? ''
    // A hook that ran the agent only after its lookups would wait here for good.
    const waited = await Promise.race([agentStarted.then(() => true), new Promise(r => setTimeout(() => r(false), 500))])
    order.push(waited ? `get ${key}` : `get ${key} before the agent`)
    const stdout = key === PLAN ? PLANNED : change('build')
    return { value: { exitCode: 0, stdout, stderr: '', isStdoutTruncated: false, isStderrTruncated: false } }
  })
  const sent = { tool: 'Agent', description: 'Build it', prompt: LINE } as const
  let received: Record<string, unknown> = {}
  on('tool.call', (_, e) => {
    // The engine gives the call its tool_use_id; every other field is the hook's input.
    const { tool_use_id: _id, ...rest } = e as unknown as Record<string, unknown>
    received = rest
    order.push('agent')
    started()
    return { result: {} } as never
  })
  const ran = await $.tool.call(sent)
  expect(order).toEqual(['agent', `get ${PATH}`, `get ${PLAN}`])
  expect(received).toEqual(sent)
  expect(ran.context).toEqual([NOT_RENDERED(NO_BRIEF)])
})

test('a brief line second is advised no_line, and the same prompt with it first is not', async ($, on) => {
  engine(on, {})
  expect((await agent($, `Build this.\n${LINE}\n${BUILT}`)).context).toEqual([NO_LINE('Build this.')])
  expect((await agent($, `${LINE}\nBuild this.\n${BUILT}`)).context).toBeUndefined()
})

test('a brief sent compact, reordered or followed by instructions passes, and one in another phase is advised', async ($, on) => {
  engine(on, {})
  expect((await agent($, `${LINE}\n${COMPACT}`)).context).toBeUndefined()
  expect((await agent($, `${LINE}\n\n${reordered(BUILT)}`)).context).toBeUndefined()
  expect((await agent($, `${LINE}\n\n${BUILT}\n\nThen run the tests.\nReport back.`)).context).toBeUndefined()
  expect((await agent($, `${LINE}\n${PRINTED}`)).context).toEqual([
    NOT_RENDERED('its `phase` is "plan", and the change\'s is "build"'),
  ])
})

test('a resent prompt that holds the advice gets it once in the result and still reaches the agent unchanged', async ($, on) => {
  const { prompts } = engine(on, {})
  const first = `Build this.\n${LINE}`
  const advice = (await agent($, first)).context?.[0] ?? ''
  const resent = `${first}\n\n${advice}`
  expect((await agent($, resent)).context).toEqual([advice])
  expect(prompts[1]).toBe(resent)
})

test('a change in plan is untouched, and the same change in build is advised', async ($, on) => {
  const answers: Answers = { changes: { [PATH]: change('plan') } }
  const { ran } = engine(on, answers)
  expect((await agent($, LINE)).context).toBeUndefined()
  expect(ran).toEqual([`get ${BOARD} ${PATH}`])
  answers.changes = undefined
  expect((await agent($, LINE)).context).toHaveLength(1)
})

test('a change whose lookup fails leaves the call untouched, and one that reads is advised', async ($, on) => {
  const failed = JSON.stringify({
    schema: 'board/get-report/v2',
    data: [],
    failures: [{ key: PATH, code: 'board::record::invalid', message: `${PATH} fails its schema` }],
  })
  const answers: Answers = { changes: { [PATH]: failed } }
  const { prompts } = engine(on, answers)
  expect((await agent($, LINE)).context).toBeUndefined()
  expect(prompts[0]).toBe(LINE)
  answers.changes = undefined
  expect((await agent($, LINE)).context).toHaveLength(1)
})

test('a plan record that cannot be read leaves the call untouched, and one that reads advises', async ($, on) => {
  const answers: Answers = { plan: 'fails' }
  const { prompts } = engine(on, answers)
  expect((await agent($, LINE)).context).toBeUndefined()
  expect(prompts[0]).toBe(LINE)
  answers.plan = undefined
  expect((await agent($, LINE)).context).toHaveLength(1)
})
