import { atom, read, update } from 'claude-code'
import type { EngineInterface, Register, RenderElement } from 'claude-code'

import type { Checked, Rule, StateBadge } from '../types'
import type { Configured } from './rules'
import { SLOT, joinBand } from './band'
import {
  BLOCK,
  REMINDER_NOTE,
  badgeValue,
  boardName,
  contextText,
  rulesBadge,
  paneTitle,
  parseCheck,
  badgesProblem,
  parseBadges,
  parsePersonal,
  parseForPath,
  reminder,
  sections,
  stateBadges,
} from './rules'

// The mod reads the rules plugin and writes nothing to the board. It shares
// nothing with other mods but the band, which each mod draws a part of: the
// session's state badges and the Rules button.

const checked = atom({ plugin: 'board-rules', key: 'checked' } as const, null as Checked | null)
const told = atom({ plugin: 'board-rules', key: 'told' } as const, {} as Record<string, string[]>)
const state = atom({ plugin: 'board-rules', key: 'state' } as const, [] as StateBadge[])

const PATH = '"$HOME/.local/bin:/opt/homebrew/bin:/usr/local/bin:$PATH"'
const CHECK = `PATH=${PATH} exec board --json call rules.check --path .`
const FOR_PATH = `PATH=${PATH} exec board --json call rules.for-path --path "$1"`
// The board that holds the directory names its repository in the listing's summary.
const WHERE = `PATH=${PATH} exec board --json ls / --limit 1 < /dev/null`
// A configured badge's command runs with the PATH the board calls use, each
// value of its argv its own argument through "$@".
const BADGE = `PATH=${PATH} exec "$@" < /dev/null`
const PANE = 'board-rules'

// A slow or hung board holds the session start or the edit no longer than this.
const TIMEOUT_MS = 5000
// A configured badge's command gets this long, once, as the session starts.
const BADGE_MS = 2000

/** The repository the session starts in, or the directory itself outside one. */
async function repositoryRoot($: EngineInterface, cwd: string): Promise<string> {
  try {
    const ran = await $.process.run(['git', 'rev-parse', '--show-toplevel'], { cwd })
    return ran.exitCode === 0 && ran.stdout.trim() !== '' ? ran.stdout.trim() : cwd
  } catch {
    return cwd
  }
}

/** Board's configuration directory: `$XDG_CONFIG_HOME/board`, or `~/.config/board`. */
async function configDir($: EngineInterface) {
  const xdg = await $.env.get('XDG_CONFIG_HOME')
  return `${xdg !== undefined && xdg !== '' ? xdg : `${(await $.env.get('HOME')) ?? ''}/.config`}/board`
}

/** The personal board's directory, from the known boards file; null when it names none. */
async function personalBoard($: EngineInterface, dir: string) {
  try {
    return parsePersonal(await $.fs.read(`${dir}/config.json`))
  } catch {
    return null
  }
}

/** The configured badges, or the red badge saying what is wrong with the file. A missing file configures none. */
async function configured($: EngineInterface, dir: string): Promise<Configured[] | StateBadge> {
  const file = `${dir}/badges.json`
  let text: string
  try {
    if (!(await $.fs.exists(file))) return []
    text = await $.fs.read(file)
  } catch (error) {
    return badgesProblem(file, `could not be read: ${String(error)}`)
  }
  const parsed = parseBadges(text)
  return 'problem' in parsed ? badgesProblem(file, parsed.problem) : parsed.badges
}

/** Runs `argv` in `cwd` and answers its badge value; null, logged, when it fails, times out or prints nothing. */
async function value($: EngineInterface, label: string, argv: string[], cwd: string, timeoutMs: number): Promise<string | null> {
  let read: { value: string } | { why: string }
  try {
    const ran = await $.process.run(['/bin/sh', '-c', BADGE, 'badge', ...argv], { cwd, timeoutMs })
    read = badgeValue(ran.exitCode, ran.stdout)
  } catch (error) {
    read = { why: `did not finish within ${timeoutMs} ms: ${String(error)}` }
  }
  if ('value' in read) return read.value
  $.ui.log(`board: the ${label} badge is hidden, because its command ${read.why}`, { to: 'debug' })
  return null
}

/** Reads the state badges once: each configured command in the session's directory, then the board's name. */
async function readState($: EngineInterface, cwd: string, root: string) {
  const dir = await configDir($)
  const [listed, personal] = await Promise.all([configured($, dir), personalBoard($, dir)])
  const badges = Array.isArray(listed) ? listed : []
  const [values, where] = await Promise.all([
    Promise.all(badges.map(async b => ({ label: b.label, value: await value($, b.label, b.argv, cwd, BADGE_MS) }))),
    $.process.run(['/bin/sh', '-c', WHERE], { cwd: root, timeoutMs: TIMEOUT_MS }).catch(() => null),
  ])
  const shown: StateBadge[] = Array.isArray(listed)
    ? values.flatMap(b => (b.value === null ? [] : [{ label: b.label, value: b.value, problem: null }]))
    : [listed]
  const board = where === null ? null : boardName(where.exitCode, where.stdout, personal)
  await update($, state, () => stateBadges(shown, board))
}

/** The rules the context block names. */
const toldBy = (answer: Checked | null) => (answer?.kind === 'read' ? answer.rules.map(c => c.rule) : [])

/** Runs rules.check at the repository root and keeps the answer for the session. */
async function check($: EngineInterface, root: string): Promise<Checked | null> {
  let answer: Checked | null
  try {
    const ran = await $.process.run(['/bin/sh', '-c', CHECK], { cwd: root, timeoutMs: TIMEOUT_MS })
    answer = parseCheck(root, ran.exitCode, ran.stdout, ran.stderr)
  } catch (error) {
    answer = { kind: 'failed', root, code: 'not run', message: String(error) }
  }
  await update($, checked, () => answer)
  await update($, told, all => ({ ...all, '': toldBy(answer) }))
  return answer
}

// A file's rules depend on its own path, since a pattern such as *.rs
// differs between files of one directory, so the cache is per file. It starts
// over when the module reloads.
const forPath = new Map<string, Rule[] | null>()

/** Appends a hidden row naming each rule on `file` the conversation was not told about. */
// The path goes to board relative to the root, so a file in a directory that
// does not exist yet is looked up too. A file outside the root, or a session
// in a repository no board holds, runs nothing.
async function beforeEdit($: EngineInterface, file: string, agentId: string | undefined) {
  const root = (await read($, checked))?.root
  if (root === undefined || !file.startsWith(`${root}/`)) return
  if (!forPath.has(file)) {
    const relative = `./${file.slice(root.length + 1)}`
    const ran = await $.process.run(['/bin/sh', '-c', FOR_PATH, 'for-path', relative], { cwd: root, timeoutMs: TIMEOUT_MS })
    forPath.set(file, parseForPath(ran.exitCode, ran.stdout))
  }
  const key = agentId ?? ''
  const known = new Set((await read($, told))[key] ?? [])
  const fresh = (forPath.get(file) ?? []).filter(c => !known.has(c.rule))
  if (fresh.length === 0) return
  await $.session.append({
    message: { type: 'user', content: [{ type: 'text', text: reminder(file, fresh) }] },
    ...(agentId === undefined ? {} : { agentId }),
  })
  await update($, told, all => ({ ...all, [key]: [...(all[key] ?? []), ...fresh.map(c => c.rule)] }))
}

export const register: Register = on => {
  // The check and the state badges run side by side, so a badge command adds
  // no wait beyond the check's own.
  on('session.start', async ($, e, next) => {
    const started = await next(e)
    const root = await repositoryRoot($, e.cwd)
    await Promise.all([check($, root), readState($, e.cwd, root)])
    return started
  })

  // One block, which the engine sends in the first request and again after a
  // compaction or /clear. The rows that named other rules are gone then, so
  // the main conversation counts as told only what the block says.
  on('prompt.context', async ($, e, next) => {
    const context = await next(e)
    const answer = await read($, checked)
    await update($, told, all => ({ ...all, '': toldBy(answer) }))
    const text = answer === null ? null : contextText(answer)
    if (text === null) return context
    return { ...context, blocks: [...context.blocks.filter(b => b.name !== BLOCK), { name: BLOCK, text }] }
  })

  // A lookup that fails leaves the edit to run as it would without the mod.
  on('tool.call', { tool: 'Edit' }, async ($, e, next) => {
    await beforeEdit($, e.file_path, e.agentId)
    return next(e)
  }).catch(($, e, next) => next(e))
  on('tool.call', { tool: 'Write' }, async ($, e, next) => {
    await beforeEdit($, e.file_path, e.agentId)
    return next(e)
  }).catch(($, e, next) => next(e))

  on('ui.render', { component: 'AbovePrompt' }, async ($, e, next) => {
    const below = await next(e)
    if (e.props.hasSurvey) return below
    const answer = await read($, checked)
    const badges = await read($, state)
    const button = answer === null ? null : rulesBadge(answer)
    const { Box, Button, Text } = $.ui.resolve(e)
    const parts: RenderElement[] = []
    if (badges.length > 0) {
      parts.push(
        (
          <Box key={SLOT.state} flexDirection="row" flexWrap="wrap" columnGap={1}>
            {badges.map(b =>
              b.problem !== null ? (
                <Text color="red">{b.problem}</Text>
              ) : (
                <Box columnGap={1}>
                  <Text dimColor>{b.label}</Text>
                  <Text inverse> {b.value} </Text>
                </Box>
              ),
            )}
          </Box>
        ) as RenderElement,
      )
    }
    if (button !== null && answer !== null) {
      parts.push(
        (
          <Box key={SLOT.controls}>
            <Button key="rules" label={button.label} onPress={() => void $.ui.open({ id: PANE, title: paneTitle(answer) })}>
              {button.problem ? <Text color="red">{button.label}</Text> : button.label}
            </Button>
          </Box>
        ) as RenderElement,
      )
    }
    return joinBand(below, parts)
  })

  on('ui.render', { component: 'Pane', requestId: PANE }, async ($, e) => {
    const { Box, Text } = $.ui.resolve(e)
    const answer = await read($, checked)
    if (answer === null) return <Text dimColor>No board holds this session's repository, so it has no rules.</Text>
    if (answer.kind === 'failed') {
      return (
        <Box flexDirection="column">
          <Text color="red">board call rules.check failed for {answer.root}, so its rules are unknown.</Text>
          <Text>
            {answer.code}: {answer.message}
          </Text>
        </Box>
      )
    }
    return (
      <Box flexDirection="column">
        <Text bold>{paneTitle(answer)}</Text>
        {sections(answer.rules).map(section => (
          <Box key={section.heading} flexDirection="column" marginTop={1}>
            <Text dimColor bold>
              {section.heading}
            </Text>
            {section.rules.length === 0 ? (
              <Text dimColor>None.</Text>
            ) : (
              section.rules.map(c => (
                <Box key={c.rule} flexDirection="column">
                  <Text>
                    <Text color="suggestion">{c.rule}</Text>: {c.requirement}
                    {c.usage ? ` (now ${c.usage})` : ''}
                    {c.described ? ' (described)' : ''}
                  </Text>
                  {c.close ? <Text dimColor>{`  ${c.close}`}</Text> : null}
                </Box>
              ))
            )}
          </Box>
        ))}
        <Box marginTop={1}>
          <Text dimColor>{REMINDER_NOTE}</Text>
        </Box>
      </Box>
    )
  })
}
