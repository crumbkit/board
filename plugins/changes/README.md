# changes

A Board plugin, built into Board as `crates/board-plugin-changes`, that walks an agent through one change to a mounted
repository, from its plan to its pull request, and gathers every record the
change produces. SKILL.md holds the procedure for each phase.

## Records

`changes/change/v1` at `/changes/<name>.json` holds the change and its
phase. The records it owns sit in `/changes/<name>/`: its plans, a
prototype's record and its understanding checks. Git history holds earlier
phases.

```json
{
  "$type": "changes/change/v1",
  "title": "Let a suite declare a timeout",
  "mount": "/repos/board",
  "phase": "build",
  "route": "owner",
  "plan": "/changes/suite-timeout/plans/20261101T120000000000Z",
  "task": "/repos/notes-personal/tasks/suite-timeout.md",
  "head": "a3f9c0d",
  "tickets": ["/jira/acme/ACME-123"],
  "pulls": ["/gh/octo/widgets/pulls/7"]
}
```

- `title`, `mount` and `phase` are required.
- `phase`: `plan`, `build`, `review`, `land`, `done`, or `dropped` to end a
  change early. `paused` holds a change the keeper took out of `build`.
- `mount` references the repository's `board/mount/v1` record, and `plan`
  the change's newest `changes/plan/v1`. `board check` reports a reference
  that reaches nothing (`board::ref::dangling`) or a record of another type
  (`board::ref::wrong_type`).
- `tickets` references the `jira/issue/v1` or `github/issue/v1` records the
  change works on, and `pulls` its `github/pull/v1` records. The jira and
  github plugins write those records; `board call github.ref --ref
  owner/repo#number` writes a GitHub one. `board refs <ticket>` lists the
  changes that reference a ticket.
- `task` references the `tasks/task/v2` the change does. `start` always
  sets it; a change without one cannot leave `plan`.
- `head` is the commit under test and review.
- `touched` is the UTC day, `YYYY-MM-DD`, on which `start`, `attach`,
  `advance`, `plan`, `resume` or `prototype` last wrote the change. The WIP
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
  --ref` attached while this plan was the newest. The records a change owns
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

`changes/understanding/v1` at `/changes/<name>/understanding/<subject>`
records how the owner showed he understands a change's `data` or `ux`.
Each of `questions` holds the `question`, the owner's `answer` in his own
words, and the `design`'s answer, and may say its `kind` (`predict`,
`choose`, `explain` or `edge`) and whether the answers `matches`.
`rechecks` holds the questions asked again before review. The
`understanding-check` skill in `claude-code/skills/` runs the check.

The day of the last sync lives in Board's cache, keyed by the path of the
board's checkout, so the stale warning is per checkout: another clone or
worktree of the board, or a cleared cache, reports `changes::sync::stale`
until a sync runs there. When the cache does not take the day, `sync` and
`migrate` say why in `unrecorded`.

`migrate [--apply true]` rewrites change records of the shape before this
split: it moves `records` to the newest plan, a prototype's fields to its
prototype record and a `/changes/sync` day to the cache, and records each
GitHub `pull_request` or `issue` URL through `github.ref`, which needs the
github plugin enabled. Without `--apply` it writes nothing and says what
it would. Both check each rewritten record against its schema. `left`
lists each change and understanding check it leaves as it is, with why: a
field no shape has, a rewritten record that fails its schema, or, with
`page`, a GitHub page that `github.ref` refused. The rest migrate. A
refusal from Board itself, such as the github plugin not being enabled,
stops the migration before it writes the changes.

## Capabilities

- `start --mount /repos/<name> --title '<title>' [--task <path> | --task-repo <repo>] [--name <name>] [--handle @<you>]`
  creates the change in `plan` and a plan with empty Problem, Outcome,
  Out of scope, UX and Data sections. `--task` names the task the change
  does, and must be a `tasks/task/v2` (`changes::ref::wrong_type`). Without
  it, `start` adds a task titled as the change through `board call
  tasks.add`, in `--task-repo` or, when that is left out, in the repo the
  caller's git remote routes to. A caller in a git repository with no
  `origin` is refused with `changes::task::unrouted`. It reads `CODEOWNERS`
  (`.github/`, root, `docs/`, the first found) to set the route and list
  `ask_first` patterns. `docs` lists the contributing docs found
  (`CONTRIBUTING.md` at the root, in `.github/` or in `docs/`, and
  `AGENTS.md`). `suite` is the first inline-code path in them that holds a
  `runs/suite/v1`, looked up in the repository's own board first. Refuses an
  existing name (`changes::change::exists`) and a repository path holding
  nothing (`changes::mount::not_found`).
- `next --path /changes/<name>`: read-only. The phase, each gate check as
  `pass` or `missing` with a detail, the instruction, and `sync`.
- `sync` in the output of `open` and `next` is `{synced, stale, code,
  command}`: the day of the last sync, or null; `stale` when that is
  before today in UTC or null, with `code` `changes::sync::stale`; and
  `command`, `board call changes.sync`. Neither runs gh, so both stay
  read-only and work offline.
- `attach --path /changes/<name> [--ref <path>] [--ticket <path>] [--pull <path>] [--task <path>] [--route <route>] [--head <sha>]`
  adds a reference once or sets the fields. `--task` sets `task`, and must
  be a `tasks/task/v2`. `--ref` adds to the newest plan's `records`, and
  refuses a change whose `plan` names no plan record
  (`changes::attach::no_plan`).
  `--ticket` to `tickets` and `--pull` to `pulls`. Refuses a reference that
  reaches nothing (`changes::ref::dangling`), and a ticket or pull that is
  not a record of a type its list accepts (`changes::ref::wrong_type`).
- `advance --path /changes/<name>` moves to the next phase when the gate
  passes. Otherwise it refuses, and `missing` lists the failed checks. The
  code is that of the first failed check: `changes::ux::not_agreed`,
  `changes::ux::no_neighbours`, `changes::data::not_signed`,
  `changes::paths::outside_plan` for the review gate's `paths` check,
  `changes::prototype::never_lands` for a prototype's `verdict` check, else
  `changes::gate::unmet`. Moving into `build` is also subject to the WIP
  limit, below; `--over_limit '<why>'` goes past it. A paused change is
  refused with
  `changes::phase::paused`. Leaving `done` or `dropped` is
  `changes::phase::final`, which points to `open` and `start`.
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
  change of the board's own, under `/changes/`, in `build`, `review`,
  `land` or `paused`, with `gh api repos/<owner>/<repo>/pulls/<number>`,
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
  `truncated`, `unreadable` and `sync`. Each names its `task` and `kind`, and a
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

- `plan` takes `{path, text, uses, paths}` on stdin, since `uses` and
  `paths` are arrays. It keeps each path once. It refuses a build change's
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
  message starts with it (see "The write guard"). The newest plan's text path,
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

Each gate reads only the change, the records it references, its plan file,
in `build` the repository's contributing docs, and in `review` the newest
plan's paths and the change's diff. A referenced record
Board marks degraded satisfies nothing.

| Leaving | Gate |
| --- | --- |
| plan | Each plan section has text, `task` is set, and `route` is set. On `ask-first`, also a `plans/decision/v1` whose `refs` name the plan or the change. Any such decision passes, including a rejection. Then `ux`, `neighbours` and `data`, below. |
| build | Every attached `tasks/task/v2` is `done` or `dropped`. A repeating task never reaches `done`, so drop it to release the gate. An attached `runs/run/v1` under `<suite>/runs/` of the check suite is full, clean, has no `fail` or `error`, and names `head` in `versions`. Each attached `changes/understanding/v1` of this change holds at least two `rechecks`, and every one of them matched; an attached understanding check that Board does not read fails the check. |
| review | An attached `review/report/v1` whose `range` ends at `head`, whose `verdict` is not `changes`, and which has no `critical` or `high` finding. Then `paths`, below. |
| land | The newest of `pulls` reads as a `github/pull/v1` record. Advancing enters `done`. |
| build, for a prototype | Never passes: `verdict` ends a prototype instead. |

The plan gate's last three checks:

- `ux`: the UX section starts with "No UI change" followed by a reason,
  or holds a `### Mockups` heading with a list under it. Each item of
  that list names one mockup, by its first URL or, without one, by its
  first board path in inline code. Other links in the section are not
  mockups. Each mockup is named in the `urls` or `refs` of an attached
  `plans/decision/v1` that the owner signed: a decision with
  `written_by` set is an agent's stub and does not count.
- `neighbours`: a UX section that lists mockups holds a `### Neighbours`
  heading with a list under it: the open changes and artifacts in the same area,
  and the components and flows the design reuses. The gate checks that the
  list exists; the reviewer judges what it says.
- `data`: the Data section starts with "None" as a word of its own,
  followed by a full stop, a colon or nothing, or an attached
  `plans/decision/v1` names in its `refs` an attached
  `changes/understanding/v1` whose `change` is this change and whose
  `subject` is `data`. As for mockups, a decision with `written_by` set
  does not count.

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
the newest plan's `paths` with SQLite's `GLOB`. Otherwise `advance` refuses
with `changes::paths::outside_plan` and names the files, because their
rules were not in the brief. Update the plan's paths with `plan`, or revert
the edit. A plan with no paths, no report on `head`, or a diff git cannot
list also fails the check.

Beyond the planned paths, the gate does not read the diff. At review, when the plan says "No UI change"
or "None", the reviewer lists the repository's UI-path and data-path
controls, `/controls/<repo>/ui-paths` and `/controls/<repo>/data-paths`,
with `controls.check`, and reports a diff that touches a path they name as
a high finding.

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
3. Otherwise it reads the acting agent's first user message: a subagent's
   own, or the main conversation's. The first line that holds text must be
   `board-brief: /changes/<name>`. The guard checks that line, and does not
   check that `changes.brief` produced it, so a parent agent can write the
   line itself.
4. It reads that change from each board that mounts the repository. It
   admits the call when the change's `phase` is `build` and its `mount` is the
   mount that holds the file; a paused change is refused and told to
   resume. It then reads `<change>/prototype`: when a prototype record is
   there, the file's checkout must also be on branch `proto/<name>`, and
   when something there does not read, the edit is refused with
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

The guard refuses an edit of every mounted repository, including the notes
repositories a personal board mounts, such as `/repos/notes-personal`. An
agent without a brief writes notes and tasks through the `tasks` and `notes`
capabilities instead.

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
tests cover the gates, the WIP limit's window and origin forms, the pull
request body, the brief's fitting of rules, and the manifest and its
committed schemas.
