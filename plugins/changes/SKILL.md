---
name: changes
description: 'Walk one change to a repository from its plan to its pull request, and gather every record it produces. Use it when you start a change to a repository mounted in the board, and whenever you resume one. Start with `board -C <board> call changes.start --repo /repos/<name> --title ''<title>''`, then call `changes.next --path /changes/<name>` for the next step. SKILL.md and README.md in the plugin''s directory say more.'
---

# changes

The gate is the contract. `next` says what the current phase still lacks;
`advance` moves on only when nothing is missing.

## Steps

1. Run `board -C <board> call changes.start --repo /repos/<name> --title '<title>'`.
   Add `--handle @<you>` so the route reflects who owns what. Read every path
   in `docs`. Treat each `ask_first` pattern as a file to ask about before
   changing.
2. Run `board -C <board> call changes.next --path /changes/<name>`
   whenever you start a phase or lose context. Do what `instruction` says
   and fix each check whose `status` is `missing`.
3. Attach every record you make with
   `board -C <board> call changes.attach --path /changes/<name> --ref <path>`.
   Set the commit under test with `--head <sha>` after each push, the issue
   with `--issue <url>`, and the pull request with `--pull-request <url>`.
4. Run `board -C <board> call changes.advance --path /changes/<name>`
   when every check passes.
5. To drop a change, `board put` it with `phase` set to `dropped`.

## Each phase

- **plan**: interview the owner. Use your own planning skill where one is
  configured. Write the plan's Problem, Outcome and Out of scope sections.
  On the `ask-first` route, attach the owner's `plans/decision/v1` whose
  `refs` name the plan.
- **build**: build test-first. Attach each `tasks/task/v2`. Run the check
  suite that `start` reported on the head commit, record the run, and attach
  it.
- **review**: review the head commit with clean context and attach the
  report. Fix every critical or high finding, record the new head, and
  review again.
- **land**: add `## What the tests prove` and `## What the tests do not
  prove` sections to the plan with what the runs cannot show on their own.
  Run `board -C <board> call changes.pr --path /changes/<name>`, read
  `/changes/<name>/pr.md` and edit it if it is wrong. Fix each path in
  `unreadable` and run `pr` again, or say why in the body. Run `command`
  yourself, in a POSIX shell, from a checkout of the repository on the
  change's `head`, then record the pull request's URL. Merging happens
  outside Board. Advancing from `land` ends the change in `done`.

## Refusals

- `changes::gate::unmet`: `missing` names the failed checks. Run `next`.
- `changes::ref::dangling`: nothing is at the path. Write the record
  first.
- `changes::change::exists`: pass another `--name`, or resume the
  change with `next`.
- `changes::repo::not_found`: run `board ls /repos` to find the mount.
- `changes::phase::final`: the change is done or dropped. Run `open`, or
  `start` a new change.
