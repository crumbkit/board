// What the write guard reads from git, the board config and `board`, and the
// words of each refusal. Pure functions, so the tests can pin every word.

/**
 * A mount in a known board: the board's directory, the mount's path, the
 * repository's directory (null when Board cannot read it) and the file of the
 * mount record, relative to the board.
 */
export type Mount = { board: string; mount: string; repo: string | null; record: string }

/** What `board get` answered for the change a brief names. */
export type Change =
  | { kind: 'found'; mount: string; phase: string; changeKind: string }
  | { kind: 'missing' }
  | { kind: 'failed'; why: string }

const BRIEF = /^board-brief:\s*(\/changes\/\S+)$/

/** The change the first line of `text` names as `board-brief: /changes/<name>`, or null. */
export function briefOf(text: string): string | null {
  const first = text.split('\n').map(line => line.trim()).find(line => line !== '')
  return first?.match(BRIEF)?.[1] ?? null
}

/** The repository's own directory from `git rev-parse --git-common-dir`, so a worktree names its repository. */
export function repositoryOf(commonDir: string): string {
  const dir = commonDir.trim().replace(/\/+$/, '')
  return dir.endsWith('/.git') ? dir.slice(0, -'/.git'.length) : dir
}

/**
 * The repository an origin URL names, as `host/owner/name` in lower case, or
 * null for an empty URL. ssh (`git@host:path`, `ssh://`) and http(s) forms of
 * one repository, with or without `.git`, give the same answer.
 */
export function originOf(url: string): string | null {
  const trimmed = url.trim()
  if (trimmed === '') return null
  const scp = trimmed.match(/^[^/@:]+@([^/:]+):(.*)$/)
  const parts = scp ?? trimmed.match(/^[a-z+]+:\/\/(?:[^@/]+@)?([^/:]+)(?::\d+)?\/(.*)$/i)
  const [host, path] = parts === null ? ['', trimmed] : [parts[1]!, parts[2]!]
  const name = path.replace(/\/+$/, '').replace(/\.git$/, '').replace(/^\/+/, '')
  return `${host}/${name}`.replace(/^\//, '').toLowerCase()
}

/**
 * The directory an origin URL names when it is a local path or a `file://`
 * URL, or null for a remote URL. A relative path is taken from `repo`, the
 * repository's own directory.
 */
export function localOrigin(url: string, repo: string): string | null {
  const trimmed = url.trim()
  const file = trimmed.match(/^file:\/\/[^/]*(\/.*)$/i)
  if (file !== null) return decodeURIComponent(file[1]!)
  if (trimmed.startsWith('/')) return trimmed
  if (trimmed.startsWith('./') || trimmed.startsWith('../')) return `${repo}/${trimmed}`
  return null
}

/** Each repository's origin from the ORIGINS script, `<repo>\t<url>` per line, by repository. */
export function parseOrigins(stdout: string): Map<string, string | null> {
  const origins = new Map<string, string | null>()
  for (const line of stdout.split('\n')) {
    const tab = line.indexOf('\t')
    if (tab > 0) origins.set(line.slice(0, tab), originOf(line.slice(tab + 1)))
  }
  return origins
}

function parseJson(text: string): unknown {
  try {
    return JSON.parse(text)
  } catch {
    return undefined
  }
}

/** The known boards' directories from the board config, or a reason it cannot be read. */
export function parseBoards(stdout: string): string[] | { why: string } {
  const config = parseJson(stdout) as { boards?: unknown } | undefined
  const boards = config?.boards
  if (Array.isArray(boards) && boards.every(b => typeof b === 'string')) return boards
  return { why: 'the board config is not JSON with a `boards` list of directories' }
}

/** The SQL that lists a board's own mounts with the directory of each repository. */
export const MOUNTS_SQL =
  'SELECT e.logical, CASE WHEN m.code IS NULL THEN s.repository END, m.path FROM mounts m ' +
  'LEFT JOIN sources s ON s.source = m.target JOIN entries e ON e.source = m.source AND e.path = m.path ' +
  "WHERE m.source = 'worktree'"

/** The mounts in a `board --json query` report of MOUNTS_SQL, or null when it is not one. */
export function parseMounts(board: string, stdout: string): Mount[] | null {
  const report = parseJson(stdout) as { schema?: unknown; data?: unknown } | undefined
  if (report?.schema !== 'board/query-report/v2' || !Array.isArray(report.data)) return null
  return report.data
    .filter(
      (row): row is [string, string | null, string] =>
        Array.isArray(row) &&
        typeof row[0] === 'string' &&
        (typeof row[1] === 'string' || row[1] === null) &&
        typeof row[2] === 'string',
    )
    .map(([mount, repo, record]) => ({ board, mount, repo: repo?.replace(/\/+$/, '') ?? null, record }))
}

/** The change in a `board --json get` report. */
export function parseChange(stdout: string): Change {
  const report = parseJson(stdout) as
    | {
        data?: { record?: { mount?: unknown; phase?: unknown; kind?: unknown } }[]
        failures?: { code?: unknown; message?: unknown }[]
      }
    | undefined
  const record = report?.data?.[0]?.record
  if (typeof record?.mount === 'string' && typeof record.phase === 'string') {
    const changeKind = typeof record.kind === 'string' ? record.kind : 'build'
    return { kind: 'found', mount: record.mount, phase: record.phase, changeKind }
  }
  const failure = report?.failures?.[0]
  if (failure?.code === 'board::path::not_found') return { kind: 'missing' }
  if (typeof failure?.code === 'string') return { kind: 'failed', why: `${failure.code}: ${String(failure.message)}` }
  return { kind: 'failed', why: 'it printed no change record' }
}

/** The refusal of an edit of the board config. */
export const configRecord = (file: string) =>
  `changes::guard::board_record: ${file} is the board config, which changes only outside an agent's Edit, ` +
  'Write and NotebookEdit. Ask the owner to change it.'

/** The refusal of an edit of a change record. */
export const changeRecord = (file: string, board: string) =>
  `changes::guard::board_record: ${file} is a change record of the board at ${board}, which changes only ` +
  `through board commands. Use \`board -C ${board} call changes.start\`, \`changes.attach\` or ` +
  '`changes.advance`, or `board put` to drop a change.'

/** The refusal of an edit of a mount record. */
export const mountRecord = (file: string, at: Mount) =>
  `changes::guard::board_record: ${file} is the mount record of ${at.mount} in the board at ${at.board}, ` +
  `which changes only through board commands. Use \`board -C ${at.board} mount update ${at.mount}\` to ` +
  'follow its repository, or `board put` to change the mount.'

/** Whether `relative`, a path inside a board, is a change record: `changes/<name>.json`. */
export const isChangeRecord = (relative: string) => /^changes\/[^/]+\.json$/.test(relative)

/** The refusal of an edit the guard cannot check. */
export const unknown = (file: string, why: string) =>
  `changes::guard::unknown: the guard could not tell whether this agent may edit ${file}: ${why}. The guard ` +
  'refuses every edit of a mounted repository it cannot check; fix what failed and try again.'

/** The refusal of an agent with no brief, or whose brief names a change no board that mounts the file holds. */
export function noBrief(file: string, at: Mount, named: string | null): string {
  const said =
    named === null
      ? "this agent's first message does not"
      : `this agent's first message names ${named}, which the board at ${at.board} does not hold`
  return (
    `changes::guard::no_brief: ${file} is in ${at.mount}, which the board at ${at.board} mounts. Only an agent ` +
    `whose first message starts with \`board-brief: /changes/<name>\` may edit it, and ${said}. Start a builder ` +
    `whose first message is the output of \`board -C ${at.board} call changes.brief --path /changes/<name>\`.`
  )
}

/** The only branch a prototype's builder may edit: `proto/<name>`. */
export const prototypeBranch = (name: string) => `proto/${name.replace(/\/+$/, '').split('/').pop()}`

/**
 * The refusal for a change, or null when its builder may edit `file`, which
 * is checked out on `branch` ('' when detached).
 */
export function judge(
  file: string,
  at: Mount,
  name: string,
  change: { mount: string; phase: string; changeKind: string },
  branch: string,
): string | null {
  if (change.phase === 'paused') {
    return (
      `changes::guard::not_building: ${name} is paused, and its builder may edit ${file} only in build. ` +
      `Run \`board -C ${at.board} call changes.resume --path ${name}\` when the cap allows.`
    )
  }
  if (change.phase !== 'build') {
    return (
      `changes::guard::not_building: ${name} is in phase ${change.phase}, and its builder may edit ${file} only ` +
      `in build. Run \`board -C ${at.board} call changes.advance --path ${name}\` once its plan gate passes.`
    )
  }
  if (change.mount !== at.mount) {
    return (
      `changes::guard::wrong_mount: ${name} is a change to ${change.mount}, and ${file} is in ${at.mount}. ` +
      `Start a builder from the brief of a change to ${at.mount}.`
    )
  }
  const only = prototypeBranch(name)
  if (change.changeKind === 'prototype' && branch !== only) {
    const on = branch === '' ? 'a detached HEAD' : `branch ${branch}`
    return (
      `changes::guard::wrong_branch: ${name} is a prototype, and its builder may edit ${file} only on branch ` +
      `${only}, but the file's checkout is on ${on}. Run \`git switch -c ${only}\` there, or \`git switch ${only}\` ` +
      'once it exists.'
    )
  }
  return null
}
