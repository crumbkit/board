---
name: github
description: 'Record a GitHub pull request or issue as a typed record, and push, open or edit pull requests, comment and open issues only after the boundary passes what they send, and merge pull requests once the land gate of their change passes. Use it to name a pull request or issue in a record, and in place of git push and gh for sending. Run `board github ref --ref <owner>/<repo>#<number>`, or `board github push`, `pr-create`, `pr-edit`, `comment`, `issue-create` or `merge` from the checkout. SKILL.md and README.md in the plugin''s directory say more.'
---

# github

A pull request or issue that a record names should be a reference to its
`github/pull/v1` or `github/issue/v1` record, so `board check` can follow it.
Content you send to GitHub should go through this plugin's actions, so the
boundary checks it first.

## Record a pull request or issue

1. Run `board -C <board> github ref --ref <owner>/<repo>#<number>`,
   such as `--ref octo/widgets#12`. You need not know whether the number is
   a pull request or an issue.
2. Put the `path` it returns, such as `/gh/octo/widgets/pulls/12`, in the
   record that names it.
3. Running it again is safe. When the record is already there, `written` is
   false and gh is not run.

## Send to GitHub

Run each action from inside the checkout. From a checkout no board mounts,
add `--personal` after `board`.

- Push the branch checked out: `board github push`.
- Push and open a pull request:
  `board github pr-create --title <title> --body <body>`, with
  `--base <branch>` or `--draft true` when needed.
- Change a pull request: `board github pr-edit --number <n> --title <title>`,
  or `--body <body>`.
- Comment: `board github comment --number <n> --body <body>`.
- Open an issue: `board github issue-create --title <title> --body <body>`.
- Squash-merge the pull request of a change in its land phase:
  `board github merge --change /changes/<name>`. It merges the change's
  head and moves the change to its next phase. It also fetches the branch
  the change's mount follows and moves the mount to the merged commit, so
  do not run `board mount update` after it. When the output has
  `mount_warning`, the pull request merged but the mount was left at its
  pin. Run the warning's `command` as it is; it is there only when it can
  run unchanged. Without one, follow the message. The pull request is the
  change's only one in this repository, and the commit takes the pull
  request's title and body; `--number`, `--subject` and `--body` override.
  It reads the change in the board the command runs in: for a change in
  another board, add `-C <that board's directory>` and name it as
  `/changes/<name>`, never through a mount. From the
  checkout of a repository that another board mounts, a change in the
  personal board needs `--personal`. Without a change, pass `--head <sha>`,
  the full 40-hex commit. When `queued` is true, a merge queue holds the
  pull request: after it merges, run `board changes advance --path
  <change>` in the same board, then the `mount_warning` command, if any.

On `github::boundary::refused`, nothing was sent. Read `fields.failures`, the
message, and the hint after it, which is the boundary's `Next:` step. Change
what it names, such as a commit message with `git commit --amend` or the body
you pass, and run the action again. A boundary is a record the owner keeps,
so ask before you change one. A refusal files no request for the owner; pass
`--ask_owner true` on `push`, `pr-create`, `pr-edit`, `comment` or
`issue-create` only when the exact content must go as it is.

## Raw git push and gh

Do not send to GitHub with `git push` or `gh` directly. In Claude Code, the
`board-github` mod refuses them when a boundary covers the repository, and
its message ends with a `Next:` step naming the `board call github.<action>`
that sends the same content. Run that command. When you start a subagent
whose prompt holds such a command in code, the mod adds
`github::brief::covered` advice to the subagent's prompt and to your
result; name the `board call github.<action>` in the next prompt instead.

To check pushes made outside Board too, such as from a terminal, run
`board github install-hook` in the checkout once. It writes nothing: it
prints the `pre-push` hook, which has the boundary check what each push
sends, inside the shell command that installs it. Show the owner the
command and run it with their approval. When the hook refuses a push, read
its message, change what it names, and push again.

## Refusals

- `board::plugin::bad_input`: the reference is not `owner/repo#number`, or
  `pr-edit` was given neither a title nor a body.
- `github::ref::not_found`: GitHub has no such number in the repository, or
  gh's account cannot see it. Check it with `gh issue view`.
- `github::boundary::refused`: the boundary refused the content; see above.
- `github::board::refused`: the check could not run, such as when no
  personal board is named. Enable boundary and rules in the personal
  board.
- `github::checkout::not_found`, `github::remote::not_github`,
  `github::checkout::detached`: run the action in a checkout on a branch
  whose origin is GitHub.
- `github::push::failed`: git refused the push. Pull or rebase, then run it
  again.
- `github::gh::failed`: gh is missing, signed out or failed, including the
  `gh api` read of a pull request that `merge` makes for a left-out subject
  or body. Run
  `gh auth status`. After a check passed, look on GitHub before you run the
  action again, since the content may have been sent.
- `github::merge::change`: the `--change` path is not under `/changes/`.
  Run the merge in the board that holds the change, with `-C`.
- `github::merge::gate`: the change is not in its land phase, or its land
  gate misses the checks in `fields.missing`. Run `board changes next`.
- `github::merge::number`: no `--number`, and no change that records exactly
  one pull request of this repository. Pass `--number <n>`.
- `github::merge::pull`: GitHub has no pull request `--number`, or it has
  no title, so there is no subject or body to default to. Check
  `--number`, or pass `--subject` and `--body`.
- `github::merge::head`: no head to merge, or `--head` differs from the
  change's head. Attach the head to the change or pass the right one.
- `github::merge::advance`: the pull request merged but the change did not
  move. Do not merge again; run `board changes advance --path <change>` in
  the board the merge ran in.
- `github::hook::exists`: the checkout already has a different `pre-push`
  hook. Ask the owner before you move it aside.
- `github::record::degraded`: the record already there fails its schema.
  Run `board check` on its path and fix it.
