// What a Bash command sends to GitHub, read from its text without running it,
// and the words a refusal says. Pure functions, so the tests can pin every word.

/**
 * The command a refusal names, followed by the action's name. This is the one
 * place to change once `board gh <action>` exists.
 */
export const BOARD_COMMAND = 'board call github.'

/** One word of a command: its text once quotes are removed, and its source as typed. */
export type Word = { text: string; raw: string; dynamic: boolean }

/** A write to GitHub that a command makes. */
export type Write = {
  /** The github action that sends the same content, or `api` for a gh write Board has no action for. */
  action: string
  /** What the command is, such as `gh pr merge`, for a write Board has no action for. */
  label?: string
  /** A sentence the refusal adds after the `Next:` step. */
  note?: string
  /** The directory it runs in, relative to the session's, as typed; empty for the session's own. */
  cd: string
  /** The same directory as the shell will see it, or null when a word in it is expanded by the shell. */
  dir: string | null
  /** `owner/repo` when the command names it; otherwise the checkout's remote decides. */
  repo?: string
  /** The remote a `git push` names, or the empty string for git's default. */
  remote?: string
  /** The `board` command that sends the same content. */
  next: string
}

/**
 * The words of each simple command in `source`, split at `;`, `&&`, `||`,
 * `|`, `&`, parentheses and line ends. A word holding `$`, a backtick or a
 * glob is `dynamic`: its text is not what the shell will see. The bodies of
 * here-documents are skipped. `null` when a quote or `$(` is not closed.
 */
export function segments(source: string): Word[][] | null {
  /** The here-document `<<` at `i` opens: its tag, whether tabs are stripped, and the index after the tag. */
  const opened = (i: number): { tag: string; strip: boolean; end: number } | null => {
    const strip = source[i + 2] === '-'
    const found = /^\s*(['"]?)([^\s'"<>;&|()]+)\1/.exec(source.slice(i + (strip ? 3 : 2)))
    return found === null ? null : { tag: found[2] as string, strip, end: i + (strip ? 3 : 2) + found[0].length }
  }
  /** The index after the bodies of `pending`, which start at line start `i`, each ending at its tag's line. */
  const bodies = (i: number, pending: { tag: string; strip: boolean }[]): number => {
    for (const { tag, strip } of pending.splice(0)) {
      while (i < source.length) {
        const end = source.indexOf('\n', i)
        const line = source.slice(i, end < 0 ? source.length : end)
        i = end < 0 ? source.length : end + 1
        if ((strip ? line.replace(/^\t+/, '') : line) === tag) break
      }
    }
    return i
  }
  const all: Word[][] = []
  let words: Word[] = []
  let word: Word | null = null
  let skipNext = false
  const heredocs: { tag: string; strip: boolean }[] = []
  const endWord = () => {
    if (word !== null && !skipNext) words.push(word)
    else if (word !== null) skipNext = false
    word = null
  }
  const endSegment = () => {
    endWord()
    skipNext = false
    if (words.length > 0) all.push(words)
    words = []
  }
  const add = (text: string, raw: string, dynamic = false) => {
    word ??= { text: '', raw: '', dynamic: false }
    word.text += text
    word.raw += raw
    word.dynamic ||= dynamic
  }
  /** The index just past the `)` that closes the `$(` before `i`, or -1. */
  const closing = (i: number): number => {
    let depth = 1
    // A here-document's body is literal text, so a quote in it opens nothing.
    const pending: { tag: string; strip: boolean }[] = []
    for (let j = i; j < source.length; j++) {
      const c = source[j]
      if (c === '\n' && pending.length > 0) j = bodies(j + 1, pending) - 1
      else if (c === '<' && source[j + 1] === '<' && source[j + 2] !== '<') {
        const doc = opened(j)
        if (doc === null) return -1
        pending.push(doc)
        j = doc.end - 1
      } else if (c === '\\') j++
      else if (c === "'") {
        j = source.indexOf("'", j + 1)
        if (j < 0) return -1
      } else if (c === '"') {
        const end = closingQuote(j + 1)
        if (end < 0) return -1
        j = end - 1
      } else if (c === '(') depth++
      else if (c === ')' && --depth === 0) return j + 1
    }
    return -1
  }
  /** The index just past the `"` that closes the one before `i`, or -1. */
  const closingQuote = (i: number): number => {
    for (let j = i; j < source.length; j++) {
      const c = source[j]
      if (c === '\\') j++
      else if (c === '"') return j + 1
      else if (c === '$' && source[j + 1] === '(') {
        const end = closing(j + 2)
        if (end < 0) return -1
        j = end - 1
      }
    }
    return -1
  }
  let i = 0
  while (i < source.length) {
    const c = source[i] as string
    if (c === '\n') {
      endSegment()
      // The here-documents opened on that line end at their tag.
      i = bodies(i + 1, heredocs)
    } else if (c === ' ' || c === '\t') {
      endWord()
      i++
    } else if (c === '#' && word === null) {
      const end = source.indexOf('\n', i)
      i = end < 0 ? source.length : end
    } else if (';&|'.includes(c)) {
      endSegment()
      i++
    } else if (c === '(' || c === ')') {
      // A subshell's bounds are segments of their own, with no text, so `writes` can undo a `cd` inside one.
      endSegment()
      all.push([{ text: '', raw: c, dynamic: false }])
      i++
    } else if (c === '<' && source[i + 1] === '<' && source[i + 2] !== '<') {
      endWord()
      const doc = opened(i)
      if (doc === null) return null
      heredocs.push(doc)
      i = doc.end
    } else if (c === '<' || c === '>') {
      // A redirection: its operator and its target are not words of the command.
      // A file descriptor before it, such as the 2 of 2>&1, is not a word either.
      if (/^\d+$/.test((word as Word | null)?.raw ?? '')) word = null
      endWord()
      i++
      while (i < source.length && '<>&|'.includes(source[i] as string)) i++
      if (source[i - 1] === '&' && /\d|-/.test(source[i] ?? '')) i++
      else skipNext = true
    } else if (c === "'") {
      const end = source.indexOf("'", i + 1)
      if (end < 0) return null
      add(source.slice(i + 1, end), source.slice(i, end + 1))
      i = end + 1
    } else if (c === '"') {
      const end = closingQuote(i + 1)
      if (end < 0) return null
      const inner = source.slice(i + 1, end - 1)
      add(inner.replace(/\\(["\\$`])/g, '$1'), source.slice(i, end), /[$`]/.test(inner))
      i = end
    } else if (c === '\\') {
      add(source[i + 1] ?? '', source.slice(i, i + 2))
      i += 2
    } else if (c === '$' && source[i + 1] === '(') {
      const end = closing(i + 2)
      if (end < 0) return null
      add('', source.slice(i, end), true)
      i = end
    } else {
      add(c, c, '$`*?['.includes(c))
      i++
    }
  }
  endSegment()
  return all
}

/** Words a command may start with that run the rest as a command. */
const WRAPPERS = new Set(['env', 'command', 'exec', 'time', 'nohup'])
/** `gh pr` and `gh issue` flags that take no value, so the word after one is not its value. */
const GH_SWITCHES = [
  '-d', '--draft', '-f', '--fill', '--fill-first', '--fill-verbose', '-w', '--web', '-e', '--editor', '--edit-last',
  '--create-if-none', '--delete-last', '--yes', '--no-maintainer-edit', '--dry-run', '--remove-milestone',
]
/** The switches of the verbs that have more, and `gh api`'s, which differ: its `-f` takes a field. */
const SWITCHES: Record<string, string[]> = {
  'pr review': ['-a', '--approve', '-r', '--request-changes', '-c', '--comment'],
  'pr merge': ['-s', '--squash', '-m', '--merge', '-r', '--rebase', '--admin', '--auto', '--disable-auto', '--delete-branch'],
  'release create': ['--prerelease', '--latest', '--generate-notes', '--verify-tag'],
  'release edit': ['--prerelease', '--latest', '--verify-tag'],
}
const API_SWITCHES = new Set(['-i', '--include', '--paginate', '--silent', '--slurp', '--verbose'])
/**
 * The gh commands with no matching action that send text, and the flags that carry it. One of them with such
 * a flag is refused as a write Board has no action for. `gh repo create --push` and other commands that send
 * no text by flag are not read (see README.md).
 */
const TEXT: Record<string, string[]> = {
  'pr review': ['-b', '--body', '-F', '--body-file'],
  'pr merge': ['-b', '--body', '-t', '--subject', '-F', '--body-file'],
  'pr close': ['-c', '--comment'],
  'pr reopen': ['-c', '--comment'],
  'issue close': ['-c', '--comment'],
  'issue reopen': ['-c', '--comment'],
  'issue edit': ['-t', '--title', '-b', '--body', '-F', '--body-file'],
  'release create': ['-t', '--title', '-n', '--notes', '-F', '--notes-file'],
  'release edit': ['-t', '--title', '-n', '--notes', '-F', '--notes-file'],
  'repo edit': ['-d', '--description'],
}
/** `git` options that take the next word as their value. */
const GIT_VALUED = new Set(['-C', '-c', '--git-dir', '--work-tree', '--namespace', '--config-env'])
/** `git push` options that take the next word as their value. */
const PUSH_VALUED = new Set(['-o', '--push-option', '--repo', '--receive-pack', '--exec'])

/** A word as the shell would pass it, quoted for a command a refusal names. */
const quoted = (w: Word) => (w.dynamic || w.raw !== w.text ? w.raw : /^[\w@%+=:,./-]+$/.test(w.text) ? w.text : `'${w.text.replaceAll("'", `'\\''`)}'`)

/** A word's value after its first `at` characters, such as the `POST` of `-XPOST`. */
const rest = (w: Word, at: number): Word => {
  const text = w.text.slice(at)
  return { text, raw: w.raw === w.text ? text : quoted({ text, raw: text, dynamic: w.dynamic }), dynamic: w.dynamic }
}

/** The flags and positional words after `start`, with gh's rule that a flag not in `switches` takes a value. */
function flags(words: Word[], start: number, switches: Set<string>): { named: Map<string, Word>; positional: Word[] } {
  const named = new Map<string, Word>()
  const positional: Word[] = []
  for (let i = start; i < words.length; i++) {
    const w = words[i] as Word
    const t = w.text
    if (!t.startsWith('-') || t === '-') positional.push(w)
    else if (t.includes('=') && t.startsWith('--')) named.set(t.slice(0, t.indexOf('=')), rest(w, t.indexOf('=') + 1))
    else if (switches.has(t)) named.set(t, { text: 'true', raw: 'true', dynamic: false })
    else if (!t.startsWith('--') && t.length > 2 && !switches.has(t.slice(0, 2))) named.set(t.slice(0, 2), rest(w, 2))
    else {
      const value = words[++i]
      if (value !== undefined) named.set(t, value)
    }
  }
  return { named, positional }
}

const pick = (named: Map<string, Word>, ...names: string[]) => names.map(n => named.get(n)).find(w => w !== undefined)

/** The `board` command for `action` with each given flag, and a placeholder for a missing required one. */
function command(action: string, given: [string, Word | string | undefined][]): string {
  const parts = given.flatMap(([flag, value]) =>
    value === undefined ? [] : [`--${flag} ${typeof value === 'string' ? value : quoted(value)}`],
  )
  return [`${BOARD_COMMAND}${action}`, ...parts].join(' ')
}

/** The body a gh command gives by `--body` or `--body-file`, as a word to pass on. */
function body(named: Map<string, Word>): Word | string | undefined {
  const file = pick(named, '-F', '--body-file')
  if (file !== undefined) return file.text === '-' ? '<body>' : `"$(cat ${quoted(file)})"`
  return pick(named, '-b', '--body')
}

/** The number a gh command names by number or URL, or a placeholder. */
function number(w: Word | undefined): string {
  const found = /(?:^|\/(?:pull|issues)\/)#?(\d+)\/?$/.exec(w?.text ?? '')
  return found ? (found[1] as string) : '<number>'
}

/** The `owner/repo` that `--repo` names as `[host/]owner/repo`; null for a host other than github.com. */
function repoOf(w: Word | undefined): string | null | undefined {
  if (w === undefined || w.dynamic) return undefined
  const parts = w.text.split('/')
  return parts.length > 2 && parts[0]?.toLowerCase() !== 'github.com' ? null : parts.slice(-2).join('/')
}

/** The write a `gh` command makes, or null when it makes none. */
function ghWrite(words: Word[], cd: Place): Write | null {
  const [group, verb] = [words[1]?.text, words[2]?.text]
  const key = `${group} ${verb}`
  const switches = new Set(group === 'api' ? API_SWITCHES : [...GH_SWITCHES, ...(SWITCHES[key] ?? [])])
  const { named, positional } = flags(words, group === 'api' ? 2 : 3, switches)
  const repo = repoOf(pick(named, '-R', '--repo'))
  if (repo === null) return null
  const at = { ...cd, ...(repo === undefined ? {} : { repo }) }
  const title = pick(named, '-t', '--title')
  if (group === 'pr' && verb === 'create') {
    const base = pick(named, '-B', '--base')
    const draft = pick(named, '-d', '--draft') === undefined ? undefined : 'true'
    const next = command('pr-create', [['title', title ?? '<title>'], ['body', body(named) ?? '<body>'], ['base', base], ['draft', draft]])
    const head = pick(named, '-H', '--head')
    const note = head === undefined ? {} : { note: ` It opens the pull request from the branch checked out, not from ${quoted(head)}.` }
    return { action: 'pr-create', ...at, next, ...note }
  }
  if (group === 'pr' && verb === 'edit') {
    const next = command('pr-edit', [['number', number(positional[0])], ['title', title], ['body', body(named)]])
    return { action: 'pr-edit', ...at, next }
  }
  if ((group === 'pr' || group === 'issue') && verb === 'comment') {
    return { action: 'comment', ...at, next: command('comment', [['number', number(positional[0])], ['body', body(named) ?? '<body>']]) }
  }
  if (group === 'issue' && verb === 'create') {
    return { action: 'issue-create', ...at, next: command('issue-create', [['title', title ?? '<title>'], ['body', body(named) ?? '<body>']]) }
  }
  if (TEXT[key]?.some(flag => named.has(flag))) return { action: 'api', label: `gh ${key}`, ...at, next: '' }
  if (group !== 'api') return null
  return apiWrite(named, positional[0], cd)
}

/** The write a `gh api` call makes: a method other than GET, or fields without a method, which gh sends as POST. */
function apiWrite(named: Map<string, Word>, endpoint: Word | undefined, cd: Place): Write | null {
  const method = (pick(named, '-X', '--method')?.text ?? '').toUpperCase()
  const fields = ['-f', '-F', '--field', '--raw-field', '--input'].some(f => named.has(f))
  if (endpoint === undefined || method === 'GET' || method === 'HEAD' || (method === '' && !fields)) return null
  const path = endpoint.text.replace(/^\//, '')
  // A GraphQL mutation may name any repository by node id, so its destination is unknown (see README.md).
  if (path === 'graphql') return null
  const found = /^repos\/([^/]+)\/([^/]+)(\/.*)?$/.exec(path)
  if (found === null || endpoint.dynamic) return null
  const [owner, name, rest = ''] = found.slice(1) as [string, string, string?]
  const placeholder = owner.startsWith('{') || owner.startsWith(':')
  const at = { ...cd, ...(placeholder ? {} : { repo: `${owner}/${name}` }) }
  const verb = method || 'POST'
  const numbered = /^\/(pulls|issues)\/(\d+)(\/comments)?$/.exec(rest)
  if (verb === 'POST' && rest === '/pulls') return { action: 'pr-create', ...at, next: command('pr-create', [['title', '<title>'], ['body', '<body>']]) }
  if (verb === 'POST' && rest === '/issues') return { action: 'issue-create', ...at, next: command('issue-create', [['title', '<title>'], ['body', '<body>']]) }
  if (verb === 'PATCH' && numbered?.[1] === 'pulls' && !numbered[3]) {
    return { action: 'pr-edit', ...at, next: command('pr-edit', [['number', numbered[2]], ['title', '<title>']]) }
  }
  if (verb === 'POST' && numbered?.[1] === 'issues' && numbered[3]) {
    return { action: 'comment', ...at, next: command('comment', [['number', numbered[2]], ['body', '<body>']]) }
  }
  return { action: 'api', label: 'gh api', ...at, next: '' }
}

/** The write a `git` command makes, or null when it is not a push. */
function gitWrite(words: Word[], cd: Place): Write | null {
  let i = 1
  let at = cd
  for (; i < words.length && words[i]?.text.startsWith('-'); i++) {
    const t = words[i]?.text as string
    // Another repository or work tree than the directory's own leaves the remote unknown.
    if (/^--(git-dir|work-tree)(=|$)/.test(t)) at = { ...at, dir: null }
    if (GIT_VALUED.has(t)) {
      if (t === '-C') at = move(at, words[i + 1])
      i++
    }
  }
  if (words[i]?.text !== 'push') return null
  let remote = ''
  for (let j = i + 1; j < words.length; j++) {
    const w = words[j] as Word
    if (w.text === '--repo' || w.text.startsWith('--repo=')) remote = w.text === '--repo' ? (words[j + 1]?.text ?? '') : w.text.slice(7)
    if (PUSH_VALUED.has(w.text)) j++
    else if (!w.text.startsWith('-') && remote === '') {
      remote = w.dynamic ? '' : w.text
      break
    }
  }
  return { action: 'push', ...at, remote, next: `${BOARD_COMMAND}push` }
}

/** Where a command runs: as typed, and as the shell sees it. */
type Place = { cd: string; dir: string | null }

/** `next` relative to `base`; an absolute `next` replaces it. */
const join = (base: string, next: string) => (next.startsWith('/') || next.startsWith('~') || base === '' ? next : `${base}/${next}`)

/** The place `cd <word>` or `git -C <word>` moves to from `from`. */
const move = (from: Place, word: Word | undefined): Place =>
  word === undefined
    ? from
    : { cd: join(from.cd, word.raw), dir: word.dynamic || from.dir === null ? null : join(from.dir, word.text) }

/**
 * Every write to GitHub `source` makes, in order: `git push`, `gh pr
 * create|edit|comment`, `gh issue create|comment`, the gh commands in TEXT
 * with text to send, and `gh api` calls that write to a repository. `cd
 * <dir>` moves the commands after it, until the end of a subshell it is in. `null` when
 * the text cannot be read as a command, such as an unclosed quote.
 */
export function writes(source: string): Write[] | null {
  // Most commands name neither tool, and cost no parse.
  if (!/\b(git|gh)\b/.test(source)) return []
  const all = segments(source)
  if (all === null) return null
  const found: Write[] = []
  let cd: Place = { cd: '', dir: '' }
  // The place before each open subshell, which its end restores.
  const outer: Place[] = []
  for (const segment of all) {
    const bound = segment.length === 1 ? segment[0]?.raw : undefined
    if (bound === '(' || bound === ')') {
      cd = bound === '(' ? (outer.push(cd), cd) : (outer.pop() ?? cd)
      continue
    }
    let words = segment
    let here = cd
    while (words[0] !== undefined && (WRAPPERS.has(words[0].text) || /^[A-Za-z_]\w*=/.test(words[0].text))) {
      if (/^GIT_(DIR|WORK_TREE)=/.test(words[0].text)) here = { ...here, dir: null }
      words = words.slice(1)
    }
    const head = words[0]
    if (head === undefined || head.dynamic) continue
    if (head.text === 'cd') cd = move(cd, words[1])
    const write = head.text === 'git' ? gitWrite(words, here) : head.text === 'gh' ? ghWrite(words, here) : null
    if (write !== null) found.push(write)
  }
  return found
}

/** The reason the model reads for a refused write: what it sends where, and the `Next:` step. */
export function refusal(write: Write, to: string): string {
  const where = write.cd === '' ? '' : `cd ${write.cd} && `
  const what =
    write.action === 'push'
      ? 'This git push sends commits'
      : write.action === 'api'
        ? `This ${write.label ?? 'gh'} call writes`
        : `This gh command sends content that github.${write.action} sends`
  const head = `${what} to ${to}, which a boundary in the personal board covers. Content for it goes through Board, which has the boundary check it first.`
  if (write.action === 'api') {
    return (
      `${head} Board has no action for this write. Next: send it with one of \`${BOARD_COMMAND}push\`, ` +
      '`pr-create`, `pr-edit`, `comment` or `issue-create` if one fits, or ask the owner how to send it.'
    )
  }
  const scope = write.action === 'push' ? ' It pushes the branch checked out to origin.' : ''
  return `${head} Next: run \`${where}${write.next}\`.${scope}${write.note ?? ''}`
}
