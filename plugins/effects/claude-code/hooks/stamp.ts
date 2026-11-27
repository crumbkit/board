// Whether the board changed since the mod last read it, told without running
// `board`.
//
// Each `board` read costs about a second of snapshot work and competes for
// the index's lock, so a mod that polls each minute reads only when the
// board's stamp has changed since the last read that worked. The stamp is the
// commit the board's HEAD names and the size and modified time of its Git
// index, read through `$.fs`, so a new commit or a staged write changes it.
//
// Soft limit: this reads only what a board writes, a ref file or
// packed-refs line and the index. A hand edit that is neither committed nor
// staged leaves the stamp as it was, and shows with the next commit. A
// followed mount's new commits show at the board's own next commit. Anything
// the stamp cannot read makes it unknown, and an unknown stamp reads `board`
// every time, as the mods did before.
//
// A copy of this file sits in each mod that polls; .github/workflows/mods.yml
// checks the copies match the plans mod's, whose tests cover it.

import type { FsStat } from 'claude-code'

/** What the stamp reads through: the plugin's `$`, or a test's stand-in for its files. */
export type Reader = {
  fs: { read: (path: string) => Promise<string>; exists: (path: string) => Promise<boolean>; stat: (path: string) => Promise<Pick<FsStat, 'kind' | 'size' | 'mtimeMs'>> }
  env: { get: (name: string) => Promise<string | undefined> }
}

/** The stamp the last read that worked saw, kept in the mod's memory for the session; null when none did. */
export type Seen = { stamp: string | null }

export const unseen = (): Seen => ({ stamp: null })

/** `path` with a leading `~/` written out as `home`. */
const expanded = (path: string, home: string | undefined) => (path.startsWith('~/') && home ? `${home}${path.slice(1)}` : path)

/** The directory `personal` names in the board config's text, or null when the text names none. */
export function personalOf(config: string): string | null {
  try {
    const personal = (JSON.parse(config) as { personal?: unknown }).personal
    return typeof personal === 'string' && personal !== '' ? personal : null
  } catch {
    return null
  }
}

/**
 * The board's directory: `configured` when it is set, else the board config's
 * `personal` entry, as `board --personal` finds it. Null when neither names one.
 */
export async function boardOf($: Reader, configured: string): Promise<string | null> {
  const home = await $.env.get('HOME')
  if (configured !== '') return expanded(configured, home)
  const config = `${(await $.env.get('XDG_CONFIG_HOME')) || `${home}/.config`}/board/config.json`
  const personal = personalOf(await $.fs.read(config))
  return personal === null ? null : expanded(personal, home)
}

/** The commit `ref` names in `packed`, the text of a packed-refs file, or null when it names none. */
export function packedRef(packed: string, ref: string): string | null {
  for (const line of packed.split('\n')) {
    if (line.startsWith('#') || line.startsWith('^')) continue
    const [sha, name] = line.trim().split(' ')
    if (name === ref && sha) return sha
  }
  return null
}

/** `path` joined to `dir` when it is relative. */
const under = (dir: string, path: string) => (path.startsWith('/') ? path : `${dir}/${path}`)

/** The text of the file at `path`, without its surrounding whitespace. */
async function text($: Reader, path: string): Promise<string> {
  return (await $.fs.read(path)).trim()
}

/**
 * The board's Git directory, and the common one its refs live in: `.git`
 * itself, or the directory a `.git` file names, as a worktree's does.
 */
async function gitDirs($: Reader, dir: string): Promise<{ git: string; common: string }> {
  const dotGit = `${dir}/.git`
  const kind = (await $.fs.stat(dotGit)).kind
  const git = kind === 'dir' ? dotGit : under(dir, (await text($, dotGit)).replace(/^gitdir:\s*/, ''))
  const common = (await $.fs.exists(`${git}/commondir`)) ? under(git, await text($, `${git}/commondir`)) : git
  return { git, common }
}

/** The commit HEAD names: detached, through a ref file, or through packed-refs. */
async function head($: Reader, git: string, common: string): Promise<string> {
  const named = await text($, `${git}/HEAD`)
  if (!named.startsWith('ref:')) return named
  const ref = named.slice('ref:'.length).trim()
  for (const where of [git, common]) {
    if (await $.fs.exists(`${where}/${ref}`)) return text($, `${where}/${ref}`)
  }
  const sha = packedRef(await $.fs.read(`${common}/packed-refs`), ref)
  if (sha === null) throw new Error(`${ref} names no commit`)
  return sha
}

/** The board's stamp: the commit HEAD names and the index's size and modified time. Null when any of it cannot be read. */
export async function stampOf($: Reader, configured: string): Promise<string | null> {
  try {
    const dir = await boardOf($, configured)
    if (dir === null) return null
    const { git, common } = await gitDirs($, dir)
    const index = await $.fs.stat(`${git}/index`)
    return `${await head($, git, common)} ${index.size} ${index.mtimeMs}`
  } catch {
    return null
  }
}

/**
 * Runs `read` unless the board's stamp is known and is the one the last read
 * that worked saw; `always` runs it regardless, as a read the owner starts
 * does. `read` says whether it worked. One that worked records the stamp
 * taken before it began, so a commit made while it ran is read next time;
 * one that failed forgets the stamp, so the next poll reads again. Resolves
 * whether `read` ran.
 */
export async function whenChanged($: Reader, configured: string, seen: Seen, read: () => Promise<boolean>, always = false): Promise<boolean> {
  const stamp = await stampOf($, configured)
  if (!always && stamp !== null && stamp === seen.stamp) return false
  seen.stamp = null
  if (await read()) seen.stamp = stamp
  return true
}
