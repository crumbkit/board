import { atom, read, update } from 'claude-code'
import type { EngineInterface, PluginOptions, Register, RenderElement } from 'claude-code'

import type { Open, Question, Ready, Unread, Waiting } from '../types'
import { SLOT, drawsInto, joinBand, keepOneBand } from './band'
import { failingLabel, failingMode, failingPart, keptLines, kindLabel, parseReady, pullRef, readyBadge, readyFor, stateText, transitions, waitingLess, waitsOnFailing } from './ready'
import { begin, countCallsForList, end, isDue, REFRESH_MS, schedule, tickIsDue } from './refresh'
import type { Counted, Schedule } from './refresh'
import { unseen, whenChanged } from './stamp'
import type { Seen } from './stamp'
import { answerText, approvalText, approvedChange, blocking, countBadge, failure, parseAnswered, parseApproved, parseOpen, parseRows, parseWaiting, plain, project, refused, report, REPORT_LIMIT, REPORT_SQL, WRITTEN_BY } from './plans'

// The mod reads the owner's open questions from Board, draws their count in
// the band above the prompt and lists them in a pane, where the owner answers
// one with a press or in words. Beside them it reads the changes ready to land
// that wait only on the owner, draws their count just before Waiting and lists
// them in a pane of their own. /yesterday shows the morning report in a
// second pane. An answer goes through `board call plans.answer`; it is the
// owner's own press, so nothing gates it.

const open = atom({ plugin: 'board-plans', key: 'open' } as const, null as Open | null)
const morning = atom({ plugin: 'board-plans', key: 'report' } as const, null as string | null)
const waiting = atom({ plugin: 'board-plans', key: 'waiting' } as const, null as Waiting | null)

// Why the last read of each count failed, so a failure shows in the band.
const failed = atom({ plugin: 'board-plans', key: 'failed' } as const, { open: null, waiting: null } as Unread)

const ready = atom({ plugin: 'board-plans', key: 'ready' } as const, null as Ready | null)
const readyFailed = atom({ plugin: 'board-plans', key: 'readyFailed' } as const, null as string | null)

const QUESTIONS = 'questions'
const READY = 'ready'
// Board lives in ~/.local/bin, which a session's PATH may lack.
const RUN = 'PATH="$HOME/.local/bin:/opt/homebrew/bin:/usr/local/bin:$PATH" exec board "$@"'
// A slow or hung board holds a refresh or an answer no longer than this.
const TIMEOUT_MS = 5000
// changes.ready reads gates and runs merge checks, so its read gets longer. It
// runs beside the refresh, never awaited, so the band keeps the last count while it runs.
const READY_TIMEOUT_MS = 20_000
// plans.answer takes board's lock twice, for the answer and for the attach,
// and each take may wait up to 5 seconds.
const ANSWER_MS = 15_000
// The pane lists this many questions; the band counts them all.
const LISTED = 100

type Ran = { exitCode: number; stdout: string; stderr: string }

/** The configured board's directory; empty for the personal board. */
const configured = (options: PluginOptions) => String(options.board ?? '')

/** Runs `board` with `args` in the configured board, the personal board when none is set. */
function board($: EngineInterface, options: PluginOptions, args: string[], timeoutMs = TIMEOUT_MS): Promise<Ran> {
  const dir = configured(options)
  const where = dir === '' ? ['--personal'] : ['-C', dir]
  return $.process.run(['/bin/sh', '-c', RUN, 'board', ...where, '--json', ...args], { timeoutMs })
}

/**
 * Reads what waits on the owner. Until the plans plugin has `waiting`, it
 * says so once and is not asked again while the module is loaded.
 */
async function refreshWaiting($: EngineInterface, options: PluginOptions, local: { absent: boolean }): Promise<Counted> {
  if (local.absent) return { kind: 'absent' }
  try {
    const ran = await board($, options, ['call', 'plans.waiting'])
    const read = parseWaiting(ran.exitCode, ran.stdout, ran.stderr)
    if (read.kind === 'absent') local.absent = true
    if (read.kind === 'failed') $.ui.log(`board: plans.waiting failed: ${read.why}`, { to: 'debug' })
    if (read.kind === 'read') await update($, waiting, () => read.waiting)
    await update($, failed, last => ({ ...last, waiting: read.kind === 'failed' ? read.why : null }))
    return read.kind === 'read' ? { kind: 'read', total: read.waiting.total } : { kind: read.kind }
  } catch (error) {
    $.ui.log(`board: plans.waiting did not run: ${String(error)}`, { to: 'debug' })
    await update($, failed, last => ({ ...last, waiting: `plans.waiting did not run: ${String(error)}` }))
    return { kind: 'failed' }
  }
}

/** Where the mod records what the red failing part would have shown: `$XDG_STATE_HOME/board/ready-failing.jsonl`. */
async function statePath($: EngineInterface): Promise<string> {
  const state = (await $.env.get('XDG_STATE_HOME')) ?? `${(await $.env.get('HOME')) ?? ''}/.local/state`
  return `${state}/board/ready-failing.jsonl`
}

/**
 * Appends a line to the state file for each ready change that starts failing,
 * changes its reason or stops failing, and drops lines older than 30 days. A
 * file that cannot be written records nothing, and the debug log says why.
 */
async function recordFailing($: EngineInterface, found: Ready) {
  try {
    const path = await statePath($)
    const now = await $.clock.now()
    const kept = keptLines(await $.fs.read(path).catch(() => ''), now)
    const added = transitions(kept, found.changes, new Date(now).toISOString(), await $.session.id())
    if (added.length > 0) await $.fs.write(path, stateText(kept, added))
  } catch (error) {
    $.ui.log(`board: the ready-failing state file was not written: ${String(error)}`, { to: 'debug' })
  }
}

/**
 * Reads the changes ready to land. A board with no changes plugin, or one
 * without `ready`, says so once and is not asked again while the module is
 * loaded. A failure keeps the last count. Says whether the read worked.
 */
async function refreshReady($: EngineInterface, options: PluginOptions, local: { readyAbsent: boolean }): Promise<boolean> {
  if (local.readyAbsent) return true
  let why: string | null
  try {
    const ran = await board($, options, ['call', 'changes.ready'], READY_TIMEOUT_MS)
    const read = parseReady(ran.exitCode, ran.stdout, ran.stderr)
    if (read.kind === 'absent') {
      local.readyAbsent = true
      await update($, ready, () => null)
    }
    if (read.kind === 'read') {
      await update($, ready, () => read.ready)
      await recordFailing($, read.ready)
    }
    why = read.kind === 'failed' ? read.why : null
  } catch (error) {
    why = `changes.ready did not run: ${String(error)}`
  }
  if (why !== null) $.ui.log(`board: changes.ready failed: ${why}`, { to: 'debug' })
  await update($, readyFailed, () => why)
  return why === null
}

/** Reads the open questions again, and says whether the read worked. A failure is logged, kept for the band and keeps the last count read. */
async function refreshOpen($: EngineInterface, options: PluginOptions): Promise<boolean> {
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
  return found !== null
}

/**
 * What the mod keeps between reads: whether plans.waiting or changes.ready is
 * absent, whether a changes.ready read runs, its schedule, and the board's
 * stamp the last read that worked saw, one for the questions and one for the
 * changes ready to land.
 */
type Local = { absent: boolean; readyAbsent: boolean; readying: boolean; schedule: Schedule; seen: Seen; readySeen: Seen }

/**
 * Starts a read of the changes ready to land without waiting for it, so no
 * refresh, session start or answer waits on it. While one runs, another is
 * not started. It runs no `board` when the board's stamp is the one the last
 * read of changes.ready that worked saw; `always` reads regardless, as a
 * session start and an answer do.
 */
function startReady($: EngineInterface, options: PluginOptions, local: Local, always = false) {
  if (local.readying || local.readyAbsent) return
  local.readying = true
  void guarded($, 'changes.ready', async () => {
    await whenChanged($, configured(options), local.readySeen, () => refreshReady($, options, local), always)
  }, () => {
    local.readying = false
  })
}

/** Runs `work`, logs what it throws, so a read launched without a wait rejects nothing, and runs `done` however it ends. */
async function guarded($: EngineInterface, what: string, work: () => Promise<void>, done: () => void) {
  try {
    await work()
  } catch (error) {
    $.ui.log(`board: ${what} did not run: ${String(error)}`, { to: 'debug' })
  } finally {
    done()
  }
}

const totalOf = (counted: Counted) => (counted.kind === 'read' ? counted.total : null)

/** Reads the open questions, whose read the schedule marks begun, marks it ended beside the count `total`, and says whether it worked. */
async function list($: EngineInterface, options: PluginOptions, s: Schedule, total: number | null): Promise<boolean> {
  let worked = false
  await guarded($, 'plans.open', async () => {
    worked = await refreshOpen($, options)
  }, () => end(s, 'list', { worked, total }))
  return worked
}

/**
 * Reads the open questions and what waits on the owner, side by side, as a
 * session starts and after an answer. It reads whatever the board's stamp
 * says, and records the stamp when both reads work, so the next minute's
 * tick on an unchanged board runs no `board`.
 */
async function refreshBoth($: EngineInterface, options: PluginOptions, local: Local) {
  const s = local.schedule
  const now = await $.clock.now()
  begin(s, 'count', now)
  begin(s, 'list', now)
  let worked = false
  let counted: Counted = { kind: 'failed' }
  await guarded($, 'a refresh', async () => {
    startReady($, options, local, true)
    await whenChanged($, configured(options), local.seen, async () => {
      ;[worked, counted] = await Promise.all([refreshOpen($, options), refreshWaiting($, options, local)])
      return worked && counted.kind !== 'failed'
    }, true)
  }, () => {
    end(s, 'count')
    end(s, 'list', { worked, total: totalOf(counted) })
  })
}

/**
 * Reads what waits on the owner, whose read the schedule marks begun, and the
 * open questions only when the count calls for them. It runs no `board` when
 * the board's stamp is the one the last read that worked saw; the schedule
 * still counts the minute as read, so the next check is a minute away.
 */
async function refreshCount($: EngineInterface, options: PluginOptions, local: Local) {
  const s = local.schedule
  await guarded($, 'plans.waiting', async () => {
    startReady($, options, local)
    await whenChanged($, configured(options), local.seen, async () => {
      const counted = await refreshWaiting($, options, local)
      const now = await $.clock.now()
      if (!countCallsForList(s, counted, now)) return counted.kind !== 'failed'
      begin(s, 'list', now)
      return (await list($, options, s, totalOf(counted))) && counted.kind !== 'failed'
    })
  }, () => end(s, 'count'))
}

/**
 * Records the owner's answer through `plans.answer`, which refuses a question
 * that is no longer open or already has an answer at any id, and attaches the
 * answer to the change the question blocks. Then it tries to send the answer
 * to the session that asked and to each session that asked the same question.
 * A question a person asked without a session is sent only to those other
 * sessions, and the toast names them.
 *
 * Only Board's own failure report says nothing was written; a run that timed
 * out or was killed may have recorded the answer, so nothing is sent then.
 *
 * Once the answer is recorded, the send is best effort. It
 * names each asking session's `local_` id as a string, the SendMessage tool's
 * spelling of a recipient, because `{ sessionId }` refuses a `local_` id.
 */
async function answer($: EngineInterface, options: PluginOptions, local: Local, q: Question, choice: string | undefined, words: string) {
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
    // A person who asked without a Claude Code session has no session to send to.
    const asker = q.session === undefined ? Promise.resolve(null) : send(q.session)
    const [sent] = await Promise.all([asker, ...others.map(send)])
    const { attached } = answered
    const unattached = attached?.refused === undefined ? null : `not attached to ${attached.change}: ${attached.refused}`
    const approval = await askApproval($, options, approvedChange(q, choice, attached))
    $.ui.toast(
      (sent === null
        ? `Answered and recorded in Board${unattached === null ? '' : `, but ${unattached}`}; ${q.person ?? 'the asker'} asked without a Claude Code session, so ${others.length === 0 ? 'nothing was sent' : `it was sent to ${listed(others)}, which asked the same`}${approval === '' ? '.' : ''}`
        : sent
          ? `Answered, and sent to ${q.session}${unattached === null ? '' : `. The answer is recorded but was ${unattached}`}`
          : `Answered and recorded in Board${unattached === null ? '' : `, but ${unattached}`}. The message to ${q.session} was not delivered`) + approval,
    )
    // The answer changes what waits, so the count and the list are read again.
    await refreshBoth($, options, local)
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
  if (ran?.exitCode === 0) return approvalText(change, parseApproved(ran.stdout))
  const why = ran === null ? 'board did not finish' : failure(ran.exitCode, ran.stdout, ran.stderr)
  return `. No approval of ${change} was asked (${why}); run board call changes.approve --path ${change}`
}

/** `items` joined as a sentence names them, such as `a, b and c`. */
const listed = (items: string[]) => (items.length < 2 ? items.join('') : `${items.slice(0, -1).join(', ')} and ${items[items.length - 1]}`)

const age = (asked: string, now: number) => {
  const hours = Math.floor((now - Date.parse(asked)) / 3_600_000)
  return hours < 1 ? 'new' : hours < 48 ? `${hours}h` : `${Math.floor(hours / 24)}d`
}

export const register: Register = (on, options) => {
  const local: Local = { absent: false, readyAbsent: false, readying: false, schedule: schedule(), seen: unseen(), readySeen: unseen() }
  const mode = failingMode(options.failing)

  // The minute's refresh starts with the first band drawn, which a `-p` run
  // never draws. Desktop sessions report `isInteractive: false`, so that flag
  // cannot gate it.
  let polling = false

  on('session.start', async ($, e, next) => {
    const started = await next(e)
    await $.command.register({ name: 'yesterday', description: "Show the morning report of yesterday's Board records, from 07:00" })
    await refreshBoth($, options, local)
    return started
  })

  // The host asks each plugin for its own row; this mod draws into every Board mod's.
  keepOneBand(on)
  on('ui.render', { component: 'AbovePrompt', plugin: '*' }, async ($, e, next) => {
    const below = await next(e)
    if (!polling) {
      polling = true
      // A tick reads only when the band was drawn since the last read, so a session nobody draws runs no board.
      $.clock.every(REFRESH_MS, async () => {
        const now = await $.clock.now()
        if (!tickIsDue(local.schedule, now)) return
        begin(local.schedule, 'count', now)
        await refreshCount($, options, local)
      })
    }
    // Drawing may not write state, so a read the draw starts runs once the draw is done. It is
    // marked begun now, because the host may draw again first; a callback that never runs leaves
    // a read that counts as lost after LOST_MS, so it cannot stop later reads.
    const now = await $.clock.now()
    local.schedule.drawn = true
    if (isDue(local.schedule.count, now)) {
      begin(local.schedule, 'count', now)
      $.clock.after(0, () => void refreshCount($, options, local))
    }
    if (e.props.hasSurvey || !drawsInto(e.plugin)) return below
    // plans.waiting counts the open questions; when it has not answered, the button counts plans.open's.
    // Waiting leaves out the questions Ready to land counts, so a check is counted once.
    const landed = await read($, ready)
    const listed = await read($, open)
    const badge = countBadge(waitingLess(await read($, waiting), landed, listed, mode), listed, await read($, failed))
    const landing = readyBadge(landed, await read($, readyFailed), mode)
    const { Box, Button, Text } = $.ui.resolve(e)
    const parts: RenderElement[] = []
    if (landing !== null) {
      parts.push(
        (
          <Box key={SLOT.ready}>
            <Button key="ready" label={landing.label} onPress={() => void $.ui.open({ id: READY, title: 'Ready to land' })}>
              <Text color={landing.problem ? 'red' : 'success'}>{landing.label}</Text>
              {landing.failing === null ? null : <Text color="red"> {landing.failing}</Text>}
            </Button>
          </Box>
        ) as RenderElement,
      )
    }
    if (badge !== null) {
      parts.push(
        (
          <Box key={SLOT.waiting}>
            <Button key="waiting" label={badge.label} onPress={() => void $.ui.open({ id: QUESTIONS, title: 'Open questions' })}>
              <Text color={badge.problem ? 'red' : 'warning'}>{badge.label}</Text>
            </Button>
          </Box>
        ) as RenderElement,
      )
    }
    return joinBand(below, parts)
  })

  on('ui.render', { component: 'Pane', requestId: READY }, async ($, e) => {
    const found = await read($, ready)
    const unread = await read($, readyFailed)
    const { Box, Button, Text } = $.ui.resolve(e)
    // /ask is the owner's skill, so it runs as if he typed it; a session without it says so.
    const ask = () => void $.command.run({ command: 'ask' }).catch(error => $.ui.toast(`/ask did not run: ${String(error)}`))
    const problem =
      unread === null ? null : (
        <Text color="red">{plain(`The changes ready to land could not be read: ${unread}.${found === null ? '' : ' This list is from the last read that worked.'}`)}</Text>
      )
    if (found === null || found.changes.length === 0) return problem ?? <Text dimColor>No change is ready to land.</Text>
    const shown = mode === 'shown'
    const failing = shown ? found.changes.filter(c => c.failing !== null) : []
    const red = failingPart(failing)
    const more = found.total - found.changes.length
    return (
      <Box flexDirection="column">
        {problem}
        <Box flexDirection="row" flexWrap="wrap" justifyContent="space-between" columnGap={1} marginBottom={1}>
          <Box flexDirection="row" columnGap={1}>
            <Text weight="semibold">Ready to land · {found.total - failing.length}</Text>
            {red === null ? null : <Text color="red">{red}</Text>}
          </Box>
          <Button key="ask-all" label="Ask these now (/ask)" variant="primary" onPress={ask} />
        </Box>
        {found.changes.map(c => {
          const why = shown ? c.failing : null
          const pull = pullRef(c.pull)
          const age = readyFor(c.days)
          return (
            <Box key={c.path} flexDirection="column" marginBottom={1}>
              <Box flexDirection="row" flexWrap="wrap" columnGap={2}>
                {pull === null ? null : <Text key="pull" dimColor>{pull}</Text>}
                {age === null ? null : <Text key="age" dimColor>{age}</Text>}
                <Text key="kind" inverse> {kindLabel(c.kind)} </Text>
                {why === null ? null : <Text key="failing" color="red" inverse> {failingLabel(why)} </Text>}
              </Box>
              <Text weight="semibold">{c.title}</Text>
              <Text>Waits on: {plain(why === null ? c.waits_on : waitsOnFailing(c.waits_on, why))}</Text>
              {why === null ? (
                <Box flexDirection="row" columnGap={1}>
                  <Text dimColor>Clears it:</Text>
                  <Button key={`${c.path}#ask`} label="Answer in /ask" onPress={ask} />
                </Box>
              ) : (
                <Box flexDirection="row" columnGap={1}>
                  <Text dimColor>To see why:</Text>
                  <Text>board changes next --path {c.path}</Text>
                </Box>
              )}
            </Box>
          )
        })}
        {more > 0 ? <Text dimColor>{more} more are ready; `board changes ready` lists them.</Text> : null}
      </Box>
    )
  })

  on('ui.render', { component: 'Pane', requestId: QUESTIONS }, async ($, e) => {
    // The pane's read stands beside the count last read, so an unchanged count does not read the list again.
    const shown = (await read($, waiting))?.total ?? null
    const drawn = await $.clock.now()
    if (isDue(local.schedule.list, drawn)) {
      begin(local.schedule, 'list', drawn)
      $.clock.after(0, () => void list($, options, local.schedule, shown))
    }
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
                  onPress={() => answer($, options, local, q, o.id, o.text)}
                />
              ))}
            </Box>
            {Input === undefined ? null : (
              <Input key={`${q.path}#words`} placeholder="Answer in your own words" submitLabel="answer" onSubmit={words => {
                if (words.trim() !== '') void answer($, options, local, q, undefined, words)
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
