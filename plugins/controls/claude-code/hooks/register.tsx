import { atom, read, update } from 'claude-code'
import type { EngineInterface, Register } from 'claude-code'

import type { Checked, Control } from '../types'
import { BLOCK, contextText, parseCheck, parseForPath, reminder, summary } from './controls'

// The mod reads the controls plugin and writes nothing to the board. It shares
// nothing with other mods but the band, which each mod draws a part of.

const checked = atom({ plugin: 'board-controls', key: 'checked' } as const, null as Checked | null)
const told = atom({ plugin: 'board-controls', key: 'told' } as const, {} as Record<string, string[]>)

const PATH = '"$HOME/.local/bin:/opt/homebrew/bin:/usr/local/bin:$PATH"'
// Without a path, the personal board checks the repository the session is in,
// with that repository's own controls when it is a board itself.
const CHECK = `PATH=${PATH} exec board --personal --json call controls.check`
const FOR_PATH = `PATH=${PATH} exec board --json call controls.for-path --path "$1"`
// The tool the attention mod registers. While it is listed, the controls line
// joins the band that mod draws; otherwise it goes to the status line.
const ATTENTION = 'mcp__attention-approve__request_signature'

const hasAttention = async ($: EngineInterface) => (await $.tool.list()).some(t => t.name === ATTENTION)

// A slow or hung board holds the session start or the edit no longer than this.
const TIMEOUT_MS = 5000

/** The repository the session starts in, or the directory itself outside one. */
async function repositoryRoot($: EngineInterface, cwd: string): Promise<string> {
  try {
    const ran = await $.process.run(['git', 'rev-parse', '--show-toplevel'], { cwd })
    return ran.exitCode === 0 && ran.stdout.trim() !== '' ? ran.stdout.trim() : cwd
  } catch {
    return cwd
  }
}

/** The controls the context block names. */
const toldBy = (answer: Checked | null) => (answer?.kind === 'read' ? answer.controls.map(c => c.control) : [])

/** Runs the personal board's controls.check at the repository root and keeps the answer for the session. */
async function check($: EngineInterface, cwd: string): Promise<Checked | null> {
  const root = await repositoryRoot($, cwd)
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
  on('session.start', async ($, e, next) => {
    const started = await next(e)
    const answer = await check($, e.cwd)
    if (answer !== null && !(await hasAttention($))) $.ui.status(`board: ${summary(answer)}`)
    return started
  })

  // The attention mod may register its tool after this mod's session.start ran.
  on('tool.register', async ($, e, next) => {
    const registered = await next(e)
    if ('value' in registered && registered.value?.tool === ATTENTION) $.ui.status(undefined)
    return registered
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
    const answer = await read($, checked)
    if (e.props.hasSurvey || answer === null || !(await hasAttention($))) return below
    const { Box, Text } = $.ui.resolve(e)
    return (
      <Box flexDirection="column">
        {below.type === 'engine' ? null : below}
        <Text color={answer.kind === 'failed' ? 'red' : undefined} dimColor={answer.kind === 'read'}>
          {summary(answer)}
        </Text>
      </Box>
    )
  })
}
