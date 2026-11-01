---
name: github
description: 'Record a GitHub pull request or issue as a typed record that other records can reference. Use it when a record should name a pull request or issue. Run `board -C <board> call github.ref --ref <owner>/<repo>#<number>`, then reference the `path` it returns. SKILL.md and README.md in the plugin''s directory say more.'
---

# github

A pull request or issue that a record names should be a reference to its
`github/pull/v1` or `github/issue/v1` record, so `board check` can follow it.

## Steps

1. Run `board -C <board> call github.ref --ref <owner>/<repo>#<number>`,
   such as `--ref octo/widgets#12`. You need not know whether the number is
   a pull request or an issue.
2. Put the `path` it returns, such as `/gh/octo/widgets/pulls/12`, in the
   record that names it.
3. Running it again is safe. When the record is already there, `written` is
   false and gh is not run.

## Refusals

- `board::plugin::bad_input`: the reference is not `owner/repo#number`.
- `github::ref::not_found`: GitHub has no such number in the repository, or
  gh's account cannot see it. Check it with `gh issue view`.
- `github::gh::failed`: gh is missing, signed out or failed. Run
  `gh auth status`.
- `github::record::degraded`: the record already there fails its schema.
  Run `board check` on its path and fix it.
