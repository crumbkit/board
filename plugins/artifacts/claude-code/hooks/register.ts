import type { EngineInterface, Register } from 'claude-code'

// After the Artifact tool answers, the mod records what it saw in the
// personal board through `board --personal`: a publish or a read writes the
// artifact record when it is missing and a version record with a copy of the
// page; a listing of the account's own artifacts goes to artifacts.import.
// A version's `for` names the change the loop works on: a subagent's, from
// the runs mod's agent record, else the one change the session holds.
// A hook never blocks the tool: a failure is logged, never thrown.

const ARTIFACT_URL = /^https:\/\/claude\.ai\/(?:code\/)?artifact\/([A-Za-z0-9_-]+)$/
// The Artifact tool's read result names the file that holds a large page.
const SAVED = /saved to (\/.+?\.(html|md)) — /
// It inlines a smaller page after its header, inside this tag when another session may have written it.
const INLINE = /raw (HTML|Markdown) follows\]$/
const COWRITTEN = /<(cowritten-artifact-[a-z]+)>\n([\s\S]*)\n<\/\1>/
// A put refused with this found the file already there: the version is recorded.
const STALE = 'board::put::stale'
// The `limit` the skill's artifacts sync step lists with, the most the Artifact tool lists.
const SYNC_LIMIT = 200
// A refusal with one of these means the personal board does not take these
// records at all: no personal board, or no artifacts plugin. Only the debug log says so.
const QUIET = ['board::config::not_found', 'board::record::unknown_type', 'board::plugin::not_enabled', 'board::plugin::not_installed']

/** Where a page's bytes are: a file on disk, or text the tool's result held. */
export type Page = { file: string; ext: 'html' | 'md' } | { text: string; ext: 'html' | 'md' }
/** One version the tool's result showed. An undefined field is left out of the record, as JSON.stringify drops it. */
type Seen = { url: string; version?: string | undefined; title?: string | undefined; description?: string | undefined; source?: string | undefined; page?: Page | undefined }
type Refused = { code: string; message: string }
type Ran = { exitCode: number; stdout: string; stderr: string }

const iso = (ms: number) => new Date(ms).toISOString().replace(/\.\d{3}Z$/, 'Z')
// How the runs mod names an agent record from an agent id.
const segment = (id: string) => id.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '')

/** The page a read result's text holds or names, or undefined when it holds only part of one.
 *
 * The header is the first line that starts with `[Artifact `; a notice, such
 * as one saying the read is of a newer version, may come before it. Bracketed
 * notes, such as how many files the version has and what is stored for it,
 * may follow it before an inline page, and are not part of the page.
 */
export function readPage(text: string): Page | undefined {
  const lines = text.split('\n')
  const at = lines.findIndex(line => line.startsWith('[Artifact '))
  if (at === -1) return undefined
  const header = lines[at]!
  const saved = SAVED.exec(header)
  if (saved !== null) return { file: saved[1]!, ext: saved[2] as 'html' | 'md' }
  const inline = INLINE.exec(header)
  if (inline === null) return undefined
  const ext = inline[1] === 'HTML' ? 'html' : 'md'
  let start = at + 1
  while (start < lines.length && /^\[.*\]$/.test(lines[start]!)) start += 1
  const body = lines.slice(start).join('\n')
  const tagged = COWRITTEN.exec(body)
  return { text: tagged === null ? body : tagged[2]!, ext }
}

/** The page a publish sent: its source, when it is a page Board can keep beside the record. */
export function sentPage(source: string | undefined): Page | undefined {
  const ext = source?.match(/\.(html|md)$/)?.[1]
  return source === undefined || ext === undefined ? undefined : { file: source, ext: ext as 'html' | 'md' }
}

function refusal(ran: Ran): Refused {
  try {
    const failure = JSON.parse(ran.stdout) as Partial<Refused>
    return { code: failure.code ?? 'unknown', message: failure.message ?? ran.stderr.trim() }
  } catch {
    return { code: 'unknown', message: ran.stderr.trim() || `board exited with status ${ran.exitCode}` }
  }
}

class BoardRefused extends Error {
  constructor(readonly refused: Refused) {
    super(`${refused.code} ${refused.message}`)
  }
}

/** Runs `board --personal --json put <path>`; true when it wrote, false when the file was already there. */
async function put($: EngineInterface, path: string, body: { record: object } | { page: Page }): Promise<boolean> {
  const flags = ['--expect', 'none']
  let ran: Ran
  if ('record' in body) {
    ran = await $.process.run(['board', '--personal', '--json', 'put', path, ...flags], { stdin: JSON.stringify(body.record) })
  } else if ('text' in body.page) {
    ran = await $.process.run(['board', '--personal', '--json', 'put', path, '--file', ...flags], { stdin: body.page.text })
  } else {
    // A page can pass the 4 MiB a mod may read, so the shell hands the file to board.
    const script = 'exec board --personal --json put "$1" --file --expect none < "$2"'
    ran = await $.process.run(['sh', '-c', script, 'sh', path, body.page.file], { timeoutMs: 120_000 })
  }
  if (ran.exitCode === 0) return true
  const refused = refusal(ran)
  if (refused.code === STALE) return false
  throw new BoardRefused(refused)
}

/** The account the session is signed in to, from `oauthAccount.emailAddress` in `.claude.json`. */
async function account($: EngineInterface): Promise<string | undefined> {
  const dir = (await $.env.get('CLAUDE_CONFIG_DIR')) ?? (await $.env.get('HOME'))
  if (dir === undefined) return undefined
  // The file can pass the 4 MiB a mod may read, so jq reads it.
  const ran = await $.process.run(['jq', '-r', '.oauthAccount.emailAddress // empty', `${dir}/.claude.json`])
  const email = ran.stdout.trim()
  return ran.exitCode === 0 && email !== '' ? email : undefined
}

async function sha256($: EngineInterface, page: Page): Promise<string> {
  if ('text' in page) {
    const digest = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(page.text))
    return [...new Uint8Array(digest)].map(byte => byte.toString(16).padStart(2, '0')).join('')
  }
  const ran = await $.process.run(['shasum', '-a', '256', page.file], { timeoutMs: 60_000 })
  const hash = ran.stdout.slice(0, 64)
  if (ran.exitCode !== 0 || !/^[0-9a-f]{64}$/.test(hash)) throw new Error(`shasum could not hash ${page.file}: ${ran.stderr.trim()}`)
  return hash
}

function refused($: EngineInterface, what: string, error: unknown) {
  const to = error instanceof BoardRefused && QUIET.includes(error.refused.code) ? 'debug' : 'transcript'
  $.ui.log(`board: ${what}: ${error instanceof Error ? error.message : String(error)}`, { to })
}

/** The output document of `board --personal --json <args>`, or undefined when board refused. */
async function document($: EngineInterface, args: string[]): Promise<unknown> {
  const ran = await $.process.run(['board', '--personal', '--json', ...args])
  return ran.exitCode === 0 ? JSON.parse(ran.stdout) : undefined
}

/**
 * The change the loop works on: for a subagent, the `works_on` of its agent
 * record; else, or without one, the one open change the session holds.
 */
async function belongsTo($: EngineInterface, agentId: string | undefined): Promise<string | undefined> {
  if (agentId !== undefined) {
    const got = (await document($, ['get', `/agents/${segment(agentId)}`])) as { data?: { record?: { works_on?: string } }[] } | undefined
    const worksOn = got?.data?.[0]?.record?.works_on
    if (worksOn !== undefined) return worksOn
  }
  const open = (await document($, ['call', 'changes.open', '--holder', await $.session.id()])) as { data?: { output?: { changes: { path: string }[]; total: number } } } | undefined
  const output = open?.data?.output
  return output?.total === 1 ? output.changes[0]?.path : undefined
}

/** Writes the artifact record when it is missing, then the version and its page when the version is new. */
async function record($: EngineInterface, seen: Seen, agentId: string | undefined) {
  const id = ARTIFACT_URL.exec(seen.url)?.[1]
  if (id === undefined) return $.ui.log(`board: ${seen.url} is not a claude.ai artifact link, so it was not recorded`, { to: 'debug' })
  try {
    const email = await account($)
    if (email === undefined) return $.ui.log(`board: no oauthAccount.emailAddress in .claude.json, so ${seen.url} was not recorded`)
    const sha = seen.page === undefined ? undefined : await sha256($, seen.page)
    const key = seen.version ?? sha?.slice(0, 16)
    if (key === undefined) return $.ui.log(`board: ${seen.url} gave neither a version id nor a page, so it was not recorded`, { to: 'debug' })
    const now = iso(await $.clock.now())
    const belongs = await belongsTo($, agentId)
    const artifact = `/artifacts/${id}`
    const path = `${artifact}/versions/${key}`
    const created = await put($, artifact, { record: { $type: 'artifacts/artifact/v1', url: seen.url, account: email, first_seen: now } })
    const copied = seen.page === undefined ? false : await put($, `${path}.${seen.page.ext}`, { page: seen.page })
    const version = {
      $type: 'artifacts/version/v1',
      version: seen.version,
      title: seen.title,
      description: seen.description,
      copy: sha === undefined ? 'none' : undefined,
      sha256: sha,
      source: seen.source,
      session: await $.session.id(),
      at: now,
      for: belongs === undefined ? undefined : [belongs],
    }
    const written = await put($, path, { record: version })
    if (!written && !copied && !created) return $.ui.log(`board: ${seen.url} version ${key} is already recorded`, { to: 'debug' })
    $.ui.log(`board: recorded ${seen.url} version ${key} as ${path}`)
  } catch (error) {
    refused($, `no record of ${seen.url}`, error)
  }
}

type Listing = { artifacts: { url: string }[]; scope?: string }
type Imported = { written: string[]; known: number; truncated: { message: string } | null }

/** Passes the listing to artifacts.import; resolves what the agent should be told, if anything. */
async function imported($: EngineInterface, listing: Listing, sync: boolean): Promise<string | undefined> {
  try {
    const email = await account($)
    if (email === undefined) {
      $.ui.log('board: no oauthAccount.emailAddress in .claude.json, so the listing was not imported')
      return undefined
    }
    const ran = await $.process.run(['board', '--personal', '--json', 'call', 'artifacts.import'], { stdin: JSON.stringify({ account: email, listing }) })
    if (ran.exitCode !== 0) throw new BoardRefused(refusal(ran))
    const output = (JSON.parse(ran.stdout) as { data: { output: Imported } }).data.output
    const urls = output.written.map(path => listing.artifacts.find(row => row.url.endsWith(`/${path.split('/').at(-1)}`))?.url ?? path)
    $.ui.log(`board: imported ${urls.length} new artifact${urls.length === 1 ? '' : 's'} of ${email}; ${output.known} already recorded`)
    const lines = urls.length === 0 ? ['The artifacts mod found no new artifacts in this listing.'] : [
      `The artifacts mod recorded ${urls.length} new artifact${urls.length === 1 ? '' : 's'} of ${email} without a version.`,
      'Read each with the Artifact tool\'s read action so the mod records its current version:',
      ...urls.map(url => `- ${url}`),
    ]
    // Only the sync step, which lists 200, asks the agent to act. A listing of 200 rows
    // that says it is truncated is all the tool will list; a shorter one was cut by its limit.
    if (!sync) return undefined
    const final = output.truncated !== null && listing.artifacts.length === SYNC_LIMIT
    return [...lines, ...(final ? [`The listing was truncated: ${output.truncated!.message}.`] : [])].join('\n')
  } catch (error) {
    refused($, 'the listing was not imported', error)
    return undefined
  }
}

type Published = { url?: string; version?: string; title?: string; path?: string; owned?: boolean }
type Read = { read?: { url: string; code: number; title?: string }; owned?: boolean; artifactRead?: { ver?: string } }

export const register: Register = on => {
  on('tool.call', { tool: 'Artifact' }, async ($, e, next) => {
    const ran = await next(e)
    if (ran.deny !== undefined || ran.isError === true) return ran
    const action = e.action ?? 'publish'
    if (action === 'publish' && e.asset !== true) {
      const result = ran.result as Published
      // A publish that names no local page, such as one through an Artifact type, has no copy.
      const source = e.file_path === undefined ? undefined : (result.path ?? e.file_path)
      // A publish to another person's artifact, which the owner may edit, is not the owner's artifact.
      if (result.url !== undefined && result.owned !== false) {
        const page = sentPage(source)
        await record($, { url: result.url, version: result.version, title: result.title, description: e.description, source: page === undefined ? undefined : source, page }, e.agentId)
      }
    } else if (action === 'read' && e.path === undefined && e.paths === undefined && e.type_url === undefined) {
      const result = ran.result as Read
      // Only the owner's own artifact comes back as its page; another's is a summary.
      if (result.read?.code !== 200 || result.owned !== true) return ran
      const page = readPage(ran.text ?? '')
      if (page === undefined) {
        $.ui.log(`board: the read of ${result.read.url} held no whole page, so it was not recorded`, { to: 'debug' })
        return ran
      }
      await record($, { url: result.read.url, version: result.artifactRead?.ver, title: result.read.title, page }, e.agentId)
    } else if (action === 'list' && (e.scope ?? 'mine') === 'mine' && e.type === undefined && e.type_url === undefined && e.url === undefined) {
      const listing = ran.result as Listing
      if (!Array.isArray(listing.artifacts) || listing.scope !== undefined) return ran
      const told = await imported($, listing, e.limit === SYNC_LIMIT)
      if (told !== undefined) return { ...ran, context: [...(ran.context ?? []), told] }
    }
    return ran
  })
}
