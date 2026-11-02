// The brief line and its parser are the changes write guard's, copied from
// plugins/changes/claude-code/hooks/guard.ts, since a mod cannot import from
// outside its own folder. A test of the artifacts crate checks that the two
// copies stay the same, so the guard and this mod agree on which change a
// builder works on.

const BRIEF = /^board-brief:\s*(\/changes\/\S+)$/

/** The change the first line of `text` names as `board-brief: /changes/<name>`, or null. */
export function briefOf(text: string): string | null {
  const first = text.split('\n').map(line => line.trim()).find(line => line !== '')
  return first?.match(BRIEF)?.[1] ?? null
}

// `$.session.messages` returns at most this many entries, the newest.
export const WINDOW = 4096

/** What the agent's first message says: the change its brief names, or none, and whether the message could be read at all. */
export type Brief = { change: string } | { change: null; unread: boolean }

/** The task a Bash command claims with tasks.claim, and the board directory its `-C` names, if any. */
export type Claim = { task: string; board?: string }

/**
 * The task a Bash command claims: the absolute `--path` that follows
 * `tasks.claim` in the same command. A claim piped onward or followed by
 * `||` is left out, since Bash then reports success even when Board refused
 * it. Soft limit: this reads the command's words, not a parse of the shell;
 * a claim inside a subshell, a variable or a script is not seen.
 */
export function claimOf(command: string): Claim | undefined {
  const parts = command.split(/(\|\||&&|[|;\n])/)
  for (let at = 0; at < parts.length; at += 2) {
    const part = parts[at]!
    const claim = part.search(/\btasks\.claim\b/)
    if (claim === -1) continue
    const next = parts[at + 1]
    if (next === '|' || next === '||') return undefined
    const task = /--path[= ]['"]?(\/[^\s'"]+)/.exec(part.slice(claim))?.[1]
    if (task === undefined) return undefined
    const board = /(?:^|\s)-C[= ]?['"]?([^\s'"]+)/.exec(part.slice(0, claim))?.[1]
    return board === undefined ? { task } : { task, board }
  }
  return undefined
}

/** `path` with a leading `~` or `$HOME` spelled out and no trailing slash. */
export function expanded(path: string, home: string | undefined): string {
  const spelled = home === undefined ? path : path.replace(/^(~|\$HOME|\$\{HOME\})(?=\/|$)/, home)
  return spelled.length > 1 ? spelled.replace(/\/+$/, '') : spelled
}

/** The directory `personal` names in the board config's text, or undefined when the text names none. */
export function personalOf(config: string): string | undefined {
  try {
    const personal = (JSON.parse(config) as { personal?: unknown }).personal
    return typeof personal === 'string' ? personal : undefined
  } catch {
    return undefined
  }
}
