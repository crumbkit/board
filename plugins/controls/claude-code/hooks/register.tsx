import { atom, read, update } from 'claude-code'
import type { EngineInterface, Register, RenderElement } from 'claude-code'

import type { Checked, Control, StateBadge } from '../types'
import { SLOT, joinBand } from './band'
import {
  BLOCK,
  REMINDER_NOTE,
  badgeValue,
  boardName,
  contextText,
  controlsBadge,
  paneTitle,
  parseCheck,
  parseConfig,
  parseForPath,
  reminder,
  sections,
  stateBadges,
} from './controls'

// The mod reads the controls plugin and writes nothing to the board. It shares
// nothing with other mods but the band, which each mod draws a part of: the
// session's state badges and the Controls button.

const checked = atom({ plugin: 'board-controls', key: 'checked' } as const, null as Checked | null)
const told = atom({ plugin: 'board-controls', key: 'told' } as const, {} as Record<string, string[]>)
const state = atom({ plugin: 'board-controls', key: 'state' } as const, [] as StateBadge[])

const PATH = '"$HOME/.local/bin:/opt/homebrew/bin:/usr/local/bin:$PATH"'
const CHECK = `PATH=${PATH} exec board --json call controls.check --path .`
const FOR_PATH = `PATH=${PATH} exec board --json call controls.for-path --path "$1"`
// The board that holds the directory names its repository in the listing's summary.
const WHERE = `PATH=${PATH} exec board --json ls / --limit 1 < /dev/null`
const PANE = 'board-controls'

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

/** The known boards file, which names the configured badges and the personal board. */
async function config($: EngineInterface) {
  const xdg = await $.env.get('XDG_CONFIG_HOME')
  const base = xdg !== undefined && xdg !== '' ? xdg : `${(await $.env.get('HOME')) ?? ''}/.config`
  try {
    return parseConfig(await $.fs.read(`${base}/board/config.json`))
  } catch {
    return parseConfig('{}')
  }
}

/** Runs `argv` in `cwd` and answers its badge value; null when it fails, times out or prints nothing. */
async function value($: EngineInterface, argv: string[], cwd: string, timeoutMs: number): Promise<string | null> {
  try {
    const ran = await $.process.run(argv, { cwd, timeoutMs })
    return badgeValue(ran.exitCode, ran.stdout)
  } catch {
    return null
  }
}

/** Reads the state badges once: each configured command in the session's directory, then the board's name. */
async function readState($: EngineInterface, cwd: string, root: string) {
  const { badges, personal } = await config($)
  const [configured, where] = await Promise.all([
    Promise.all(badges.map(async b => ({ label: b.label, value: await value($, b.argv, cwd, BADGE_MS) }))),
    $.process.run(['/bin/sh', '-c', WHERE], { cwd: root, timeoutMs: TIMEOUT_MS }).catch(() => null),
  ])
  const shown = configured.filter((b): b is StateBadge => b.value !== null)
  const board = where === null ? null : boardName(where.exitCode, where.stdout, personal)
  await update($, state, () => stateBadges(shown, board))
}

/** The controls the context block names. */
const toldBy = (answer: Checked | null) => (answer?.kind === 'read' ? answer.controls.map(c => c.control) : [])

/** Runs controls.check at the repository root and keeps the answer for the session. */
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

// A file's controls depend on its own path, since a pattern such as *.rs
// differs between files of one directory, so the cache is per file. It starts
// over when the module reloads.
const forPath = new Map<string, Control[] | null>()

/** Appends a hidden row naming each control on `file` the conversation was not told about. */
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
  const fresh = (forPath.get(file) ?? []).filter(c => !known.has(c.control))
  if (fresh.length === 0) return
  await $.session.append({
    message: { type: 'user', content: [{ type: 'text', text: reminder(file, fresh) }] },
    ...(agentId === undefined ? {} : { agentId }),
  })
  await update($, told, all => ({ ...all, [key]: [...(all[key] ?? []), ...fresh.map(c => c.control)] }))
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
  // compaction or /clear. The rows that named other controls are gone then, so
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
    const button = answer === null ? null : controlsBadge(answer)
    const { Box, Button, Text } = $.ui.resolve(e)
    const parts: RenderElement[] = []
    if (badges.length > 0) {
      parts.push(
        (
          <Box key={SLOT.state} flexDirection="row" flexWrap="wrap" columnGap={1}>
            {badges.map(b => [<Text dimColor>{b.label}</Text>, <Text bold>{b.value}</Text>])}
          </Box>
        ) as RenderElement,
      )
    }
    if (button !== null && answer !== null) {
      parts.push(
        (
          <Box key={SLOT.controls}>
            <Button key="controls" label={button.label} onPress={() => void $.ui.open({ id: PANE, title: paneTitle(answer) })}>
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
    if (answer === null) return <Text dimColor>No board holds this session's repository, so it has no controls.</Text>
    if (answer.kind === 'failed') {
      return (
        <Box flexDirection="column">
          <Text color="red">board call controls.check failed for {answer.root}, so its controls are unknown.</Text>
          <Text>
            {answer.code}: {answer.message}
          </Text>
        </Box>
      )
    }
    return (
      <Box flexDirection="column">
        <Text bold>{paneTitle(answer)}</Text>
        {sections(answer.controls).map(section => (
          <Box key={section.heading} flexDirection="column" marginTop={1}>
            <Text dimColor bold>
              {section.heading}
            </Text>
            {section.controls.length === 0 ? (
              <Text dimColor>None.</Text>
            ) : (
              section.controls.map(c => (
                <Text>
                  <Text color="suggestion">{c.control}</Text>: {c.requirement}
                  {c.usage ? ` (now ${c.usage})` : ''}
                </Text>
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
