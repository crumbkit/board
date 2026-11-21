import { atom, read, update } from 'claude-code'
import type { EngineInterface, PluginOptions, Register, RenderElement } from 'claude-code'

import type { Open, Question, Unread, Waiting } from '../types'
import { SLOT, drawsInto, joinBand, keepOneBand } from './band'
import { answerText, approvedChange, blocking, countBadge, failure, parseAnswered, parseOpen, parseRows, parseWaiting, plain, project, refused, report, REPORT_LIMIT, REPORT_SQL, WRITTEN_BY } from './plans'

// The mod reads the owner's open questions from Board, draws their count in
// the band above the prompt and lists them in a pane, where the owner answers
// one with a press or in words. /yesterday shows the morning report in a
// second pane. An answer goes through `board call plans.answer`; it is the
// owner's own press, so nothing gates it.

const open = atom({ plugin: 'board-plans', key: 'open' } as const, null as Open | null)
const morning = atom({ plugin: 'board-plans', key: 'report' } as const, null as string | null)
const waiting = atom({ plugin: 'board-plans', key: 'waiting' } as const, null as Waiting | null)

// Why the last read of each count failed, so a failure shows in the band.
const failed = atom({ plugin: 'board-plans', key: 'failed' } as const, { open: null, waiting: null } as Unread)

const QUESTIONS = 'questions'
// Board lives in ~/.local/bin, which a session's PATH may lack.
const RUN = 'PATH="$HOME/.local/bin:/opt/homebrew/bin:/usr/local/bin:$PATH" exec board "$@"'
// A slow or hung board holds a refresh or an answer no longer than this.
const TIMEOUT_MS = 5000
// plans.answer takes board's lock twice, for the answer and for the attach,
// and each take may wait up to 5 seconds.
const ANSWER_MS = 15_000
const REFRESH_MS = 60_000
// The pane lists this many questions; the band counts them all.
const LISTED = 100

type Ran = { exitCode: number; stdout: string; stderr: string }

/** Runs `board` with `args` in the configured board, the personal board when none is set. */
function board($: EngineInterface, options: PluginOptions, args: string[], timeoutMs = TIMEOUT_MS): Promise<Ran> {
  const dir = String(options.board ?? '')
  const where = dir === '' ? ['--personal'] : ['-C', dir]
  return $.process.run(['/bin/sh', '-c', RUN, 'board', ...where, '--json', ...args], { timeoutMs })
}

/**
 * Reads what waits on the owner. Until the plans plugin has `waiting`, it
 * says so once and is not asked again while the module is loaded.
 */
async function refreshWaiting($: EngineInterface, options: PluginOptions, local: { absent: boolean }) {
  if (local.absent) return
  try {
    const ran = await board($, options, ['call', 'plans.waiting'])
    const read = parseWaiting(ran.exitCode, ran.stdout, ran.stderr)
    if (read.kind === 'absent') local.absent = true
    if (read.kind === 'failed') $.ui.log(`board: plans.waiting failed: ${read.why}`, { to: 'debug' })
    if (read.kind === 'read') await update($, waiting, () => read.waiting)
    await update($, failed, last => ({ ...last, waiting: read.kind === 'failed' ? read.why : null }))
  } catch (error) {
    $.ui.log(`board: plans.waiting did not run: ${String(error)}`, { to: 'debug' })
    await update($, failed, last => ({ ...last, waiting: `plans.waiting did not run: ${String(error)}` }))
  }
}

/** Reads the open questions again. A failure is logged, kept for the band and keeps the last count read. */
async function refreshOpen($: EngineInterface, options: PluginOptions) {
  let found: Open | null = null
  let why: string | null = null
  try {
    const ran = await board($, options, ['call', 'plans.open', '--limit', String(LISTED)])
    found = ran.exitCode === 0 ? parseOpen(ran.stdout) : null
    if (found === null) why = failure(ran.exitCode, ran.stdout, ran.stderr)
  } catch (error) {
    why = `plans.open did not run: ${String(error)}`
  }
  if (why !== null) $.ui.log(`board: plans.open failed: ${why}`, { to: 'debug' })
  if (found !== null) await update($, open, () => found)
  await update($, failed, last => ({ ...last, open: why }))
}

/** Reads the open questions and what waits on the owner, side by side. */
async function refresh($: EngineInterface, options: PluginOptions, local: { absent: boolean }) {
  await Promise.all([refreshOpen($, options), refreshWaiting($, options, local)])
}

/**
 * Records the owner's answer through `plans.answer`, which refuses a question
 * that is no longer open or already has an answer at any id, and attaches the
 * answer to the change the question blocks. Then it tries to send the answer
 * to the session that asked and to each session that asked the same question.
 *
 * Only Board's own failure report says nothing was written; a run that timed
 * out or was killed may have recorded the answer, so nothing is sent then.
 *
 * Once the answer is recorded, the send is best effort. It
 * names each asking session's `local_` id as a string, the SendMessage tool's
 * spelling of a recipient, because `{ sessionId }` refuses a `local_` id.
 */
async function answer($: EngineInterface, options: PluginOptions, q: Question, choice: string | undefined, words: string) {
  try {
    // The desktop app names this session's `local_` id, the form questions use.
    const host = await $.env.get('CLAUDE_CODE_HOST_SESSION_ID')
    // Each input is a flag; the `=` form keeps words that start with `-` as the value.
    const args = ['call', 'plans.answer', `--path=${q.path}`, `--option=${choice ?? 'none'}`, `--words=${words}`]
    if (host?.startsWith('local_')) args.push(`--session=${host}`)
    args.push(`--written_by=${WRITTEN_BY}`)
    const ran = await board($, options, args, ANSWER_MS).catch(() => null)
    if (ran?.exitCode !== 0) {
      const why = ran === null ? null : refused(ran.stdout)
      return $.ui.toast(
        why === null
          ? 'The answer may have been recorded, because board did not finish. The question stays in the list until plans.open stops listing it.'
          : `No answer was recorded: ${why}`,
      )
    }
    await update($, open, found => found && { questions: found.questions.filter(o => o.path !== q.path), total: found.total - 1 })
    const answered = parseAnswered(ran.stdout)
    const text = answerText(q.path, answered.answer, choice, words)
    const send = async (to: string) => {
      try {
        return (await $.session.send({ to, text })).isDelivered
      } catch {
        // A send that rejects is a send refused; the answer is in Board all the same.
        return false
      }
    }
    // A board from before plans.ask merged questions lists no `also_asked`.
    const others = (q.also_asked ?? []).filter(to => to !== q.session)
    const [sent] = await Promise.all([send(q.session), ...others.map(send)])
    const { attached } = answered
    const unattached = attached?.refused === undefined ? null : `not attached to ${attached.change}: ${attached.refused}`
    const approval = await askApproval($, options, approvedChange(q, choice, attached))
    $.ui.toast(
      (sent
        ? `Answered, and sent to ${q.session}${unattached === null ? '' : `. The answer is recorded but was ${unattached}`}`
        : `Answered and recorded in Board${unattached === null ? '' : `, but ${unattached}`}. The message to ${q.session} was not delivered`) + approval,
    )
  } catch (error) {
    $.ui.toast(`No answer was recorded: ${String(error)}`)
  }
}

/**
 * Asks the owner, through `changes.approve`, to approve with Touch ID the
 * change the answer approves, which only that signed approval proves. The
 * toast's sentence about it; empty when the answer approves no change.
 */
async function askApproval($: EngineInterface, options: PluginOptions, change: string | null): Promise<string> {
  if (change === null) return ''
  const ran = await board($, options, ['call', 'changes.approve', `--path=${change}`], ANSWER_MS).catch(() => null)
  if (ran?.exitCode === 0) return `. The approval of ${change} waits for Touch ID in the effects band`
  const why = ran === null ? 'board did not finish' : failure(ran.exitCode, ran.stdout, ran.stderr)
  return `. No approval of ${change} was asked (${why}); run board call changes.approve --path ${change}`
}

const age = (asked: string, now: number) => {
  const hours = Math.floor((now - Date.parse(asked)) / 3_600_000)
  return hours < 1 ? 'new' : hours < 48 ? `${hours}h` : `${Math.floor(hours / 24)}d`
}

export const register: Register = (on, options) => {
  const local = { absent: false }

  // The minute's refresh starts with the first band drawn, which a `-p` run
  // never draws. Desktop sessions report `isInteractive: false`, so that flag
  // cannot gate it.
  let polling = false

  on('session.start', async ($, e, next) => {
    const started = await next(e)
    await $.command.register({ name: 'yesterday', description: "Show the morning report of yesterday's Board records, from 07:00" })
    await refresh($, options, local)
    return started
  })

  // The host asks each plugin for its own row; this mod draws into every Board mod's.
  keepOneBand(on)
  on('ui.render', { component: 'AbovePrompt', plugin: '*' }, async ($, e, next) => {
    const below = await next(e)
    if (!polling) {
      polling = true
      $.clock.every(REFRESH_MS, () => void refresh($, options, local))
    }
    if (e.props.hasSurvey || !drawsInto(e.plugin)) return below
    // plans.waiting counts the open questions; when it has not answered, the button counts plans.open's.
    const badge = countBadge(await read($, waiting), await read($, open), await read($, failed))
    if (badge === null) return below
    const { Box, Button, Text } = $.ui.resolve(e)
    const part = (
      <Box key={SLOT.waiting}>
        <Button key="waiting" label={badge.label} onPress={() => void $.ui.open({ id: QUESTIONS, title: 'Open questions' })}>
          <Text color={badge.problem ? 'red' : 'warning'}>{badge.label}</Text>
        </Button>
      </Box>
    )
    return joinBand(below, [part as RenderElement])
  })

  on('ui.render', { component: 'Pane', requestId: QUESTIONS }, async ($, e) => {
    const found = await read($, open)
    const unread = (await read($, failed)).open
    const now = await $.clock.now()
    const ui = $.ui.resolve(e)
    const { Box, Button, Text } = ui
    // Mobile draws no Input, so a question there is answered by its options alone.
    const Input = 'Input' in ui ? ui.Input : undefined
    const problem =
      unread === null ? null : (
        <Text color="red">{plain(`The open questions could not be read: ${unread}.${found === null ? '' : ' This list is from the last read that worked.'}`)}</Text>
      )
    if (found === null || found.questions.length === 0) return problem ?? <Text dimColor>No open questions.</Text>
    const more = found.total - found.questions.length
    return (
      <Box flexDirection="column">
        {problem}
        {found.questions.map(q => (
          <Box key={q.path} flexDirection="column" marginBottom={1}>
            <Text dimColor>
              {project(q.path)} · {age(q.asked, now)}
              {blocking(q)}
            </Text>
            <Text>{q.question}</Text>
            <Box flexWrap="wrap">
              {q.options.map(o => (
                <Button
                  key={`${q.path}#${o.id}`}
                  label={`${o.id}: ${o.text}`}
                  variant={o.id === q.recommendation ? 'primary' : undefined}
                  onPress={() => answer($, options, q, o.id, o.text)}
                />
              ))}
            </Box>
            {Input === undefined ? null : (
              <Input key={`${q.path}#words`} placeholder="Answer in your own words" submitLabel="answer" onSubmit={words => {
                if (words.trim() !== '') void answer($, options, q, undefined, words)
              }} />
            )}
          </Box>
        ))}
        {more > 0 ? <Text dimColor>{more} more are open; `board call plans.open` lists them.</Text> : null}
      </Box>
    )
  })

  // The report is a plain list of record fields, drawn as Markdown. Once
  // board-ui lands (#301), this pane moves to its view and binding
  // records, and a short narrative per project from `$.model.complete` with a
  // Haiku-class model may lead it.
  on('command.run', { command: 'yesterday' }, async $ => {
    const ran = await board($, options, ['query', REPORT_SQL, '--limit', String(REPORT_LIMIT)])
    const found = ran.exitCode === 0 ? parseRows(ran.stdout) : null
    if (found === null) return { text: `board: the morning report could not be read: ${failure(ran.exitCode, ran.stdout, ran.stderr)}` }
    const built = report(found.rows, found.truncated)
    await update($, morning, () => built.markdown)
    await $.ui.open({ id: 'yesterday', title: 'Yesterday' })
    return { text: built.line }
  }).catch((_$, _e) => ({ text: 'board: the morning report did not run; `claude --debug` logs why' }))

  on('ui.render', { component: 'Pane', requestId: 'yesterday' }, async ($, e) => {
    const { Markdown, Text } = $.ui.resolve(e)
    const markdown = await read($, morning)
    return markdown === null ? <Text dimColor>Run /yesterday.</Text> : <Markdown text={markdown} />
  })
}
