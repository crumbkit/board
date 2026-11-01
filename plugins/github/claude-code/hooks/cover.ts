// Whether a boundary in the personal board covers a write, read the way the
// boundary plugin matches one. Pure functions, so the tests can pin each case.

/** One boundary's `to`, `actions` and `from`, as the query reads them. */
export type Boundary = { to: string; actions: string[]; from: string[] | null }

/**
 * Every valid boundary in the personal board. A record that fails its schema
 * is left out: the boundary plugin refuses every check while one exists, so
 * a github action would not send either.
 */
export const BOUNDARIES_SQL =
  "SELECT json_extract(body, '$.to') AS dest, json_extract(body, '$.actions') AS actions, " +
  "json_extract(body, '$.from') AS origins FROM records WHERE type = 'boundary/boundary/v1' AND status = 'ok'"

type Report = Record<string, unknown>

export function parseJson(text: string): Report | null {
  try {
    const value: unknown = JSON.parse(text)
    return typeof value === 'object' && value !== null ? (value as Report) : null
  } catch {
    return null
  }
}

const strings = (text: unknown): string[] | null => {
  const value: unknown = typeof text === 'string' ? JSON.parse(text) : null
  return Array.isArray(value) && value.every(v => typeof v === 'string') ? (value as string[]) : null
}

/** Reads `board --json --personal query BOUNDARIES_SQL`; null when it printed no whole query report. */
export function parseBoundaries(stdout: string): Boundary[] | null {
  const report = parseJson(stdout)
  const columns = (report?.summary as { columns?: string[] } | undefined)?.columns
  if (report?.schema !== 'board/query-report/v2' || !Array.isArray(report.data) || columns === undefined) return null
  if (report.truncated === true) return null
  return (report.data as unknown[][]).flatMap(row => {
    const r = Object.fromEntries(columns.map((c, i) => [c, row[i]]))
    const actions = strings(r.actions)
    if (typeof r.dest !== 'string' || actions === null) return []
    return [{ to: r.dest, actions, from: r.origins === null ? null : strings(r.origins) }]
  })
}

/** Python's `fnmatch.translate` for the shell-style globs a boundary holds: `*`, `?` and `[...]`. */
function glob(pattern: string): RegExp {
  let out = ''
  for (let i = 0; i < pattern.length; i++) {
    const c = pattern[i] as string
    let j = i + 1
    if (c === '[' && pattern[j] === '!') j++
    if (c === '[' && pattern[j] === ']') j++
    const close = c === '[' ? pattern.indexOf(']', j) : -1
    if (c === '*') out += '.*'
    else if (c === '?') out += '.'
    else if (close > 0) {
      let set = pattern.slice(i + 1, close).replaceAll('\\', '\\\\')
      if (set.startsWith('!')) set = `^${set.slice(1)}`
      else if (set.startsWith('^')) set = `\\${set}`
      out += `[${set}]`
      i = close
    } else out += c.replace(/[.*+?^${}()|[\]\\/]/g, '\\$&')
  }
  return new RegExp(`^${out}$`, 's')
}

/** Whether `text` matches the glob, with case. */
export const matches = (text: string, pattern: string) => glob(pattern).test(text)

/**
 * Whether `boundary` covers `github.<action>` to `to` from the checkout at
 * mount `origin`. Destinations match without case. A boundary with `from`
 * covers a checkout at or under one of its paths, and also a checkout with no
 * mount, since the github action then names no `from` and every boundary
 * applies. `origin` undefined means the mount is unknown, and such a boundary
 * is not counted.
 */
export function covers(boundary: Boundary, action: string, to: string, origin: string | null | undefined): boolean {
  if (!matches(to.toLowerCase(), boundary.to.toLowerCase())) return false
  if (!boundary.actions.some(a => matches(`github.${action}`, a))) return false
  if (boundary.from === null || origin === null) return true
  if (origin === undefined) return false
  return boundary.from.some(path => origin === path || origin.startsWith(`${path.replace(/\/+$/, '')}/`))
}

/** The checkout's mount in `board --json refs .`: its path, null when no board holds it, undefined when unknown. */
export function parseMount(exitCode: number, stdout: string): string | null | undefined {
  const report = parseJson(stdout)
  if (exitCode !== 0) return report?.code === 'board::config::not_found' ? null : undefined
  const data = report?.schema === 'board/refs-report/v2' && Array.isArray(report.data) ? report.data : null
  const location = (data?.[0] as { location?: { display?: unknown; pointers?: unknown[] } } | undefined)?.location
  if (location === undefined || typeof location.display !== 'string') return undefined
  return Array.isArray(location.pointers) && location.pointers.length > 0 ? location.display : null
}
