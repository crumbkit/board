---
name: loop
description: 'Walk one change to a repository from shaping the request to learning from it, and gather every record it produces. Use it when you start a change to a repository mounted in the board, and whenever you resume one. Start with `board -C <board> call loop.start --repo /repos/<name> --title ''<title>''`, then call `loop.next --path /changes/<name>` for the next step. SKILL.md and README.md in the plugin''s directory say more.'
---

# loop

The gate is the contract. `next` says what the current phase still lacks;
`advance` moves on only when nothing is missing.

## Steps

1. Run `board -C <board> call loop.start --repo /repos/<name> --title '<title>'`.
   Add `--handle @<you>` so the route reflects who owns what. Read every path
   in `docs`. Treat each `ask_first` pattern as a file to ask about before
   changing.
2. Run `board -C <board> call loop.next --path /changes/<name>`
   whenever you start a phase or lose context. Do what `instruction` says
   and fix each check whose `status` is `missing`.
3. Attach every record you make with
   `board -C <board> call loop.attach --path /changes/<name> --ref <path>`.
   Set the commit under test with `--head <sha>` after each push, the issue
   with `--issue <url>`, and the pull request with `--pull-request <url>`.
4. Run `board -C <board> call loop.advance --path /changes/<name>`
   when every check passes.
5. To drop a change, `board put` it with `phase` set to `dropped`.

## Each phase

- **shape**: interview the owner. Use your own planning skill where one is
  configured. Write the plan's Problem, Outcome and Out of scope sections.
  Record each question as a `plans/question/v1` and attach it.
- **plan**: have the plan reviewed with clean context and attach the
  `review/report/v1`. On the `ask-first` route, attach the owner's
  `plans/decision/v1` whose `refs` name the plan.
- **build**: build test-first. Attach each `tasks/task/v1`. Run the check
  suite that `start` reported on the head commit, record the run, and attach
  it.
- **review**: review the head commit with clean context and attach the
  report. Fix every critical or high finding, record the new head, and
  review again.
- **land**: open the pull request and record its URL. Merging happens
  outside Board.
- **learn**: record what to improve as notes or tasks and attach them, or
  decide there is nothing to record.

## Refusals

- `loop::gate::unmet`: `missing` names the failed checks. Run `next`.
- `loop::ref::dangling`: nothing is at the path. Write the record
  first.
- `loop::change::exists`: pass another `--name`, or resume the
  change with `next`.
- `loop::repo::not_found`: run `board ls /repos` to find the mount.
- `loop::phase::final`: the change is done or dropped. Run `open`, or
  `start` a new change.
