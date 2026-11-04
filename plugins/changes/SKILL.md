---
name: changes
description: 'Walk one change to a repository from its plan to its pull request, and gather every record it produces. Use it when you start a change to a repository mounted in the board, and whenever you resume one. Start with `board -C <board> call changes.start --mount /repos/<name> --title ''<title>''`, then call `changes.next --path /changes/<name>` for the next step. SKILL.md and README.md in the plugin''s directory say more.'
---

# changes

The gate is the contract. `next` says what the current phase still lacks;
`advance` moves on only when nothing is missing.

## Steps

1. Every change does a task. Run
   `board -C <board> call changes.start --mount /repos/<name> --title '<title>' --task <task>`,
   or leave out `--task` to add one titled as the change.
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
   When the board holds the ticket or pull request as a record, attach it
   with `--ticket <path>` or `--pull <path>`.
4. Run `board -C <board> call changes.advance --path /changes/<name>`
   when every check passes.
5. To drop a change, run
   `board -C <board> call changes.drop --path /changes/<name> --reason '<why>'`.
6. When `open` or `next` reports `sync.stale`, run
   `board -C <board> call changes.sync`. It ends each change whose pull
   request closed: `done` when it merged, `dropped` when it closed without
   merging. Add `--dry_run true` to see what it would do first. Close the
   task of each change it ends. An entry with `changes::sync::owed` still
   has open tasks; close them and sync again.
7. At most three active build changes per mount are in `build`, and apart
   from them three active prototypes exploring. A change is active when a
   changes capability touched it, its head was committed or a run it
   references finished on one of the last three days, today included
   (`BOARD_CHANGES_ACTIVE_DAYS` sets the number).
   When `advance`, `resume` or `prototype` refuses with `changes::wip::full`,
   first deal with the changes in `idle`: run each one's `command`, which
   syncs, drops or resumes it. If the work must still start now, run the
   `override` command with the reason in `--over_limit`; the change records
   it and the output carries the warning `changes::wip::over_limit`. Pause new
   work too when more than five items wait on the owner, or one has waited
   more than two days; ask him one question when the ranking is unclear.
   Pause a change with `changes.pause` and resume it with `changes.resume`.

## Prototypes

When the owner wants to try an idea, run
`board -C <board> call changes.prototype --mount /repos/<name> --question '<question>' --parent <task>`.
It adds the task, writes the change in `build` and returns its brief. Start
one builder from the brief; the owner iterates with it directly. The builder
commits and pushes to `proto/<name>` after every round, and may edit only
there. When the owner has his answer, ask him for the verdict in one
sentence and the artifact version he means, and record it with
`changes.verdict --path /changes/<name> --state settled --verdict '<sentence>' --artifact <url>`.
The verdict also closes the prototype's task. Ask again when a prototype
has had no new round for three days. Record
`abandoned` only when the owner says so. `changes.open --kind prototype`
lists every prototype with its verdict.

## Each phase

- **plan**: interview the owner. Use your own planning skill where one is
  configured. Write the plan's Problem, Outcome, Out of scope, UX and Data
  sections. UX starts with "No UI change" and a reason, or lists each
  mockup by URL or board path under a `### Mockups` heading, and holds a
  `### Neighbours` list of the open changes and
  artifacts in the same area and the components the design reuses. Attach
  the owner's decision that names each mockup in its `urls` or `refs`; a
  decision an agent wrote, with `written_by` set, does not count.
  Data starts with "None", or holds the data shapes, where each is stored,
  who reads and writes it and what users see; then run the
  `understanding-check` skill and attach its record and the owner's
  decision that references it. Write the plan with `changes.plan`, whose
  `paths` lists every file and pattern the change will edit, by mounted
  path; the brief lists the rules for those paths.
  On the `ask-first` route, attach the owner's `plans/decision/v1` whose
  `refs` name the plan.
- **build**: start each builder in a fresh agent whose first message
  begins with `brief_line` from `changes.brief`, then the brief. With the
  `board-changes` mod installed, no other agent can Edit, Write or
  NotebookEdit the repository; it cannot see writes made through Bash.
  Build test-first. Attach each `tasks/task/v2`. Run the check
  suite that `start` reported on the head commit, record the run, and attach
  it. Before advancing, ask the owner two of each understanding check's
  predict or edge questions again, worded differently, and add them to the
  record's `rechecks`. The build gate refuses until each record holds two
  rechecks that matched.
- **review**: review the head commit with clean context and attach the
  report. The gate refuses a diff that edits a file outside the plan's
  paths; add the file to the paths with `changes.plan`, or revert the
  edit. When the plan says "No UI change" or "None", list the
  repository's UI-path and data-path controls with `controls.check`, and
  report a diff that touches a path they name as a high finding. Fix
  every critical or high finding, record the new head, and review again.
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
- `changes::ux::not_agreed`: a mockup has no owner decision naming it, or
  UX lists none and does not say "No UI change".
- `changes::ux::no_neighbours`: add a Neighbours list under UX.
- `changes::plan::no_paths`: give the plan `paths`, the files and patterns
  it will edit.
- `changes::plan::outside_mount`: start each planned path with the change's
  mount and `/`.
- `changes::paths::outside_plan`: the diff edits the files named outside
  the plan's paths. Add them with `changes.plan`, or revert the edit.
- `changes::brief::controls_missing`: the Board running the plugin has no
  `controls.for-paths`. Run `board plugin enable controls`.
- `changes::data::not_signed`: run the understanding check and attach its
  record and the sign-off decision.
- `changes::prototype::never_lands`: a prototype ends with `verdict`.
- `changes::wip::full`: `active` lists the changes holding places for the
  mount and `idle` the idle ones with a `command` each. Deal with the idle
  ones, pause one, wait, or run `override` with a reason.
- `changes::wip::over_limit`: a warning, not a refusal. The change went past
  the limit with the reason it records in `over_limit`.
- `changes::wip::bad_active_days`: set `BOARD_CHANGES_ACTIVE_DAYS` to a whole
  number of days, or unset it.
- `changes::sync::stale`: a warning in `open` and `next`. No sync has run
  today; run `changes.sync`.
- `changes::sync::gh_failed`: gh could not read a pull request, and sync wrote
  nothing. Check `gh auth status` and sync again.
- `changes::sync::owed`, `changes::sync::pull_not_found`,
  `changes::sync::not_github`, `changes::sync::no_pull_request`: entries
  that sync left alone. Run each entry's `command`. For
  `pull_not_found`, check `gh auth status` first, since GitHub answers 404
  for a repository gh cannot see.
- `changes::phase::paused`: `resume` the change first.
- `changes::pause::not_building`, `changes::resume::not_paused`: only a
  change in `build` is paused, and only a paused one resumed.
- `changes::prototype::not_prototype`: only a prototype takes a verdict.
- `changes::task::not_a_task_path`: `--parent` names a task under a
  `tasks/` directory, such as `/repos/notes-personal/tasks/<slug>.md`, not
  a project brief.
- `changes::task::unrouted`: name the task with `--task`, or the repo to
  add it to with `--task-repo`.
- `changes::ref::dangling`: nothing is at the path. Write the record
  first.
- `changes::ref::wrong_type`: the ticket or pull is not a record of a type
  its list accepts. Point it at a `jira/issue/v1`, `github/issue/v1` or
  `github/pull/v1` record.
- `changes::change::exists`: pass another `--name`, or resume the
  change with `next`.
- `changes::mount::not_found`: run `board ls /repos` to find the mount.
- `changes::guard::no_brief`: this agent did not start from a brief. Hand
  the edit to a builder started from `changes.brief`.
- `changes::guard::not_building`: `advance` the change to `build`.
- `changes::guard::wrong_mount`: the brief's change is for another
  repository.
- `changes::guard::wrong_branch`: a prototype's builder edits only on
  `proto/<name>`; switch to it.
- `changes::guard::unknown`: the guard could not check; fix what it names.
- `changes::phase::final`: the change is done or dropped. Run `open`, or
  `start` a new change.
