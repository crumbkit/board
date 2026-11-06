---
name: changes
description: 'Walk one change to a repository from its plan to its pull request, and gather every record it produces. Use it when you start a change to a repository mounted in the board, and whenever you resume one. Start with `board -C <board> call changes.start --mount /repos/<name> --title ''<title>''`, then call `changes.next --path /changes/<name>` for the next step. SKILL.md and README.md in the plugin''s directory say more.'
---

# changes

The gate is the contract. A change follows a flow, a list of phases, each
with an instruction and the checks that gate leaving it. `next` says what
the current phase still lacks; `advance` moves on only when no check that
blocks is missing.

## Steps

1. Every change does a task. Run
   `board -C <board> call changes.start --mount /repos/<name> --title '<title>' --task <task>`,
   or leave out `--task` to add one titled as the change.
   Add `--handle @<you>` so the route reflects who owns what. Add
   `--flow <name>` to follow another flow than the board's choice, such as
   `small`. Read every path in `docs` and in `surfaced`, and note each
   `obligations` entry. Treat each `ask_first` pattern as a file to ask
   about before changing.
2. Run `board -C <board> call changes.next --path /changes/<name>`
   whenever you start a phase or lose context. Do what `instruction` says
   and fix each check whose `status` is `missing`, following its `remedy`.
   When `ask` is set, only the owner or a reviewer can pass the checks
   left: put `ask` to them. Stop where the change's `stop` says, or the
   flow's when it has none, unless told otherwise; once `next` returns
   `stop` with `reached` true, `advance` refuses. With the `board-changes` mod installed, only a builder started
   from `changes.brief` can Edit, Write or NotebookEdit the repository; it
   cannot see writes made through Bash.
3. Attach every record you make with
   `board -C <board> call changes.attach --path /changes/<name> --ref <path>`.
   The records go on the plan the change names, so write the plan first.
   Set the commit under test with `--head <sha>` after each push, and the
   check suite with `--suite <path>` when the change names none. Record a
   GitHub issue or pull request with
   `board -C <board> call github.ref --ref <owner>/<repo>#<number>`, then
   attach the path it returns with `--ticket <path>` or `--pull <path>`.
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
7. At most three active build changes per repository are in `build`, and
   apart from them three active prototypes exploring. Mounts of one
   repository count together; `repository` in the refusal names it, and
   `unread` lists any mount whose origin git could not read. A change is
   active when a
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

## Runs

To run a change to a stop, run
`board -C <board> call changes.run --path /changes/<name> [--through <phase> | --until <phase>] --inline true`,
or start one with `--mount` and `--title` in place of `--path`. It records
the stop and returns the run brief; follow its `steps` and `stopping`.
Without `--inline`, it starts the agent `BOARD_AGENT` or `agent` in
`~/.config/board/config.json` names, in a worktree for the change, and
records the run. An agent running unattended ends with
`changes.stop --path /changes/<name> --reason <reason> --summary '<line>'`:
`reached` at the stop; `waiting` after writing the `ask` as a
`plans/question/v1` whose `blocks` names the change, passed as
`--question`; `refused`, `budget` or `error` otherwise. Then it exits 0.

## Flows

The plugin ships `development`, `small` and `flow-authoring`; a board's
own flow at `/flows/<name>` wins over a shipped one. The links
`/flows/repos/<name>` and `/flows/default` choose a repository's flow and
every repository's. Each change keeps a copy, so editing a flow moves no
change in flight. Run
`board -C <board> call changes.reflow --path /changes/<name> --flow <name>`
to move a change to another flow, or to take up an edit. To write a flow,
start a change with `--flow flow-authoring --mount /` (or the mount the
flow is for) and follow it: it drafts the flow, checks it with
`flow-valid`, dry-runs it with `changes.run --sandbox true`, which runs an
agent in a throwaway copy of the board, and adopts it at `/flows/<name>`.
Run
`changes.migrate --apply true` once to move every `changes/change/v1` onto
the development flow.

## Refusals

- `changes::gate::unmet`: `missing` names the failed checks and `remedies`
  what to do for each. Run `next`.
- `changes::flow::not_found`: name a flow the board holds at
  `/flows/<name>` or a shipped one, or fix the link that chose it.
- `changes::flow::invalid`: fix each of the flow's `problems`, or name
  another flow.
- `changes::mount::needs_repository`: name the repository's mount, or a
  flow without `paths`, `run` or `controlled-paths` checks.
- `changes::sandbox::no_change`, `changes::sandbox::inline`,
  `changes::sandbox::resume`, `changes::sandbox::setup`: a sandboxed run
  starts a new change, with `--for` naming the change that asked for it.
- `changes::flow::unreadable`: fix the flow file named, or restore a
  change's copy with `reflow`.
- `changes::reflow::no_phase`: choose a flow that has the change's phase.
- `changes::run::stop_reached`: the change is at its stop. Record
  `changes.stop --reason reached`, or move the stop with the `run` command
  in the message.
- `changes::run::no_phase`, `changes::run::two_stops`,
  `changes::run::which_change`, `changes::run::no_flow`: give one stop
  among the flow's phases or `done`, and name one change with a flow.
- `changes::agent::not_set`, `changes::agent::invalid`,
  `changes::agent::not_started`: set `BOARD_AGENT` or `agent` in
  `~/.config/board/config.json` to an argv that starts, or pass
  `--inline true`.
- `changes::run::worktree`: fix the clone the mount reads, or pass
  `--inline true`.
- `changes::change::not_migrated`: run `changes.migrate --apply true`.
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
  `changes::sync::pull_unreadable`, `changes::sync::no_pull_request`: entries
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
- `changes::attach::no_plan`: the change names no plan record to keep the
  reference. Write its plan with `changes.plan`, then attach again.
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
