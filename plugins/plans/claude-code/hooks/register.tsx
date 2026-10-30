import { atom, read, update } from 'claude-code'
import type { EngineInterface, PluginOptions, Register } from 'claude-code'

import type { Open, Question } from '../types'
import { answerBatch, answerText, failure, openLine, parseOpen, parseRows, project, questionSql, refusal, report, REPORT_LIMIT, REPORT_SQL } from './plans'

// The mod reads the owner's open questions from Board, draws their count in
// the band above the prompt and lists them in a pane, where the owner answers
// one with a press or in words. /yesterday shows the morning report in a
// second pane. Every write goes through `board put`; an answer is the owner's
// own press, so nothing gates it.

const open = atom({ plugin: 'board-plans', key: 'open' } as const, null as Open | null)
const morning = atom({ plugin: 'board-plans', key: 'report' } as const, null as string | null)

const QUESTIONS = 'questions'
// Board lives in ~/.local/bin, which a session's PATH may lack.
const RUN = 'PATH="$HOME/.local/bin:/opt/homebrew/bin:/usr/local/bin:$PATH" exec board "$@"'
// A slow or hung board holds a refresh or an answer no longer than this.
const TIMEOUT_MS = 5000
// The put waits this long for board's lock, well inside TIMEOUT_MS, so a
// held lock fails as `board::lock` rather than as a killed process.
const LOCK_WAIT = '2s'
const REFRESH_MS = 60_000
// The pane lists this many questions; the band counts them all.
const LISTED = 100

type Ran = { exitCode: number; stdout: string; stderr: string }

/** Runs `board` with `args` in the configured board, the personal board when none is set. */
function board($: EngineInterface, options: PluginOptions, args: string[], stdin?: string): Promise<Ran> {
  const dir = String(options.board ?? '')
  const where = dir === '' ? ['--personal'] : ['-C', dir]
  return $.process.run(['/bin/sh', '-c', RUN, 'board', ...where, '--json', ...args], {
    timeoutMs: TIMEOUT_MS,
    ...(stdin === undefined ? {} : { stdin }),
  })
}

/** Reads the open questions again. A failure is logged and keeps the last count read. */
async function refresh($: EngineInterface, options: PluginOptions) {
  let found: Open | null = null
  try {
    const ran = await board($, options, ['call', 'plans.open', '--limit', String(LISTED)])
    found = ran.exitCode === 0 ? parseOpen(ran.stdout) : null
    if (found === null) $.ui.log(`board: plans.open failed: ${failure(ran.exitCode, ran.stdout, ran.stderr)}`, { to: 'debug' })
  } catch (error) {
    $.ui.log(`board: plans.open did not run: ${String(error)}`, { to: 'debug' })
  }
  if (found !== null) await update($, open, () => found)
}

/** Reads the question and its answers; undefined when board fails. */
async function held($: EngineInterface, options: PluginOptions, path: string) {
  const ran = await board($, options, ['query', questionSql(path)])
  return ran.exitCode === 0 ? parseRows(ran.stdout)?.rows : undefined
}

/**
 * Records the owner's answer and the question's new status in one batch, then
 * tries to send the answer to the session that asked. A question that is no
 * longer open, or that already has an answer at any id, is refused before the
 * put; one that changes between the read and the put is refused by the put's
 * `expect`, and is read again to say why.
 *
 * The answer is recorded in Board either way; the send is best effort. It
 * names the asking session's `local_` id as a string, the SendMessage tool's
 * spelling of a recipient, because `{ sessionId }` refuses a `local_` id.
 */
async function answer($: EngineInterface, options: PluginOptions, q: Question, choice: string, words: string) {
  try {
    const rows = await held($, options, q.path)
    const refused = refusal(q.path, rows)
    const row = rows?.find(r => r.kind === 'question')
    if (refused !== null || row === undefined) return $.ui.toast(refused ?? `${q.path} could not be read, so nothing was recorded`)
    // The desktop app names this session's `local_` id, the form questions use.
    const host = await $.env.get('CLAUDE_CODE_HOST_SESSION_ID')
    const by = host?.startsWith('local_') ? host : undefined
    const batch = answerBatch(q.path, String(row.blob), String(row.body), choice, words, by, await $.clock.now())
    const put = await board($, options, ['put', '--batch', '--wait', LOCK_WAIT], batch)
    if (put.exitCode !== 0) {
      const why = failure(put.exitCode, put.stdout, put.stderr)
      const since = why.startsWith('board::put::stale') ? refusal(q.path, await held($, options, q.path)) : null
      return $.ui.toast(since ?? `No answer was recorded: ${why}`)
    }
    await update($, open, found => found && { questions: found.questions.filter(o => o.path !== q.path), total: found.total - 1 })
    let sent = false
    try {
      sent = (await $.session.send({ to: q.session, text: answerText(q.path, choice, words) })).isDelivered
    } catch {
      // A send that rejects is a send refused; the answer is in Board all the same.
    }
    $.ui.toast(sent ? `Answered, and sent to ${q.session}` : `Answered and recorded in Board. The message to ${q.session} was not delivered`)
  } catch (error) {
    $.ui.toast(`No answer was recorded: ${String(error)}`)
  }
}

const age = (asked: string, now: number) => {
  const hours = Math.floor((now - Date.parse(asked)) / 3_600_000)
  return hours < 1 ? 'new' : hours < 48 ? `${hours}h` : `${Math.floor(hours / 24)}d`
}

export const register: Register = (on, options) => {
  // The minute's refresh starts with the first band drawn, which a `-p` run
  // never draws. Desktop sessions report `isInteractive: false`, so that flag
  // cannot gate it.
  let polling = false

  on('session.start', async ($, e, next) => {
    const started = await next(e)
    await $.command.register({ name: 'yesterday', description: "Show the morning report of yesterday's Board records, from 07:00" })
    await refresh($, options)
    return started
  })

  on('ui.render', { component: 'AbovePrompt' }, async ($, e, next) => {
    const below = await next(e)
    if (!polling) {
      polling = true
      $.clock.every(REFRESH_MS, () => void refresh($, options))
    }
    const found = await read($, open)
    if (e.props.hasSurvey || found === null || found.total === 0) return below
    const { Box, Button, Text } = $.ui.resolve(e)
    return (
      <Box flexDirection="column">
        {below.type === 'engine' ? null : below}
        <Box>
          <Text>{openLine(found.total)} </Text>
          <Button key="show" label="Show" onPress={() => void $.ui.open({ id: QUESTIONS, title: 'Open questions' })} />
        </Box>
      </Box>
    )
  })

  on('ui.render', { component: 'Pane', requestId: QUESTIONS }, async ($, e) => {
    const found = await read($, open)
    const now = await $.clock.now()
    const ui = $.ui.resolve(e)
    const { Box, Button, Text } = ui
    // Mobile draws no Input, so a question there is answered by its options alone.
    const Input = 'Input' in ui ? ui.Input : undefined
    if (found === null || found.questions.length === 0) return <Text dimColor>No open questions.</Text>
    const more = found.total - found.questions.length
    return (
      <Box flexDirection="column">
        {found.questions.map(q => (
          <Box key={q.path} flexDirection="column" marginBottom={1}>
            <Text dimColor>
              {project(q.path)} · {age(q.asked, now)}
              {q.blocks === null ? '' : ` · blocks ${q.blocks}`}
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
                if (words.trim() !== '') void answer($, options, q, 'none', words)
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
