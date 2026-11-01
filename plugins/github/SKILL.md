---
name: github
description: 'Record a GitHub pull request or issue as a typed record, and push, open or edit pull requests, comment and open issues only after the boundary passes what they send. Use it to name a pull request or issue in a record, and in place of git push and gh for sending. Run `board call github.ref --ref <owner>/<repo>#<number>`, or `board call github.push`, `github.pr-create`, `github.pr-edit`, `github.comment` or `github.issue-create` from the checkout. SKILL.md and README.md in the plugin''s directory say more.'
---

# github

A pull request or issue that a record names should be a reference to its
`github/pull/v1` or `github/issue/v1` record, so `board check` can follow it.
Content you send to GitHub should go through this plugin's actions, so the
boundary checks it first.

## Record a pull request or issue

1. Run `board -C <board> call github.ref --ref <owner>/<repo>#<number>`,
   such as `--ref octo/widgets#12`. You need not know whether the number is
   a pull request or an issue.
2. Put the `path` it returns, such as `/gh/octo/widgets/pulls/12`, in the
   record that names it.
3. Running it again is safe. When the record is already there, `written` is
   false and gh is not run.

## Send to GitHub

Run each action from inside the checkout. From a checkout no board mounts,
add `--personal` after `board`.

- Push the branch checked out: `board call github.push`.
- Push and open a pull request:
  `board call github.pr-create --title <title> --body <body>`, with
  `--base <branch>` or `--draft true` when needed.
- Change a pull request: `board call github.pr-edit --number <n> --title <title>`,
  or `--body <body>`.
- Comment: `board call github.comment --number <n> --body <body>`.
- Open an issue: `board call github.issue-create --title <title> --body <body>`.

On `github::boundary::refused`, nothing was sent. Read `fields.failures` and
the message, which ends with the boundary's `Next:` step, also in
`fields.recovery.text`. Change what it names, such as a commit message with
`git commit --amend` or the body you pass, and run the action again. A
boundary is a record the owner keeps, so ask before you change one.

## Refusals

- `board::plugin::bad_input`: the reference is not `owner/repo#number`, or
  `pr-edit` was given neither a title nor a body.
- `github::ref::not_found`: GitHub has no such number in the repository, or
  gh's account cannot see it. Check it with `gh issue view`.
- `github::boundary::refused`: the boundary refused the content; see above.
- `github::board::refused`: the check could not run. Enable boundary and
  controls in the personal board.
- `github::checkout::not_found`, `github::remote::not_github`,
  `github::checkout::detached`: run the action in a checkout on a branch
  whose origin is GitHub.
- `github::push::failed`: git refused the push. Pull or rebase, then run it
  again.
- `github::gh::failed`: gh is missing, signed out or failed. Run
  `gh auth status`. After a check passed, look on GitHub before you run the
  action again, since the content may have been sent.
- `github::record::degraded`: the record already there fails its schema.
  Run `board check` on its path and fix it.
