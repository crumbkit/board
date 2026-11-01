import { expect, test } from 'claude-code/testing'
import type { On } from 'claude-code'
import type { Engine } from 'claude-code/testing'

import { BOARD_COMMAND, writes } from '../hooks/command'
import { covers, matches } from '../hooks/cover'

// These prove which Bash calls the mod refuses and what the refusal says,
// with git and board stood in by a process.run hook. They do not run board or
// git: the Python tests prove what the github actions and the pre-push hook
// check, and the boundary plugin's tests prove how a boundary matches.

const ORIGIN = 'https://github.com/Octo/Widgets.git'
type Row = [string, string, string | null]
const boundary = (to: string, actions = ['github.*'], from: string[] | null = null): Row => [
  to,
  JSON.stringify(actions),
  from === null ? null : JSON.stringify(from),
]
const query = (rows: Row[]) =>
  JSON.stringify({ schema: 'board/query-report/v2', data: rows, truncated: false, summary: { columns: ['dest', 'actions', 'origins'] } })
const refs = (display: string, mounted = true) =>
  JSON.stringify({
    schema: 'board/refs-report/v2',
    data: [{ location: { display, pointers: mounted ? [{ repository: '/board' }] : [] } }],
    failures: [],
  })

type Answers = {
  boundaries?: Row[] | 'fails'
  urls?: string
  mount?: { exitCode: number; stdout: string }
}

/** Stands in for git, board and the Bash tool beneath the mod, and keeps what ran. */
function engine(on: On, answers: Answers) {
  const runs: string[] = []
  const ran: string[] = []
  const result = (exitCode: number, stdout: string) => ({
    value: { exitCode, stdout, stderr: '', isStdoutTruncated: false, isStderrTruncated: false },
  })
  on('process.run', (_, e) => {
    const dir = e.argv[4] ?? ''
    if (e.argv.includes('query')) {
      runs.push(`query in '${dir}'`)
      const rows = answers.boundaries ?? [boundary('github:octo/*')]
      return rows === 'fails' ? result(1, '') : result(0, query(rows))
    }
    if (e.argv.includes('refs')) {
      runs.push(`refs in '${dir}'`)
      return result(answers.mount?.exitCode ?? 0, answers.mount?.stdout ?? refs('/repos/widgets'))
    }
    runs.push(`urls of '${e.argv[5]}' in '${dir}'`)
    return result(0, `${answers.urls ?? ORIGIN}\n`)
  })
  on('tool.call', (_, e) => {
    ran.push(e.tool === 'Bash' ? e.command : e.tool)
    return { result: { stdout: '', stderr: '', interrupted: false } } as never
  })
  return { runs, ran }
}

const bash = async ($: Engine, command: string) => {
  const answer = await $.tool.call({ tool: 'Bash', command })
  return 'deny' in answer && typeof answer.deny === 'string' ? answer.deny : answer.isError ? String(answer.text) : null
}

test('a git push to a covered repository is refused with the board command, and one to another repository runs', async ($, on) => {
  const answers: Answers = {}
  const { runs, ran } = engine(on, answers)
  expect(await bash($, 'git push')).toBe(
    'This git push sends commits to github:octo/widgets, which a boundary in the personal board covers. Content ' +
      'for it goes through Board, which has the boundary check it first. Next: run `board call github.push`. It ' +
      'pushes the branch checked out to origin.',
  )
  expect(runs).toEqual(["query in ''", "urls of '' in ''"])
  expect(ran).toEqual([])
  answers.urls = 'git@github.com:someone/else.git'
  expect(await bash($, 'git push')).toBeNull()
  expect(ran).toEqual(['git push'])
})

test('gh pr create is refused with github.pr-create carrying the same title and body, here-document and all', async ($, on) => {
  engine(on, {})
  const command = `gh pr create --base main --title "Add a gear" --body "$(cat <<'EOF'\nIt turns.\ngit push\nEOF\n)"`
  const reason = await bash($, command)
  expect(reason).toContain(
    `Next: run \`board call github.pr-create --title "Add a gear" --body "$(cat <<'EOF'\nIt turns.\ngit push\nEOF\n)" --base main\`.`,
  )
})

test('each gh write names its matching action, number and text', () => {
  const next = (command: string) => writes(command)?.map(w => w.next)
  expect(next('gh pr edit 12 --title New')).toEqual(['board call github.pr-edit --number 12 --title New'])
  expect(next('gh pr comment https://github.com/octo/widgets/pull/7 -b "Looks right"')).toEqual([
    'board call github.comment --number 7 --body "Looks right"',
  ])
  expect(next('gh issue comment 3 --body-file notes.md')).toEqual(['board call github.comment --number 3 --body "$(cat notes.md)"'])
  expect(next("gh issue create -t Wobble -b 'It wobbles.'")).toEqual(["board call github.issue-create --title Wobble --body 'It wobbles.'"])
  expect(next('gh pr create --fill --draft')).toEqual(['board call github.pr-create --title <title> --body <body> --draft true'])
  expect(next('gh api -X PATCH repos/octo/widgets/pulls/3 -f title=x')).toEqual(['board call github.pr-edit --number 3 --title <title>'])
  expect(next('gh api repos/{owner}/{repo}/issues/4/comments -f body=hi')).toEqual(['board call github.comment --number 4 --body <body>'])
})

test('assignments, wrappers and redirections around a push leave the push and its remote', () => {
  expect(writes('GIT_TRACE=1 env git push -q origin main 2>&1 | tail -3')?.map(w => [w.action, w.remote])).toEqual([['push', 'origin']])
  expect(writes('git -c push.default=current push --repo=upstream > log.txt')?.map(w => w.remote)).toEqual(['upstream'])
})

test('reads and other commands make no write and run nothing before the call', async ($, on) => {
  const { runs, ran } = engine(on, {})
  const reads = [
    'git status && git log -1',
    'gh pr view 12',
    'gh api repos/octo/widgets/pulls',
    'gh api -X GET repos/octo/widgets/issues -f state=open',
    "echo 'git push'",
    'cat <<EOF > notes.md\ngit push\nEOF',
    'gh api graphql -f query=\'mutation { x }\'',
  ]
  for (const command of reads) expect(await bash($, command)).toBeNull()
  expect(runs).toEqual([])
  expect(ran).toEqual(reads)
})

test('a gh api write to another repository is refused only when a boundary covers that repository', async ($, on) => {
  const { runs } = engine(on, {})
  expect(await bash($, 'gh api -X DELETE repos/octo/widgets/git/refs/heads/x')).toContain('Board has no action for this write.')
  expect(await bash($, 'gh api -X DELETE repos/someone/else/git/refs/heads/x')).toBeNull()
  expect(runs).toEqual(["query in ''", "query in ''"])
})

test('a board that cannot be read lets the write run', async ($, on) => {
  const { ran } = engine(on, { boundaries: 'fails' })
  expect(await bash($, 'git push origin main')).toBeNull()
  expect(ran).toEqual(['git push origin main'])
})

test('a command the mod cannot parse runs, and the same command parsed is refused', async ($, on) => {
  const { ran } = engine(on, {})
  expect(await bash($, 'git push "origin')).toBeNull()
  expect(ran).toEqual(['git push "origin'])
  expect(await bash($, 'git push "origin"')).toContain('Next: run `board call github.push`')
})

test('cd and git -C move the push, and the refusal names the same directory', async ($, on) => {
  const { runs } = engine(on, {})
  expect(await bash($, 'cd ../widgets && git -C sub push upstream main')).toContain('Next: run `cd ../widgets/sub && board call github.push`.')
  expect(runs).toEqual(["query in '../widgets/sub'", "urls of 'upstream' in '../widgets/sub'"])
  expect(await bash($, 'cd "$TARGET" && git push')).toBeNull()
})

test('a boundary limited to other actions lets a push run and refuses a pull request', async ($, on) => {
  engine(on, { boundaries: [boundary('github:octo/*', ['github.pr-*'])] })
  expect(await bash($, 'git push')).toBeNull()
  expect(await bash($, 'gh pr create -t T -b B')).toContain('Next: run `board call github.pr-create --title T --body B`.')
})

test('a boundary limited by from covers a checkout under its mount and not one elsewhere', async ($, on) => {
  const answers: Answers = { boundaries: [boundary('github:octo/*', ['github.*'], ['/repos/widgets'])] }
  engine(on, answers)
  expect(await bash($, 'git push')).toContain('Next:')
  answers.boundaries = [boundary('github:octo/*', ['github.*'], ['/repos/gears'])]
  expect(await bash($, 'git push')).toBeNull()
})

test('a boundary limited by from covers a checkout no board holds, and not one whose mount is unknown', async ($, on) => {
  const missing = JSON.stringify({ schema: 'board/failure/v1', code: 'board::config::not_found', message: 'no board' })
  const answers: Answers = { boundaries: [boundary('github:octo/*', ['github.*'], ['/repos/gears'])], mount: { exitCode: 1, stdout: missing } }
  engine(on, answers)
  expect(await bash($, 'git push')).toContain('Next:')
  answers.mount = { exitCode: 2, stdout: '' }
  expect(await bash($, 'git push')).toBeNull()
})

test('globs match as the boundary plugin matches them, destinations without case', () => {
  expect(matches('github:octo/widgets', 'github:octo/*')).toBe(true)
  expect(matches('github:octo/widgets', 'github:oct?/[uvw]idgets')).toBe(true)
  expect(matches('github:octo/widgets', 'github:octo/[!w]idgets')).toBe(false)
  expect(matches('github.push', 'github.pr-*')).toBe(false)
  expect(covers({ to: 'GitHub:Octo/*', actions: ['github.push'], from: null }, 'push', 'github:octo/widgets', undefined)).toBe(true)
})

test('the board command is named in one place', () => {
  expect(BOARD_COMMAND).toBe('board call github.')
})
