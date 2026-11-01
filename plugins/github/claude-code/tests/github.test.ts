import { expect, test } from 'claude-code/testing'
import type { On } from 'claude-code'
import type { Engine } from 'claude-code/testing'

import { BOARD_COMMAND, writes } from '../hooks/command'

// These prove which Bash calls the mod refuses and what the refusal says,
// with board stood in by a process.run hook. They do not run board: the
// Python tests prove what github.covers answers, by the rules the github
// actions, the pre-push hook and the boundary plugin share.

const TO = 'github:octo/widgets'

/** What github.covers answers: the destination it names as covered, null for none, or 'fails'. */
type Covers = (asked: { dir: string; flags: string[] }) => string | null | 'fails'

/** Stands in for board and the Bash tool beneath the mod, and keeps what ran. */
function engine(on: On, answer: { covers: Covers }) {
  const asked: string[] = []
  const ran: string[] = []
  on('process.run', (_, e) => {
    const [dir = '', ...flags] = e.argv.slice(4)
    asked.push(`covers in '${dir}' ${flags.join(' ')}`)
    const to = answer.covers({ dir, flags })
    const covered = to === null || to === 'fails' ? [] : [{ to, boundaries: ['/boundaries/octo'] }]
    const stdout = to === 'fails' ? '' : JSON.stringify({ data: { output: { covered } } })
    return { value: { exitCode: to === 'fails' ? 1 : 0, stdout, stderr: '', isStdoutTruncated: false, isStderrTruncated: false } }
  })
  on('tool.call', (_, e) => {
    ran.push(e.tool === 'Bash' ? e.command : e.tool)
    return { result: { stdout: '', stderr: '', interrupted: false } } as never
  })
  return { asked, ran }
}

const always: Covers = () => TO

const bash = async ($: Engine, command: string) => {
  const answer = await $.tool.call({ tool: 'Bash', command })
  return 'deny' in answer && typeof answer.deny === 'string' ? answer.deny : answer.isError ? String(answer.text) : null
}

test('a git push to a covered repository is refused with the board command, and one to another repository runs', async ($, on) => {
  const answer = { covers: always }
  const { asked, ran } = engine(on, answer)
  expect(await bash($, 'git push')).toBe(
    'This git push sends commits to github:octo/widgets, which a boundary in the personal board covers. Content ' +
      'for it goes through Board, which has the boundary check it first. Next: run `board call github.push`. It ' +
      'pushes the branch checked out to origin.',
  )
  expect(asked).toEqual(["covers in '' --action push"])
  expect(ran).toEqual([])
  answer.covers = () => null
  expect(await bash($, 'git push')).toBeNull()
  expect(ran).toEqual(['git push'])
})

test('gh pr create is refused with github.pr-create carrying the same title and body, here-document and all', async ($, on) => {
  engine(on, { covers: always })
  const command = `gh pr create --base main --title "Add a gear" --body "$(cat <<'EOF'\nDon't break it.\ngit push\nEOF\n)"`
  expect(await bash($, command)).toContain(
    `Next: run \`board call github.pr-create --title "Add a gear" --body "$(cat <<'EOF'\nDon't break it.\ngit push\nEOF\n)" --base main\`.`,
  )
})

test('each gh write names its matching action, number and text', () => {
  const next = (command: string) => writes(command)?.map(w => w.next)
  expect(next('gh pr edit 12 --title New')).toEqual(['board call github.pr-edit --number 12 --title New'])
  expect(next('gh pr edit --remove-milestone 12 --title New')).toEqual(['board call github.pr-edit --number 12 --title New'])
  expect(next('gh pr comment https://github.com/octo/widgets/pull/7 -b "Looks right"')).toEqual([
    'board call github.comment --number 7 --body "Looks right"',
  ])
  expect(next('gh issue comment 3 --body-file notes.md')).toEqual(['board call github.comment --number 3 --body "$(cat notes.md)"'])
  expect(next("gh issue create -t Wobble -b 'It wobbles.'")).toEqual(["board call github.issue-create --title Wobble --body 'It wobbles.'"])
  expect(next('gh pr create --fill --draft')).toEqual(['board call github.pr-create --title <title> --body <body> --draft true'])
  expect(next('gh api -X PATCH repos/octo/widgets/pulls/3 -f title=x')).toEqual(['board call github.pr-edit --number 3 --title <title>'])
  expect(next('gh api repos/{owner}/{repo}/issues/4/comments -f body=hi')).toEqual(['board call github.comment --number 4 --body <body>'])
})

test('gh api reads its own flags: -f takes a field, and -X and -f take a value joined to them', () => {
  const actions = (command: string) => writes(command)?.map(w => [w.action, w.repo])
  expect(actions('gh api -f title=x repos/octo/widgets/issues')).toEqual([['issue-create', 'octo/widgets']])
  expect(actions('gh api -XPOST repos/octo/widgets/issues -ftitle=x')).toEqual([['issue-create', 'octo/widgets']])
  expect(actions('gh api -XGET repos/octo/widgets/issues -f state=open')).toEqual([])
})

test('gh commands with no matching action that send text are refused, and the same commands without text run', async ($, on) => {
  const { ran } = engine(on, { covers: always })
  const sending = ['gh pr merge 3 --squash --body "Merged"', 'gh issue close 4 -c "Done"', 'gh release create v1 --notes x', 'gh pr review 3 --approve -b ok']
  for (const command of sending) expect(await bash($, command)).toContain('Board has no action for this write.')
  expect(await bash($, 'gh pr merge 3 --squash')).toBeNull()
  expect(await bash($, 'gh issue close 4')).toBeNull()
  expect(ran).toEqual(['gh pr merge 3 --squash', 'gh issue close 4'])
})

test('assignments, wrappers and redirections around a push leave the push and its remote', () => {
  expect(writes('GIT_TRACE=1 env git push -q origin main 2>&1 | tail -3')?.map(w => [w.action, w.remote])).toEqual([['push', 'origin']])
  expect(writes('git -c push.default=current push --repo=upstream > log.txt')?.map(w => w.remote)).toEqual(['upstream'])
})

test('another git directory or work tree leaves the push unread, and the same push in place is read', () => {
  expect(writes('git --git-dir=../other/.git push')?.map(w => w.dir)).toEqual([null])
  expect(writes('GIT_DIR=../x git push')?.map(w => w.dir)).toEqual([null])
  expect(writes('git push')?.map(w => w.dir)).toEqual([''])
})

test('a cd inside a subshell moves only the commands inside it', () => {
  expect(writes('(cd ../other && git push) && git push')?.map(w => w.cd)).toEqual(['../other', ''])
})

test('a repository on another GitHub host is not read, and one on github.com is', () => {
  expect(writes('gh pr create -R github.example.com/octo/widgets -t T -b B')).toEqual([])
  expect(writes('gh pr create -R github.com/octo/widgets -t T -b B')?.map(w => w.repo)).toEqual(['octo/widgets'])
})

test('a pull request from another branch says the board command opens it from the branch checked out', async ($, on) => {
  engine(on, { covers: always })
  expect(await bash($, 'gh pr create --head other -t T -b B')).toContain(
    'Next: run `board call github.pr-create --title T --body B`. It opens the pull request from the branch checked out, not from other.',
  )
})

test('reads and other commands make no write and run nothing before the call', async ($, on) => {
  const { asked, ran } = engine(on, { covers: always })
  const reads = [
    'git status && git log -1',
    'gh pr view 12',
    'gh api repos/octo/widgets/pulls',
    'gh api -X GET repos/octo/widgets/issues -f state=open',
    "echo 'git push'",
    'cat <<EOF > notes.md\ngit push\nEOF',
    "gh api graphql -f query='mutation { x }'",
  ]
  for (const command of reads) expect(await bash($, command)).toBeNull()
  expect(asked).toEqual([])
  expect(ran).toEqual(reads)
})

test('a gh api write names its repository to github.covers', async ($, on) => {
  const { asked } = engine(on, { covers: ({ flags }) => (flags.includes('octo/widgets') ? TO : null) })
  expect(await bash($, 'gh api -X DELETE repos/octo/widgets/git/refs/heads/x')).toContain('This gh api call writes')
  expect(await bash($, 'gh api -X DELETE repos/someone/else/git/refs/heads/x')).toBeNull()
  expect(asked).toEqual(["covers in '' --action api --repo octo/widgets", "covers in '' --action api --repo someone/else"])
})

test('a github.covers that fails lets the write run', async ($, on) => {
  const { ran } = engine(on, { covers: () => 'fails' })
  expect(await bash($, 'git push origin main')).toBeNull()
  expect(ran).toEqual(['git push origin main'])
})

test('a command the mod cannot parse runs, and the same command parsed is refused', async ($, on) => {
  const { ran } = engine(on, { covers: always })
  expect(await bash($, 'git push "origin')).toBeNull()
  expect(ran).toEqual(['git push "origin'])
  expect(await bash($, 'git push "origin"')).toContain('Next: run `board call github.push`')
})

test('cd and git -C move the push, and the refusal names the same directory', async ($, on) => {
  const { asked } = engine(on, { covers: always })
  expect(await bash($, 'cd ../widgets && git -C sub push upstream main')).toContain('Next: run `cd ../widgets/sub && board call github.push`.')
  expect(asked).toEqual(["covers in '../widgets/sub' --action push --remote upstream"])
  expect(await bash($, 'cd "$TARGET" && git push')).toBeNull()
})

test('the board command is named in one place', () => {
  expect(BOARD_COMMAND).toBe('board call github.')
})
