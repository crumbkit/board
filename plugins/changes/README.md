# changes

A Board plugin, built into Board as `crates/board-plugin-changes`, that walks an agent through one change to a mounted
repository, from its plan to its pull request, and gathers every record the
change produces. SKILL.md holds the procedure; each flow's phases hold
their own instructions, which `next` returns.

## Records

`changes/change/v2` at `/changes/<name>.json` holds the change, the flow it
follows and its phase. The records it owns sit in `/changes/<name>/`: its
copy of its flow at `flow`, its plans, a prototype's record and its
understanding checks. Git history holds earlier phases.

```json
{
  "$type": "changes/change/v2",
  "title": "Let a suite declare a timeout",
  "mount": "/repos/board",
  "flow": "/changes/suite-timeout/flow",
  "phase": "build",
  "suite": "/suites/board-gates",
  "surfaced": ["/repos/notes-personal/tasks/retry.md"],
  "route": "owner",
  "plan": "/changes/suite-timeout/plans/20261101T120000000000Z",
  "task": "/repos/notes-personal/tasks/suite-timeout.md",
  "head": "a3f9c0d",
  "tickets": ["/jira/acme/ACME-123"],
  "pulls": ["/gh/octo/widgets/pulls/7"]
}
```

- `title` and `phase` are required. `mount` is left out for a change on
  the board itself, which `start --mount /` starts.
- `flow` references the change's copy of the `changes/flow/v1` it follows,
  which `start` writes. A prototype follows no flow and has none.
- `phase` is a phase of the flow, `done`, or `dropped` to end a change
  early. `paused` holds a change the keeper took out of `build`.
- `suite` references the `runs/suite/v1` whose runs the `run` check
  counts. `start --suite` or the repository's contributing docs set it, and
  `attach --suite` changes it.
- `surfaced` lists the open tasks, decisions and evals on the mount when
  the change started, which the `considered` check reads.
- `stop`, when set, says where a run of the change stops, in place of the
  flow's `stop`. `run` sets it; "Runs" below says what it holds back.
- `mount` references the repository's `board/mount/v1` record, and `plan`
  the change's current `changes/plan/v1`, which `changes.plan` sets to
  the plan it writes. `board check` reports a reference
  that reaches nothing (`board::ref::dangling`) or a record of another type
  (`board::ref::wrong_type`).
- `tickets` references the `jira/issue/v1` or `github/issue/v1` records the
  change works on, and `pulls` its `github/pull/v1` records. The jira and
  github plugins write those records; `board call github.ref --ref
  owner/repo#number` writes a GitHub one. `board refs <ticket>` lists the
  changes that reference a ticket.
- `task` references the `tasks/task/v2` the change does. `start` always
  sets it; the development flow's `task` check needs it.
- `head` is the commit under test and review.
- `touched` is the UTC day, `YYYY-MM-DD`, on which `start`, `run`,
  `attach`, `advance`, `plan`, `resume` or `prototype` last wrote the change. The WIP
  limit reads it. `sync`, `drop`, `pause` and `verdict` leave it as it is.
- `over_limit` is the reason given for the latest move into `build` past
  the WIP limit, which `pr` puts in the pull request body.
- `merge_commit_url` is the merge commit's GitHub page, which `sync` sets
  when the pull request merged. `dropped_reason` is why the change was
  dropped, which `sync` and `drop` set. `next` shows both.
- `route`: `owner` (nobody else owns a CODEOWNERS pattern), `ask-first`
  (someone else does) or `unowned` (no CODEOWNERS pattern names anyone).

`changes/plan/v1` at `/changes/<name>/plans/<stamp>.json`, beside its text
at `<stamp>.md`. The plan the change's `plan` names is its current plan,
and every reader of the plan, the gates and `brief` among them, reads
that one record.

```json
{
  "$type": "changes/plan/v1",
  "text": "/changes/suite-timeout/plans/20261101T120000000000Z.md",
  "uses": ["/repos/board/STANDARDS.md", "/controls/rules/pair-refusals", "/evals/board-standards-selection"],
  "paths": ["/repos/board/plugins/changes/**", "/repos/board/docs/plugins.md"],
  "records": ["/suites/board-tests/runs/20261025T101500Z-a3f9"]
}
```

- `text`, each of `uses` and each of `records` are references. `uses`
  names the standards documents, controls, decisions, evals and other
  changes the plan relies on.
- `paths` holds the files and SQLite `GLOB` patterns, by logical path, that
  the change will edit, in which `*` also matches `/`. A build change's plan
  always has them; a prototype's plan has none.
- `records` lists the runs, reviews, decisions and other records `attach
  --ref` attached while this plan was the change's plan. The records a change owns
  are those every one of its plans lists.

`changes/prototype/v1` at `/changes/<name>/prototype.json` makes the change
a prototype, and its path names the change. A prototype answers
`question` on branch `proto/<name>` and never lands. `verdict`, one
sentence, ends it in `done` when settled or in `dropped` when abandoned;
`verdict_artifact` is the URL of the artifact version the verdict means.
The change's phase gives the prototype's state, which `open` reports:
`settled` when done, `abandoned` when dropped and `exploring` otherwise.
A change with nothing at `<path>/prototype` is a build change. When
something is there that Board does not read as a prototype record,
whether the change is a prototype is not known: `next`, `advance`, `plan`
and `pr` refuse with `changes::change::degraded`, `open` counts the
change as unreadable, the WIP limit counts it with the prototypes, and
the write guard refuses the builder's edits.

```json
{
  "$type": "changes/prototype/v1",
  "question": "Does a step list read better than a graph?"
}
```

`changes/change/v1` is the shape before flows: it has no `flow`, `suite`,
`surfaced` or `stop`, and its phase is one of the development flow's.
Every capability reads it as a change on the shipped development flow,
whose suite the repository's contributing docs name, and writes it back as
v1. `reflow` and `attach --suite` refuse it with
`changes::change::not_migrated`; `migrate --apply true` rewrites it as v2.

`changes/run/v1` at `/changes/<name>/runs/<stamp>` records one run of an
agent that `run` started: the `flow`, the `stop` it was given, the
`program` (the file name of argv[0] when it is a slug, never the
arguments), `started` and `ended` in UTC, the `exit` status, the
`cost_usd` and `duration_ms` the agent reported, the `budget_usd` its brief
gave, `sandbox`, whether it ran in a throwaway copy, with `change`, the
name of the change it started there, the `phase` the change ended in, the
`reason` it stopped, and `stopped`, the stop record that says why.

`changes/stop/v1` at `/changes/<name>/stops/<stamp>` records why a run
stopped: its `reason` (`reached`, `waiting`, `refused`, `budget` or
`error`), the `phase`, the ids of the blocking checks `missing` there, the
`plans/question/v1` it left for the owner as `question`, and a one-line
`summary`. The agent writes it with `stop`; when the agent exits without
one, `run` writes it.

`changes/understanding/v1` at `/changes/<name>/understanding/<subject>`
records how the owner showed he understands a change's `data` or `ux`.
Each of `questions` holds the `question`, the owner's `answer` in his own
words, and the `design`'s answer, and may say its `kind` (`predict`,
`choose`, `explain` or `edge`) and whether the answers `matches`.
`rechecks` holds the questions asked again before review. The
`understanding-check` skill in `skills/understanding-check/` runs the check.

The day of the last sync lives in Board's cache, keyed by the path of the
board's checkout, so the stale warning is per checkout: another clone or
worktree of the board, or a cleared cache, reports `changes::sync::stale`
until a sync runs there. When the cache does not take the day, `sync` and
`migrate` say why in `unrecorded`.

`migrate [--apply true]` rewrites each `changes/change/v1` of the board's
own as a `changes/change/v2`. It keeps the phase, `paused` included, sets
`surfaced` to `[]`, sets `suite` to the suite the mount's contributing docs
name, which a v1 change looked up on every `next`, and writes a copy of the
shipped development flow at `<path>/flow`, whose phases are v1's; a
prototype gets no flow and no suite. A v2 change
is left as it is, so a second run finds nothing. A change of the shape
before the split of `changes/change/v1` is rewritten from that shape first:
it moves `records` to the plan the change names, a prototype's fields to its
prototype record and a `/changes/sync` day to the cache, and records each
GitHub `pull_request` or `issue` URL through `github.ref`, which needs the
github plugin enabled. Without `--apply` it writes nothing and says what
it would. Both check each rewritten record against its schema. `left`
lists each change and understanding check it leaves as it is, with why:

- a field no shape has;
- a rewritten record that fails its schema;
- a prototype record already at `<change>/prototype` that Board does not
  read, or whose question or verdict differs from the old change's;
- with `page`, a GitHub page that `github.ref` refused, such as one GitHub
  answers with 404. `recorded` lists the github records already written for
  the change's earlier pages; they stay, and the next run uses them.

The rest migrate. A failure of gh itself, such as gh being signed out or
offline, says nothing about the page, so migrate stops with
`changes::migrate::gh_failed` and says to check `gh auth status`. A
refusal from Board itself, such as the github plugin not being enabled,
also stops it. Either stops it before it writes the changes, and the next
run keeps the github records already written.

A v1 change and a migrated one both meet the development flow's gates,
below, which block on more than the gates before flows did:

- leaving plan, `owner-approval` needs a `plans/decision/v1` that the owner
  signed, whose `refs` name the plan or the change, on every route. Before
  flows only an `ask-first` change needed a decision, and any decision
  passed, a rejection or an agent's stub included. `considered` is new, and
  passes for a migrated change, whose `surfaced` is empty;
- leaving review, `ui-paths`, `data-paths` and `obligations-settled` are
  new. Before flows the reviewer read the UI-path and data-path controls,
  and no gate read obligations;
- leaving land, `test-sections` is new: the plan needs text under What the
  tests prove and What the tests do not prove.

The build gate's checks were there before flows, under other ids:
`check-run` is now `suite-run` and `recheck` is now `rechecks`, which
reads understanding checks only when Data is not "None". A review verdict
of `changes` still fails `review`.

## Capabilities

- `start --mount (/repos/<name> | /) --title '<title>' [--task <path> | --task-repo <repo>] [--name <name>] [--handle @<you>] [--flow <name or path>] [--suite <path>]`
  creates the change in the first phase of its flow, its copy of the flow,
  and a plan with empty Problem, Outcome, Out of scope, UX and Data
  sections. "Flows" below says how the flow is chosen; `flow` in the output
  names it and its phases. `--task` names the task the change
  does, and must be a `tasks/task/v2` (`changes::ref::wrong_type`). Without
  it, `start` adds a task titled as the change through `board call
  tasks.add`, in `--task-repo` or, when that is left out, in the repo the
  caller's git remote routes to. A caller in a git repository with no
  `origin` is refused with `changes::task::unrouted`. It reads `CODEOWNERS`
  (`.github/`, root, `docs/`, the first found) to set the route and list
  `ask_first` patterns. `docs` lists the contributing docs found
  (`CONTRIBUTING.md` at the root, in `.github/` or in `docs/`, and
  `AGENTS.md`). `suite` is `--suite`, a `runs/suite/v1`, or else the first
  inline-code path in the docs that holds one, looked up in the
  repository's own board first; the change stores it. `surfaced` lists, at
  most ten of each with `total` and `left_out`: the open tasks of the notes
  repo that holds the change's task and under `/projects/<mount name>/`,
  the change's own task left out; the decisions under that project or
  whose `refs` name the mount or a path under it, newest first; and the
  evals whose `mounts` name the mount or that sit under the project. The
  change records their paths. `obligations` lists each obligation control
  on the mount, `<mount>/**`, by `control` and `summary`, from
  `controls.for-paths`, and is empty when the controls plugin is not
  enabled. Refuses an existing name (`changes::change::exists`), a
  repository path holding nothing (`changes::mount::not_found`) and a flow
  neither the board holds nor the plugin ships (`changes::flow::not_found`).
  A flow whose first phase is `build`, such as `small`, starts the change
  there, and `start` checks the WIP limit as `advance` into `build` does;
  `--over_limit '<why>'` goes past it. A flow that cannot run, as "Flows"
  below says, is `changes::flow::invalid`, with each of its `problems`.
  `--mount /` starts a change on the board itself, which edits only the
  board's own records, such as a flow-authoring change. It has no
  repository, so `start` reads no CODEOWNERS, contributing docs, suite or
  obligations on a mount, its route is `unowned`, and `note` says so. Its
  agent runs in the board's root, with no worktree. A flow with a `paths`,
  `run` or `controlled-paths` check reads a repository, so `--mount /`
  refuses it with `changes::mount::needs_repository`, naming those checks;
  `reflow` refuses moving such a change to one too.
- `run (--path /changes/<name> | --mount (/repos/<name> | /) --title '<title>' [--task <path>] [--name <name>] [--flow <name or path>] [--over_limit '<why>']) [--through <phase> | --until <phase>] [--inline true] [--budget <usd>] [--sandbox true [--for /changes/<name>]]`
  starts a change as `start` does, or resumes the one at `--path`, sets its
  `stop`, and builds its run brief. "Runs" below says what it does.
- `stop --path /changes/<name> --reason <reason> --summary '<one line>' [--question <path>]`
  writes a `changes/stop/v1` with the change's phase and the blocking
  checks missing there. `--question` must be a `plans/question/v1`
  (`changes::ref::wrong_type`). Each call writes a new record.
- `next --path /changes/<name>`: read-only. The change's `flow` (its name
  and phases), its phase, each gate check as `pass` or `missing` with a
  `detail`, its `actor`, whether it `blocks`, and when missing its
  `remedy`; the instruction, with its placeholders filled; and `sync`. When
  every missing check that blocks is the owner's or a reviewer's,
  `waiting_on` names them and `ask` holds the question to put to them,
  built from those checks' details and remedies.
- `sync` in the output of `open` and `next` is `{synced, stale, code,
  command}`: the day of the last sync, or null; `stale` when that is
  before today in UTC or null, with `code` `changes::sync::stale`; and
  `command`, `board call changes.sync`. Neither runs gh, so both stay
  read-only and work offline.
- `attach --path /changes/<name> [--ref <path>] [--ticket <path>] [--pull <path>] [--task <path>] [--route <route>] [--head <sha>] [--suite <path>]`
  adds a reference once or sets the fields. `--task` sets `task`, and must
  be a `tasks/task/v2`. `--suite` sets `suite`, and must be a
  `runs/suite/v1`. `--ref` adds to the `records` of the plan the change names, and
  refuses a change whose `plan` names no plan record
  (`changes::attach::no_plan`).
  `--ticket` to `tickets` and `--pull` to `pulls`. Refuses a reference that
  reaches nothing (`changes::ref::dangling`), and a ticket or pull that is
  not a record of a type its list accepts (`changes::ref::wrong_type`).
- `advance --path /changes/<name>` moves to the next phase of the change's
  flow, and from its last phase to `done`, when every check that blocks
  passes. Otherwise it refuses, `missing` lists the failed checks that
  block and `remedies` what to do for each, in order. The code is that of
  the first: `changes::ux::not_agreed` or `changes::ux::no_neighbours` for
  a `mockups` check, `changes::data::not_signed` for an `understood` check
  of the data without rechecks, `changes::paths::outside_plan` for a
  `paths` check, `changes::prototype::never_lands` for a prototype's
  `verdict` check, else `changes::gate::unmet`. Moving into `build` is also subject to the WIP
  limit, below; `--over_limit '<why>'` goes past it. A paused change is
  refused with
  `changes::phase::paused`. Leaving `done` or `dropped` is
  `changes::phase::final`, which points to `open` and `start`.
- `reflow --path /changes/<name> --flow <name>` replaces the change's copy
  of its flow with a copy of the flow named, the board's own or a shipped
  one, and returns `path`, `flow` and `phase`. It refuses a flow without
  the phase the change stands in, `build` for a paused change
  (`changes::reflow::no_phase`, whose `phases` lists the flow's), a flow
  that is not there (`changes::flow::not_found`), a v1 change
  (`changes::change::not_migrated`) and a done or dropped one
  (`changes::phase::final`). It also restores a copy that went missing.
- `pause --path /changes/<name>` moves a change in `build` to `paused`,
  which frees its place. A prototype can be paused too, and `verdict`
  still ends a paused prototype. It refuses any other phase
  (`changes::pause::not_building`).
- `resume --path /changes/<name>` moves a paused change back to `build`.
  It refuses a change that is not paused (`changes::resume::not_paused`),
  and is subject to the WIP limit, which `--over_limit '<why>'` goes past.
- `drop --path /changes/<name> --reason '<why>'` ends a change that is not
  `done` or `dropped` in `dropped` and records the reason in
  `dropped_reason`. It returns `path`, `from`, `to` and `task`, which stays
  open; close it with `tasks.drop` or `tasks.done`. A change already ended
  is refused with `changes::phase::final`.
- `sync [--dry_run true] [--limit <n>]` reads the pull request of each
  change of the board's own, under `/changes/`, in any phase but `plan`,
  `done` and `dropped`, with `gh api repos/<owner>/<repo>/pulls/<number>`,
  once each, for up to 20 seconds each. It returns `changes`, one entry for
  each such change by path, at most 100 by default, with `total`, `moved`
  and `truncated`. Each entry has `path`, `phase`, `to`, `status`, `code`,
  `pull_request`, `task`, `detail` and `command`:

  | `status` | When | What sync does | `code` |
  | --- | --- | --- | --- |
  | `merged` | GitHub says it merged | `to` is `done`, and `merge_commit_url` is set; `detail` says when the change left `build` or `review`, whose gates it did not pass | none |
  | `owed` | It merged, but a `tasks/task/v2` that its plans' `records` reach, through any link, is not done or dropped | Leaves it; `detail` names the tasks | `changes::sync::owed` |
  | `closed` | It closed without merging | `to` is `dropped`, and `dropped_reason` says so with the day it closed | none |
  | `open` | It is open | Leaves it | none |
  | `not_found` | GitHub answers 404, which it also answers when gh's account cannot see the repository | Leaves it; `detail` says to check `gh auth status`, and `command` drops it | `changes::sync::pull_not_found` |
  | `pull_unreadable` | The newest of `pulls` is not a `github/pull/v1` record Board reads | Leaves it; `command` attaches another | `changes::sync::pull_unreadable` |
  | `no_pull_request` | The change records no pull request | Leaves it; `command` resumes it when it has a `head`, else drops it | `changes::sync::no_pull_request` |

  Every move is written in one commit, and today's day goes to the cache. With `--dry_run true` nothing is written. When gh fails in any
  other way, cannot run, runs past 20 seconds, or the gh calls together run
  past 540 seconds, sync refuses with
  `changes::sync::gh_failed` and writes nothing. A merged change moves to `done`
  from any of those phases; `from` is in git history, so a change that
  skipped review shows it. Sync never closes a change's own `task`; each
  entry names it. The
  call receives `GH_TOKEN`, `GITHUB_TOKEN` and `GH_CONFIG_DIR` for gh, and
  may run for 600 seconds.
- `open [--kind build|prototype] [--limit <n>]`: read-only. Changes not
  `done` or `dropped`, by path, at most 100 by default, with `total`,
  `truncated`, `unreadable` and `sync`. Each names its `task`, `kind` and
  `flow`, the flow's name and phases, left out for a prototype and for a
  change whose flow copy Board does not read; and a
  prototype also its `question`, `state` (from its phase), `verdict`,
  `verdict_artifact` and `branch`. `--kind` keeps one kind; `--kind prototype` also lists the
  settled and abandoned prototypes, since their verdicts are what is looked
  for.
- `prototype --mount /repos/<name> --question '<question>' --parent <task> [--title '<title>'] [--name <name>]`
  adds the prototype's task under `--parent` through `board call tasks.add
  --parent`, in the parent's repo. It writes a change in `build`, its prototype
  record with its question, and a plan whose text is the question, and returns
  `path`, `task`, `branch` and the `brief` its one builder starts from. The
  owner then iterates with that builder for as many rounds as he likes.
  The task is added before the change is written, so a refused write leaves
  it behind for `tasks.drop`. Refuses an existing name
  (`changes::change::exists`), and a `--parent` that is not under a
  `tasks/` directory (`changes::task::not_a_task_path`), before it adds
  anything. It is subject to the WIP limit for prototypes, which
  `--over_limit '<why>'` goes past.
- `verdict --path /changes/<name> --state settled|abandoned --verdict '<sentence>' [--artifact <url>]`
  records what a prototype found and ends it: `done` when settled,
  `dropped` when abandoned. It then closes the prototype's task through
  `tasks.done` or `tasks.drop`, unless the task is already done or
  dropped. Refuses a change that is not a prototype
  (`changes::prototype::not_prototype`) and one already ended
  (`changes::phase::final`).

- `pr --path /changes/<name>` drafts the pull request. It writes the body
  to `/changes/<name>/pr.md`, replacing an earlier one, and returns `path`,
  `command`, `pull_request` and `unreadable`. It never opens the pull request, never runs
  `gh` or git, and makes no model calls. See "The pull request body".

- `plan` takes `{path, text, uses, paths}` as JSON on stdin, since `uses`
  and `paths` are lists: `board call changes.plan` with the JSON piped in.
  A missing `uses` is an empty list. It keeps each path once. It refuses a build change's
  plan with no paths (`changes::plan::no_paths`) and a path that does not
  start with the change's mount and `/` (`changes::plan::outside_mount`,
  whose `paths` lists them). It writes the plan record and its text and points the change's `plan` at the
  text, in one write, in any phase but `done` and `dropped`
  (`changes::phase::final`). It keeps each reference in `uses` once. It
  refuses a reference that reaches nothing, or a record Board marks degraded
  for anything but a broken reference of its own (`changes::ref::dangling`,
  whose `refs` lists them); a directory (`changes::plan::directory`); a text
  over 3,000 words (`changes::plan::too_long`); and Markdown documents in
  `uses` over 3,000 words in all (`changes::plan::uses_too_long`). Words are
  separated by whitespace. The last limit comes from
  `/evals/board-standards-selection` in the owner's board: agents that chose
  standards from each directory's `index.md` stayed within it.
- `brief --path /changes/<name>`: read-only. `brief_line`,
  `board-brief: /changes/<name>`, which comes first; a builder's first
  message starts with it (see "The write guard"). The text path of the plan the change names,
  even once a reference in it breaks,
  `rules`, each control that applies to the plan's `paths`, once, from
  `controls.for-paths`: its `control`, its `summary` and at most three of
  the planned paths it `matched`, with `matched_left_out` counting the
  rest. Both are left out when every planned path reached the rule.
  Rules that matched only some planned paths come first, then the others,
  each group by control path. Each rule's path and summary are fitted
  first, skipping any that do not fit, and matched paths are added after
  while room remains. `rules_left_out` names up to 20 rules the word limit
  dropped, as many as fit, and `rules_unnamed` counts the rest.
  `rules_command` lists every rule: run its `argv` without a shell and give
  it its `stdin`. It is null when the plan has no paths. The brief also
  holds one line per record the plan uses (a control's `summary`, else its
  `requirement`; a decision's `decision`; an eval's question and verdict; a
  document's first heading), `left_out`, the phase and instruction `next`
  gives, and the `controls.check` command for the change's mount. It holds at
  most 600 words, counting each rule with its path and matched paths,
  each named dropped rule, and each line with its reference. Rules are
  counted before lines, but lines keep up to a quarter of the limit. A
  line that does not fit is skipped, and `left_out` counts the lines it
  dropped. The full requirement of a rule is `controls.for-path` away. It
  fails with `changes::brief::controls_missing` when the Board that runs
  it does not offer `controls.for-paths`, such as when the controls
  plugin is not enabled.

The WIP limit counts, for each repository, the active build changes in
`build`, at most three, and apart from them the active prototypes in
`build`, at most three. A change's repository is the clone Board says its
`mount` reads, keyed by that clone's `origin` as git gives it without the
user's config, in its stored form: `owner/name` for GitHub, in any case,
scheme (`git+ssh://` too), scp form, `www.` or ssh port, and the URL in
lower case otherwise. An origin that is another local clone, by absolute
path or `file://` URL, is followed to that clone's origin, through at most
three clones. Two mounts of one repository count together, even from two
clones. A clone whose origin git cannot read is keyed by its path, a chain
of local clones that runs out by its last clone, and a mount Board cannot
resolve by the mount. Origins are read from each clone's git config, and
never from a repository around a directory that is not one. Each `git log`
has 10 s, and all of them 20 s; past that the move is refused with
`changes::wip::timeout`. A prototype in `build` is always `exploring`,
because `verdict` moves it out of `build` in the same write that sets its
state. A paused change holds no place. Only the board's own changes, under
`/changes/`, count. A change is active when the newest of these days is
one of the last N days, in UTC, today included; with N at 3 on
2026-11-04, that is 2026-11-02 to 2026-11-04:

- `touched`;
- the day its `head` was committed, by `git log -1` on that head in the
  repository its mount names, so git resolves an abbreviated head;
- the day a `runs/run/v1` that its plans' `records` reach, through any link,
  `finished`.

A review report has no day of its own, so a review keeps a change active
through the `attach` that records it.

N is 3, or `BOARD_CHANGES_ACTIVE_DAYS` when that is set. A value that is
not a whole number of at least 1 is refused with
`changes::wip::bad_active_days`. A change with none of those days is idle.

When three other active changes of the kind hold places for the repository,
`advance` into `build`, `resume` and `prototype` refuse with
`changes::wip::full`, unless `--over_limit '<why>'` is given. The refusal's
fields, and with `--over_limit` the output's `warning`, hold `mount`,
`repository`, the key counted, `unread`, each mount whose origin git
could not read, or whose chain of local clones ran out, with the reason,
`kind`, `limit`, `active_days`, `active`, the changes holding places,
`idle`, each idle change of the kind in `build` for the repository with its
`action` and `command`, and `override`, the command with `--over_limit`.
The action is `sync` for a change with a pull request, `resume`
(`changes.next`) for one with a head, and `drop` otherwise. With
`--over_limit` the call goes ahead, the change records the reason in
`over_limit`, and `warning.code` is `changes::wip::over_limit`. A call
without it is refused, so an agent reads the idle list and records why it
goes past the limit, which a warning in a successful result would not make
it do. The limit is checked when a change moves into `build` and never
appears in a gate.

A path that holds no change is `changes::change::not_found`; a degraded
change is `changes::change::degraded`.

### Runs

`run` sets the change's `stop` to `--through` or `--until` a phase, else
keeps the stop the change has, else takes its flow's. The phase must be one
of the flow's, or `done` (`changes::run::no_phase`). Giving both is
`changes::run::two_stops`, and naming no change, or both a path and a
mount, is `changes::run::which_change`. A v1 change, a prototype
(`changes::run::no_flow`), a paused change and an ended one are refused.

A `through X` stop is reached when the change stands in X and X's gate has
no blocking check missing, or stands later. An `until X` stop is reached
as soon as the change stands in X or later. `next` returns `stop` with
`reached`, and when it is reached its instruction says to stop and record
`stop --reason reached`. `advance` refuses to move a change that stands in
the stop's phase, or later, with `changes::run::stop_reached`; its message
gives the `run --until <next phase> --inline true` command that moves the
stop.

The run brief holds `brief_line`, the change, its board and flow, the stop
and its rule in plain words, `steps` (each phase from the current one up
to the stop, through its phase for `through`, with who does it and its
instruction), at most 20 `surfaced` records with a count of the rest, the
rules fields `brief` returns, `stopping`, which says how an unattended
agent stops, and `budget_usd`. `--inline true` returns the brief and
starts nothing; inside Claude Code the `flow` skill in
`skills/flow/` uses it.

Without `--inline`, `run` reads the agent's argv from `BOARD_AGENT`, a
JSON array of strings, or else from `agent` in the known boards file,
`~/.config/board/config.json`, such as `{"agent": ["claude", "-p",
"--output-format", "json"]}`. With neither it refuses with
`changes::agent::not_set`, and an argv that is not a JSON array of strings
is `changes::agent::invalid`. When the stop is already reached it starts
nothing. Otherwise it makes the worktree `<clone>.worktrees/<name>` on
branch `<name>` from the branch the mount follows, or reuses it, and
starts the argv in the mount's directory there, with the brief line, a
blank line and the brief as JSON on stdin and `BOARD_CHANGE` set to the
change's path. A program that cannot start is `changes::agent::not_started`
and a worktree git cannot make is `changes::run::worktree`; the change is
already started and its stop set by then.

`run` waits for the agent, for at most 59 minutes, and has no
`--detach`. When the agent's last line on stdout is a JSON object, as
Claude Code's `--output-format json` prints, it reads `total_cost_usd` and
`duration_ms` from it. When the agent wrote no stop record during the run,
`run` writes one. Its reason is `error` when the agent ran too long or
printed too much. Otherwise, after an exit 0, it is `reached` when the
stop is reached and `waiting` when the change waits on the owner or a
reviewer. Failing those, it is `budget` when the reported cost is over
`--budget`, and `error` otherwise. It then writes the run record.
`--budget` reaches the agent only through the brief; `run` passes no flag
of any agent program.

`--sandbox true` dry-runs a flow, as the flow-authoring flow's dry-run
phase does. It needs a new change, so `--path` is
`changes::sandbox::resume` and `--inline true` is
`changes::sandbox::inline`. `run` makes a fresh directory
`board-sandbox.<random>` under `TMPDIR` and copies into it, as new git
repositories with no remote, the board's files that git tracks or would
track, uncommitted edits included, and for a change on a repository every
ref of the clone its mount reads, pointing the copy's mount record at it.
Ignored files and nested repositories are left out, and a mount that is
not a record of the board's own tree, or a copy git cannot make, is
`changes::sandbox::setup`. `run` starts the change in the copy, adding its
task to the copy's own `/tasks/` when `--task` is left out, and runs the
agent there as above, with `HOME`, `XDG_CONFIG_HOME`, `XDG_CACHE_HOME` and
`XDG_DATA_HOME` inside the directory and a known boards file that lists
only the copy. So the agent's `board` calls, its commits and any user-level
settings it would read stay off the real board, repository and home. This
is not an operating-system sandbox: the agent still runs as the person and
can write any path it names, the copy's other mounts read the real clones
at their pins, and the agent must authenticate through the variables `run`
passes on, since its home holds no credentials.

The run record and its stop record are then copied, at the same stamps,
under the change the run is for: `--for`, else the change whose directory
holds the flow `--flow` names by path, else `changes::sandbox::no_change`.
The copy of the run says `sandbox: true` and names the throwaway change in
`change`; the copied stop drops its `question`, which only the copy holds.
`run` attaches the run to that change, and returns the copy's paths, the
throwaway change's `path` and the `sandbox` directory, which it leaves for
inspection.

### Flows

A `changes/flow/v1` record describes the process a change follows: its
`name`, a one-sentence `summary`, the default `stop` of an agent that runs
it (`{"through": "<phase>"}` or `{"until": "<phase>"}`), and its `phases` in
order. Each phase has a `name`, the `instruction` that `next` returns, the
`agent` that does its work (`lead` or `builder`), the `skills` the
instruction relies on, and its `checks`. A check has an `id`, a `kind`, and
optionally an `actor` (`agent`, `owner` or `reviewer`), `blocks`, a `when`
(`route`, and `section` or `section_not`, each mapping a plan heading to a
pattern matched without case), a `remedy`, `waivable`, and the parameters
of its kind. The committed schema is
`crates/board-plugin-changes/types/changes/flow/v1.schema.json`.

The plugin ships three flows, compiled in from
`crates/board-plugin-changes/flows/`. `development` has the phases plan,
build, review and land, and stops through plan. `small` has build, review
and land, and stops through review. `flow-authoring` writes a flow, on
`--mount /` or the mount it is for: interview the owner, draft the flow at
`<change>/draft`, dry-run it with `run --sandbox`, and adopt it at
`/flows/<name>` once the owner signs a decision naming the draft and the
eval; it stops until adopt. A board's own flow at `/flows/<name>` wins
over a shipped flow of the same name. `--flow` also takes the path of a
flow record, such as a draft.

A flow can run when every check's kind is one Board knows and has the
parameter its kind needs (`sections`, `field`, `type` for `records`,
`subject`, `controls`, `path` for `exists`), every check id is unique in
the flow, the stop names one of its phases or `done`, and its
instructions, remedies and templated paths name only the placeholders
below. `start` and `reflow` refuse any other with
`changes::flow::invalid`.

`start` chooses a change's flow: the one `--flow` names; else the flow the
link `/flows/repos/<mount name>` points to; else the one `/flows/default`
points to; else `development`. A link is a `board/mount/v1` with a `path`,
and Board follows it, so it must point at a flow the board holds. A flow
that is not there is `changes::flow::not_found`, and a file there that is
not a flow Board reads is `changes::flow::unreadable`. `start` writes a
copy of the flow at `<change>/flow`, and every gate reads that copy,
so editing a board's flow never moves a change in flight; `reflow` takes
up the edit. A change whose copy is missing or does not read is refused
with `changes::flow::unreadable`, and `reflow` restores it.

`advance` moves a change through its flow's phases in order and then to
`done`. Each gate is the list of the phase's checks whose `when` holds,
evaluated by kind:

| Kind | Passes when |
| --- | --- |
| `sections` | the plan has text under each heading in `sections` |
| `field` | the change sets `field` (`task`, `route`, `head`, `plan`, `tickets` or `pulls`), with at least `min` references; for `tickets` with `types`, at least `min` tickets Board reads as one of them; for `pulls` with `types`, the newest reads as a `github/pull/v1` |
| `records` | at least `min` attached records of `type` meet `where` (`at_head`, `values`, `no_findings`, `owner_signed`, and `refs`, each of which a record's refs must name: `plan` is the plan, its text or the change), from different actors when `distinct` names a part of `by` |
| `run` | an attached run of the change's suite with `scope` is clean, passing and on `head` |
| `paths` | the diff edits only the plan's paths |
| `tasks` | every attached task but the change's own is done or dropped |
| `understood` | a decision the owner signed references an attached understanding check of this change's `subject`; with `rechecks`, each such check also holds that many matching rechecks |
| `mockups` | the `agreed` part: each mockup under UX is named by a signed decision; the `neighbours` part: UX lists its neighbours; both when `part` is left out |
| `considered` | each path in the change's `surfaced` is in the plan's `uses` or named under a `### Considered` heading |
| `obligations` | no obligation control applies to the plan's paths, except those named under the `escape_section` heading |
| `controlled-paths` | the diff edits no file that the controls in `controls` list in their `paths` or `applies_to` |
| `flow-valid` | at least one `changes/flow/v1` is attached, and each can run and gives a `remedy` to every check whose actor is the owner or a reviewer; the detail lists each problem with its phase and check id |
| `exists` | something is at `path`, its placeholders filled; a placeholder with no value yet leaves it missing |

A `controlled-paths` check whose controls the board does not hold passes,
and its detail says that no such control exists. A kind Board does not
know reads but is always missing.

A check's `actor` (`agent`, the default, `owner` or `reviewer`) says who
can make it pass. A check with `blocks: false` is reported but never
refuses `advance`. A missing check carries a `remedy`: the flow's, or else
its kind's own. A `waivable` check passes when an attached
`plans/decision/v1` that the owner signed names `<change>#<id>` in its
`refs`, and its detail names the decision; its remedy says so. A decision
with `written_by` set is an agent's stub and signs nothing, here and in
every check.

Instructions, remedies and a `controlled-paths` check's controls may name
`{change}`, `{name}`, `{mount}`, `{repo}`, `{plan}`, `{head}`, `{suite}`,
`{draft}` (the name of the first attached `changes/flow/v1`) and `{board}`
(the board's root). A placeholder with no value is left out cleanly: the
text loses it, an inline code span it leaves empty goes too, runs of
spaces become one, and no space is left before punctuation. Other text in
braces stays.

Each gate reads only what its checks need: the change, the records it
references, its plan file, its suite, the diff, the controls plugin's
obligations on the plan's paths, the controls a check names, its
tickets' types, and whether something is at each path an `exists` check
names. A referenced record Board marks degraded satisfies nothing.

The development flow's gates:

| Leaving | Checks |
| --- | --- |
| plan | `plan-sections`; `considered`; `task`; `route`; `owner-approval`, a `plans/decision/v1` the owner signed whose `refs` name the plan or the change, on every route; `mockups`, unless UX starts with "No UI change" and a reason; `data-signed`, an `understood` check of the data, unless Data starts with "None"; `obligations`, which does not block |
| build | `head`; `suite-run`, waivable; `tasks`; `rechecks`, two matching rechecks, unless Data starts with "None" |
| review | `review`, a report on `head` with no critical or high finding; `paths`; `ui-paths` and `data-paths`, `controlled-paths` checks of `/controls/{repo}/ui-paths` when UX says "No UI change" and `/controls/{repo}/data-paths` when Data says "None"; `obligations-settled`, with the escape heading `Obligations left owed` |
| land | `test-sections`, What the tests prove and What the tests do not prove; `pull-request` |
| build, for a prototype | Never passes: `verdict` ends a prototype instead. |

"No UI change" counts only when a reason follows it, and "None" only as a
word of its own followed by a full stop, a colon or nothing. Each item of
the `### Mockups` list names one mockup, by its first URL or, without one,
by its first board path in inline code; other links are not mockups. The
`### Neighbours` list names the open changes and artifacts in the same
area and the components the design reuses; the gate checks that it
exists, and the reviewer judges what it says. An attached understanding
check Board does not read fails an `understood` check. A repeating task
never reaches `done`, so drop it to release the `tasks` check. A run counts
toward `run` when it sits under `<suite>/runs/`, has no `fail` or `error`,
and names `head` in `versions`.

The review gate's `paths` check reads the whole change's diff, whatever
range a review report covers: `git diff --name-only --no-renames
--merge-base <follow> <head>` in the repository the mount names, which
diffs from the merge base of `head` and `<follow>`, the ref the mount
follows as `board refs` reports it. It runs without the user's or
system's git config. A rebased change leaves out what the followed branch
gained, and a review of only the fix commits still checks every commit of
the change. The check fails, saying why, when the change has no head,
when nothing is at its mount, when its mount is a plain directory ("<mount>
is not a mount"), and when git cannot give the diff. A rename counts as
deleting one path and adding another. Each file it lists, by its path under the mount, must match one of
the `paths` of the plan the change names with SQLite's `GLOB`. Otherwise `advance` refuses
with `changes::paths::outside_plan` and names the files, because their
rules were not in the brief. Update the plan's paths with `plan`, or revert
the edit. A plan with no paths, no report on `head`, or a diff git cannot
list also fails the check.

Commits match when one is a prefix of the other and both have at least 7
digits.

## The pull request body

`pr` returns:

- `command`: `gh pr create --title '<title>' --body-file '<file on disk>'`,
  quoted for a POSIX shell. Run it from a checkout of the repository on the
  change's `head`. The title is the change's `title` with its whitespace
  collapsed to single spaces.
- `pull_request`: when the newest of `pulls` is a `github/pull/v1`, its
  `url`. `command` is then
  `null` and `pr.md` is not written.
- `unreadable`: the references that reach nothing or that Board marks
  degraded.

`pr` reads every reference, in any phase. A reference degraded only by a
broken reference of its own is still read.

| Section | From |
| --- | --- |
| (first) | The plan's Outcome. |
| Why | The plan's Problem. |
| Out of scope | The plan's Out of scope, when it has text. |
| What the tests prove | The plan's section of that name, then each attached `runs/run/v1`: suite, scope, results by outcome, summary. |
| What the tests do not prove | The plan's section of that name, then each run's limits: targeted, not clean, not on `head`, and each case that failed, errored or was skipped. Says so when no run is attached. |
| Review | Each attached `review/report/v1`: verdict, author, range, findings by severity. |
| Evidence | The change, the plan, and every reference with its type (`file` for a file that is not a record, `directory` for a directory), or the code that hid it. |
| Work in progress | The change's `over_limit`, when set. |
| `Closes <issue>` | The `url` of each `github/issue/v1` in `tickets`. |

Plan headings match without case. A plan section holds its subsections, and
a `#` line inside a code fence is not a heading. An empty plan section
becomes a line that names it. The plan's sections are copied as written.
Text from a referenced record is put on one line, so it cannot add a heading
or a list item. Paths and free text (a run's suite and summary, a case and
its note, a report's author) go in inline code; empty text shows as
`(empty)`. Each run names at most 20 failed, errored or skipped cases and
counts the rest. `Closes` is left out for a
`url` that holds whitespace. Evidence is listed by logical path, since a board path
has no URL.

## The write guard

`claude-code/` holds a Claude Code mod, `board-changes`, that runs before
every Edit, Write and NotebookEdit. Install it with
`claude plugin install board-changes@board`. It needs `git`, and `board` on
`PATH` or in `~/.local/bin`, `/opt/homebrew/bin` or `/usr/local/bin`.

1. It places the file: it expands a leading `~/` from `HOME`, follows every
   symbolic link, and takes a file that does not exist yet from its nearest
   directory that does. A path it cannot place, such as `~user/x` or a link
   that leads nowhere, is refused with `changes::guard::unknown`.
2. It finds the git repository that holds that directory, through its
   common directory, so a worktree counts as its repository. It lists the
   mounts of each board in `boards` of `$XDG_CONFIG_HOME/board/config.json`
   (by default `~/.config/board/config.json`). A mount holds the file when
   the file's repository is a worktree of the mount's `repo`, compared as
   spelled. Otherwise, when the repository's `origin` is a local path or a
   `file://` URL, a mount holds the file when that origin is a worktree of
   the mount's `repo`. Otherwise a mount holds the file when both
   repositories' `origin` remotes name the same repository; ssh and https
   forms, letter case and a trailing `.git` do not matter. So a separate
   clone is guarded too. A file that no mount holds is admitted, so plans,
   other board records, memory and scratch directories stay writable. A
   repository with no `origin` that is no worktree of a mount is admitted.
   A repository whose directory is listed in `boards`, or a worktree of
   one, is admitted even when another known board mounts it, because a
   board holds records and no code. Its board records, in the listed
   directory and in each linked worktree, are still refused with
   `changes::guard::board_record`. In a linked worktree the guard reads
   the mount records from the worktree's own branch, and the refusal names
   the worktree. A file inside the board repository's git directory is
   refused with `changes::guard::unknown`, and the refusal says the file is
   in the git directory. The guard compares the repository's directory
   with each listed directory, with symbolic links followed, and they must
   be equal: a board inside a larger repository does not exempt that
   repository. The listed directory
   must be the repository's main checkout. A listed directory that is
   itself a linked worktree exempts nothing, so its repository stays
   guarded. The guard reads this from the config. It does not test for a
   board root marker such as `.board/board.json`, because a code
   repository can commit a board of its own, as Board's own source does.
   Listing a code repository in `boards` exempts it and all its worktrees
   from the guard, so list only boards there.
3. Otherwise it reads the acting agent's first user message: a subagent's
   own, or the main conversation's. The first line that holds text must be
   `board-brief: /changes/<name>`. The guard checks that line, and does not
   check that `changes.brief` produced it, so a parent agent can write the
   line itself.
4. It reads that change from each board that mounts the repository. It
   admits the call when the change's `phase` is `build` and its `mount` is the
   mount that holds the file; a paused change is refused and told to
   resume. It then reads `<change>/prototype` with `board get`, and when a
   prototype record is there it asks `board check --healthy` whether Board
   reads it, because `board get` prints a degraded record as it prints a
   valid one. When a readable prototype record is there, the file's
   checkout must also be on branch `proto/<name>`. When the record there is
   degraded or of another type, the edit is refused with
   `changes::guard::unknown`.

Before any of that, the guard refuses three kinds of file to every agent,
brief or not, because they change only through board commands: the board
config, each known board's change records (`changes/<name>.json`, its
`prototype.json` and its `plans/<stamp>.json`) and its mount records. A
plan's text, `changes/<name>/plans/<stamp>.md`, stays writable.

Each refusal names one code and what fixes it:

- `changes::guard::board_record`: the file is the board config, a change
  record or a mount record. The refusal names the board command to use.

- `changes::guard::no_brief`: the first message has no brief line, or names
  a change no such board holds. Start a builder whose first message is the
  output of `changes.brief`.
- `changes::guard::not_building`: run `changes.advance` until the change is
  in `build`.
- `changes::guard::wrong_mount`: the brief's change is for another
  repository.
- `changes::guard::wrong_branch`: the brief's change is a prototype, and
  the file's checkout is not on `proto/<name>`. Switch to that branch.
- `changes::guard::unknown`: the path could not be placed, git, the config
  or `board` failed, or the conversation could not be read. The refusal says
  what failed.

The guard is a guardrail for agents that follow the process, not a defence
against an agent set on getting round it. The known routes round it are
Bash writes, a hand-typed brief line and a clone of a local clone.

There is no bypass flag. The guard cannot see:

- files written through Bash, such as `sed -i` or a heredoc. The review gate
  checks that a change's commits came from its builder sessions;
- a prototype's builder that switches its checkout to `proto/<name>` for
  the edit and back before it commits. The guard reads the branch at the
  moment of the edit;
- a clone of a mounted repository with no `origin` remote, or one whose
  `origin` points somewhere else, such as a fork or an ssh host alias like
  `git@github-work:owner/name.git`;
- a brief line that a parent agent wrote by hand. The review gate checks
  that a change's commits came from its builder sessions;
- a first message that has left the newest 4,096 entries of a conversation,
  or that a compaction replaced, before the agent's first guarded edit. The
  guard keeps a brief it has found for each session and agent until the mod
  reloads, and `/clear` or `/resume` starts over. With no brief kept and a
  full window of 4,096 entries, the guard refuses with
  `changes::guard::unknown`, since the oldest row left may be one another
  agent wrote.

The guard refuses an edit of every mounted repository that is not itself a
known board, including the notes repositories a personal board mounts, such
as `/repos/notes-personal`. An agent without a brief writes notes and tasks
through the `tasks` and `notes` capabilities instead.

`claude plugin test plugins/changes/claude-code` runs the mod's tests.

## Installing

Run `board plugin enable changes` in a board. Enabling writes the schemas to
`.board/types/changes/` and names the plugin in `.board/board.json`. The type
stays at v1 and changes in place while the plugin has one user; after such a
change, enable it again.

`start`, `prototype` and `verdict` call the tasks plugin through a nested
`board call`, which syncs and pushes a notes repo, so each may run for 120
seconds instead of Board's default limit.

## Tests

`crates/board-cli/src/tests/built_in/changes.rs` and its modules run the
`board` binary in process against real git repositories mounted beside the
board and a stand-in gh. They are the former pytest suite. The crate's own
tests cover the gates, the check kinds and `when` conditions the gates
interpret, the four flows of the configurable-flows design read and checked
against the flow schema, the WIP limit's window and origin forms, the pull
request body, the brief's fitting of rules, and the manifest and its
committed schemas.
