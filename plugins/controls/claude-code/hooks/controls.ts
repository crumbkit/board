// What the controls capabilities answered, and the words a session and the
// owner read from them. Pure functions, so the tests can pin every word.

import type { Checked, Control } from '../types'

/** The name of the context block the session's first message carries. */
export const BLOCK = 'boardControls'

/** Board's code for a directory that no known board holds. */
const NO_BOARD = 'board::config::not_found'

type Report = Record<string, unknown>

function parseJson(text: string): Report | null {
  try {
    const value: unknown = JSON.parse(text)
    return typeof value === 'object' && value !== null ? (value as Report) : null
  } catch {
    return null
  }
}

function output(stdout: string): Report | null {
  const report = parseJson(stdout)
  if (report?.schema !== 'board/call-report/v2') return null
  const data = report.data as { output?: Report } | undefined
  return data?.output ?? null
}

function failure(exitCode: number, stdout: string, stderr: string): { code: string; message: string } {
  const report = parseJson(stdout) ?? parseJson(stderr)
  if (typeof report?.code === 'string' && typeof report.message === 'string') {
    return { code: report.code, message: report.message }
  }
  const text = (stderr.trim() || stdout.trim()).slice(0, 2000)
  return { code: `exit ${exitCode}`, message: text || 'board printed nothing' }
}

type Stated = { control: string; requirement: string }
type Measured = Stated & { budget: string; used: number; max: number }

/**
 * Reads `board --json call controls.check --path .` run at `root`. `null`
 * when no board holds the repository, which is no failure: it has no controls.
 */
export function parseCheck(root: string, exitCode: number, stdout: string, stderr: string): Checked | null {
  const found = exitCode === 0 ? output(stdout) : null
  if (found !== null && Array.isArray(found.stated) && Array.isArray(found.measured)) {
    const measured = (found.measured as Measured[]).map(m => ({
      control: m.control,
      requirement: m.requirement,
      usage: `${m.used} of ${m.max} ${m.budget}`,
    }))
    const stated = (found.stated as Stated[]).map(s => ({ control: s.control, requirement: s.requirement }))
    return { kind: 'read', root, controls: [...measured, ...stated] }
  }
  const failed = failure(exitCode, stdout, stderr)
  if (failed.code === NO_BOARD) return null
  return { kind: 'failed', root, ...failed }
}

/** Reads `board --json call controls.for-path`; `null` when no board holds the file. */
export function parseForPath(exitCode: number, stdout: string): Control[] | null {
  const found = exitCode === 0 ? output(stdout) : null
  if (found === null || !Array.isArray(found.controls)) return null
  return (found.controls as Stated[]).map(c => ({ control: c.control, requirement: c.requirement }))
}

/** The context block's text, or `null` when there is nothing to say. */
export function contextText(checked: Checked): string | null {
  if (checked.kind === 'failed') {
    return (
      `\`board call controls.check\` failed for ${checked.root}, so the controls on its files are unknown:\n\n` +
      `${checked.code}: ${checked.message}\n\n` +
      'Before you edit a file, run `board call controls.for-path --path <file>` in its directory.'
    )
  }
  if (checked.controls.length === 0) return null
  const lines = checked.controls.map(c => `- ${c.control}: ${c.requirement}${c.usage ? ` (now ${c.usage})` : ''}`)
  return (
    `The board's controls state requirements on files in ${checked.root}. Follow each one that applies to a ` +
    `file you change. \`board get <control>\` shows which files a control names.\n\n${lines.join('\n')}`
  )
}

/** The one line a band or the status line shows. */
export function summary(checked: Checked): string {
  const name = checked.root.split('/').filter(Boolean).at(-1) ?? checked.root
  if (checked.kind === 'failed') return `controls on ${name} could not be read: ${checked.code}`
  const n = checked.controls.length
  return n === 1 ? `1 control on ${name}` : `${n} controls on ${name}`
}

/** The hidden row that names controls on `file` the conversation was not told about. */
export function reminder(file: string, controls: readonly Control[]): string {
  const lines = controls.map(c => `- ${c.control}: ${c.requirement}`)
  const these = controls.length === 1 ? 'This board control applies' : 'These board controls apply'
  return `${these} to ${file}, which you are editing now. Follow each one in this edit and any that follows.\n\n${lines.join('\n')}`
}
