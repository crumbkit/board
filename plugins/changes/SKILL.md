---
name: changes
description: 'Walk one change to a repository from its plan to its pull request, and gather every record it produces. Use it to start, resume or list work on a repository the board mounts. Start a change with `board -C <board> changes start --mount /repos/<name> --title ''<title>''`, then run `board changes next --path /changes/<name>` for the next step. `board changes open` lists the changes in progress. SKILL.md and README.md in the plugin''s directory say more.'
---

# changes

The gate is the contract. A change follows a flow, a list of phases, each
with an instruction and the checks that gate leaving it. `next` says what
the current phase still lacks; `advance` moves on only when no check that
blocks is missing.

## Steps

1. Every change does a task. Run
   `board -C <board> changes start --mount /repos/<name> --title '<title>' --task <task>`,
   or leave out `--task` to add one titled as the change.
   Add `--handle @<you>` so the route reflects who owns what. Add
   `--flow <name>` to follow another flow than the board's choice, such as
   `small`. Read every path in `docs` and in `surfaced`, and note each
   `obligations` entry. Each entry carries a `close` line that says how to
   discharge that obligation once its work is committed. Treat each `ask_first` pattern as a file to ask
   about before changing.
2. Run `board -C <board> changes next --path /changes/<name>`
   whenever you start a phase or lose context. Do what `instruction` says
   and fix each check whose `status` is `missing`, following its `remedy`.
   When `ask` is set, only the owner or a reviewer can pass the checks
   left: put `ask` to them. Stop where the change's `stop` says, or the
   flow's when it has none, unless told otherwise; once `next` returns
   `stop` with `reached` true, `advance` refuses. With the `board-changes`
   mod installed, an Edit, Write or NotebookEdit of the repository by an
   agent not started from `changes.brief` runs with a line of advice; it
   cannot see writes made through Bash. Start a builder with a prompt
   whose first line is the brief's `brief_line`, followed by the whole
   brief as `changes.brief` prints it now, as plain text, not inside a code
   fence; compact or pretty JSON with keys in any order both pass. The
   mod checks only a prompt that has a `board-brief:` line or starts with
   a rendered brief, so a reviewer's or a planner's prompt that names the
   change is never advised. It advises an Agent call whose brief line is
   not first with `changes::brief::no_line`, and one that holds no current
   brief as a JSON object with `brief_line` with
   `changes::brief::not_rendered`. It never changes the prompt: the advice
   arrives only in your result, after the agent returns; fix the prompt
   for the next builder. A UI change's brief lists its `mockups`, and
   warns `changes::ux::not_agreed` while one lacks the owner's decision.
   The brief's `rules_left_out` gives each rule it left out a
   `reason`: the rule had no room, or it is a version rule whose capture
   finds no version in the planned files. A missing or unreadable planned
   file keeps a version rule. A rule with no room still applies, so read
   it with `rules.for-path`.
3. Attach every record you make with
   `board -C <board> changes attach --path /changes/<name> --ref <path>`.
   The records go on the plan the change names, so write the plan first:
   pipe a JSON object with `path`, `text`, `uses`, `paths` and `answers` to
   `board -C <board> changes plan`. Give the plan's `answers` beside its
   text: `ux` is none or mockups, `data` is none or stored, and `lifecycle`
   is none or record; each section holds the reason. The gates read the
   answers to decide which checks apply, and under a flow whose checks read
   one, `plan` refuses a plan that leaves it out
   (`changes::plan::no_answers`). Records the change surfaced that you
   read and do not rely on go in `considered`, as
   `[{"records": ["<path>"], "why": "<reason>"}]`. Obligations the change
   leaves owed go in `owed`, as `[{"rule": "<path>", "why": "<reason>"}]`.
   Both fields take paths, and the gates do not read paths in the text.
   Keep the plan near 900 words; `plan` refuses one over 3,000.
   Failure paths has one row per step that can fail: what fails, the state
   it leaves, the recovery, and the order of the check against the
   destructive step it guards. Consumers lists every caller and reader of
   what the change produces or alters, and names the files searched.
   Lifecycle says why nothing changes state, or names the record
   `board changes lifecycle` writes and summarises its states in one line.
   When the plan's paths reach `/rules/<repo>/lifecycle-paths`, the gate
   refuses a `lifecycle` answer of none. Write the record by piping `path`, `text` (at most 1,500
   words: the state table, the crash points and the invariants), `doc` (the
   repository file where the table lands) and `external` to
   `board -C <board> changes lifecycle`. The builder's brief names its text.
   Set the commit under test with `--head <sha>` after each push, and the
   check suite with `--suite <path>` when the change names none.
   `board github pr-create --change /changes/<name>`, run from the checkout
   as the github skill says, opens the pull request and attaches it. When
   its output has `change_note`, run the command the note gives. Record
   any other GitHub issue or pull request with
   `board -C <board> github ref --ref <owner>/<repo>#<number>`, then
   attach the path it returns with `--ticket <path>` or `--pull <path>`.
   An owner check (`owner-approval`, `data-signed`, `rechecks`, `mockups`
   and a waiver) counts an attached `plans/decision/v1` only when its
   `by.person` names a person and its `written_by` is empty. A decision the
   plan lists only in `uses` passes no check. An owner check that counts
   such decisions with refs naming the plan or the change, such as
   `owner-approval` or `owner-confirms`, also counts the owner's
   `plans/answer/v1` when its `by.person` or its `written_by` is a string
   that is not empty, as an answer from the questions pane, Board.app or
   /ask is, and the option the owner chose names the change's path, such
   as `/changes/<name>`, in its text. Such a check also counts the owner's
   Touch ID approval of the current plan: run
   `board call changes.approve --path /changes/<name>`, which the pane runs
   after such an answer, and the owner approves the effect it names. Only
   that signed approval proves the owner chose; `by` and `written_by` are
   claims any writer can make. When you ask for approval with /ask, put
   the change's path in the text of the option that approves it and in no
   other option, because any chosen option that names the path counts.
   A path followed by `#` is a longer name: an option naming
   `/changes/<name>#<check id>` waives that waivable check and approves
   nothing. An option naming a mockup's URL or board path agrees it for
   the `mockups` check. The gates read only which paths the chosen option
   names, never what it says, so name the mockup's URL or board path, or
   `/changes/<name>#<check id>`, only in the option that agrees the mockup
   or waives the check. Never name them in an option that rejects or
   revises, and never name the bare change path in the agreeing or
   waiving option, because that option would then approve the plan too.
   Attach the question and the answer, each with
   `board changes attach --path /changes/<name> --ref <path>`. When you
   record the owner's approval from chat, write the decision as the plans
   skill says, with `by.person` and the owner's words in `why`.
   Piping `{"path": "/changes/<name>", "project": "<project>", "by":
   {"session": "<your local_ id>"}}` to `board -C <board> changes ask`
   files and attaches one question whose approving option names the
   change, each data understanding check and each mockup, so one answer
   passes `owner-approval`, `data-signed` and `mockups`. A plan that
   answers `ux` none and `data` none needs no approval. After a change
   lands, `board -C <board> changes owed` lists the rechecks it still owes.
4. Run `board -C <board> changes advance --path /changes/<name>`
   when every check passes. A change that records a pull request moves
   into `done` only once GitHub reports it merged, so land it with
   `board github merge --change /changes/<name>`. To send a change back to build for a fix
   round, such as after a review asks for fixes, run
   `board -C <board> changes reopen --path /changes/<name> --reason '<why>' --agent '<your name>'`.
   Never move its phase with `board put`.
5. To drop a change, run
   `board -C <board> changes drop --path /changes/<name> --reason '<why>'`.
6. When `open` or `next` reports `sync.stale`, run
   `board -C <board> changes sync`. It ends each change whose pull
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
   When `advance`, `resume`, `reopen` or `prototype` refuses with `changes::wip::full`,
   first deal with the changes in `idle`: run each one's `command`, which
   syncs, drops or resumes it. If the work must still start now, run the
   `override` command with the reason in `--over_limit`; the change records
   it and the output carries the warning `changes::wip::over_limit`. Pause new
   work too when more than five items wait on the owner, or one has waited
   more than two days; ask him one question when the ranking is unclear.
   Pause a change with `changes.pause` and resume it with `changes.resume`.

## Prototypes

When the owner wants to try an idea, run
`board -C <board> changes prototype --mount /repos/<name> --question '<question>' --parent <task>`.
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
`board -C <board> changes run --path /changes/<name> [--through <phase> | --until <phase>] --inline true`,
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
every repository's. Each change references a stored version of its flow at
`/flows/<name>/<version>`, so editing a flow moves no change in flight. Run
`board -C <board> changes reflow --path /changes/<name> --flow <name>`
to move a change to another flow, or to take up an edit. To write a flow,
start a change with `--flow flow-authoring --mount /` (or the mount the
flow is for) and follow it: it drafts the flow, checks it with
`flow-valid`, dry-runs it with `changes.run --sandbox true`, which runs an
agent in a throwaway copy of the board, and adopts it at `/flows/<name>`.
Run
`changes.migrate --apply true` once to move every `changes/change/v1` onto
the development flow, and `changes.migrate --flows true --apply true` once
to replace the flow copies changes kept before versions.

## Refusals

- `changes::gate::unmet`: `missing` names the failed checks and `remedies`
  what to do for each. Run `next`.
- `changes::flow::not_found`: name a flow the board holds at
  `/flows/<name>` or a shipped one, or fix the link that chose it.
- `changes::flow::invalid`: fix each of the flow's `problems`, or name
  another flow.
- `changes::mount::needs_repository`: name the repository's mount, or a
  flow without `paths`, `run` or `rule-paths` checks.
- `changes::sandbox::no_change`, `changes::sandbox::inline`,
  `changes::sandbox::resume`, `changes::sandbox::setup`: a sandboxed run
  starts a new change, with `--for` naming the change that asked for it.
- `changes::flow::unreadable`: fix the flow file named, or restore a
  change's flow version with `reflow`.
- `changes::reflow::no_phase`: choose a flow that has the change's phase.
- `changes::reopen::no_build`: the change's flow has no build phase before
  where it stands. Run `changes.next` for its next step.
- `changes::reopen::no_actor`: give `--session`, `--program`, `--person` or
  a non-blank `--agent`.
- `changes::run::stop_reached`: the change is at its stop. Record
  `changes.stop --reason reached` and end the run. The person who started
  the run moves the stop by starting a later run with `--through` or
  `--until`.
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
- `changes::ux::not_agreed`: no owner decision names a mockup, and no
  attached owner answer chose an option naming it, or the plan answers
  `ux` mockups and UX lists none.
- `changes::ux::no_version`: name the agreed version of each mockup in its
  item, such as `version <id>`.
- `changes::ux::not_conformed`: the built page differs from a mockup version,
  or no critic compared it. Fix each listed element, or file a
  `plans/question/v1` for one that cannot be built, then have a clean-context
  critic write a `changes/conformance/v1` for the new head. The newest
  record at the head decides. An owner's answer to the question a difference
  names clears it, and the owner's waiver decision, or answer whose chosen
  option names `<change>#design-conformance`, passes a partial build or a page
  Playwright cannot screenshot.
- `changes::plan::other_change`: the plan's first heading is the title of
  the change in `other`, so the text is most likely that change's plan.
  Send this change's own plan, or make its first heading this change's
  title. The warning `changes::plan::title_differs` says only that the
  heading differs from the title; check the text is this change's plan.
- `changes::plan::no_answers`: send the plan again with the `answers` the
  message names, such as `{"ux": "none", "data": "none", "lifecycle": "none"}`.
- A check whose detail says the plan gives no typed answer: the plan was
  written before plans gave answers; write it again with `answers`.
- `changes::ux::no_neighbours`: add a Neighbours list under UX.
- `changes::plan::no_paths`: give the plan `paths`, the files and patterns
  it will edit.
- `changes::plan::outside_mount`: start each planned path with the change's
  mount and `/`.
- `changes::paths::outside_plan`: the plan has no paths, or git cannot
  list the change's diff. Write the plan with its paths, or fix what the
  detail names. A file the diff edits outside the plan's paths does not
  refuse: the gate lists it as added, and `changes.pr` lists it in the
  pull request body, so do not write the plan again for it.
- `changes::brief::rules_missing`: the Board running the plugin has no
  `rules.for-paths`. Run `board plugin enable rules`.
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
  today, and a change records a pull request that sync reads; run
  `changes.sync`. A change with no pull request gets no such warning.
- `changes::sync::gh_failed`: gh could not read a pull request, and sync wrote
  nothing. Check `gh auth status` and sync again.
- `changes::sync::owed`, `changes::sync::pull_not_found`,
  `changes::sync::pull_unreadable`, `changes::sync::no_pull_request`: entries
  that sync left alone. Run each entry's `command`. For
  `pull_not_found`, check `gh auth status` first, since GitHub answers 404
  for a repository gh cannot see.
- `changes::sync::unread`: an entry sync left alone because a reference of
  the change is beneath a mount that cannot be read. It has no `command`;
  repair the mount as its `detail` says, then sync again.
- `changes::phase::paused`: `resume` the change first.
- `changes::pause::not_building`, `changes::resume::not_paused`: only a
  change in `build` is paused, and only a paused one resumed.
- `changes::prototype::not_prototype`: only a prototype takes a verdict.
- `changes::task::not_a_task_path`: `--parent` names a task under a
  `tasks/` directory, such as `/repos/notes/tasks/<slug>.md`, not
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
- `changes::guard::no_brief`: advice, the edit ran. Start a change and
  build from `changes.brief`.
- `changes::guard::not_building`: advice; `advance` the change to `build`.
- `changes::guard::wrong_mount`: the brief's change is for another
  repository.
- `changes::guard::wrong_branch`: a prototype's builder edits only on
  `proto/<name>`; switch to it.
- `changes::guard::unknown`: the guard could not check. It refuses in a known
  board's checkout and advises elsewhere. Fix what it names.
- `changes::land::not_merged`: GitHub has not reported the change's pull
  request merged, so the change stays out of `done`. Merge it with
  `board github merge --change /changes/<name>`, which moves the change to
  `done` after the merge; drop the change when the pull request closed.
- `changes::land::gh_failed`: gh could not read the pull request, and
  nothing was written. Check `gh auth status` and advance again.
- `changes::phase::final`: the change is done or dropped. Run `open`, or
  `start` a new change. To correct a done or dropped change's plan, send
  the plan again with `"correct": true`; the change keeps its phase.
