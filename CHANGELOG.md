# Changelog

## Unreleased

- The development flow's suite-run remedy names only commands that exist.
  When the change names no suite, it says to name one with `board changes
  attach --suite`, write a `runs/suite/v1` with `board put /suites/<name>`,
  or ask the owner to waive the check. Otherwise it says to record CI's
  run with `board github ci --change`, or to write the run with `board put
  --new --var suite=<name>` and attach it with `board changes attach
  --ref`. It no longer names `board runs define` or `board runs run`,
  which the runs plugin does not offer. A test fails when an instruction
  or remedy in a shipped flow names a `board` command that no built-in
  plugin offers and Board does not have. A change already on an older
  stored flow version prints the old remedy until `board changes reflow
  --path <change>` moves it to the shipped one.

## 0.1.6 (2026-11-28)

- The plans and effects Claude Code mods run `board` on their minute's tick
  only when the board's HEAD commit or Git index changed since their last
  read that worked. They read both as files, with no process. On a board
  with no new commit, a session's two mods run no `board` between the
  owner's own reads, where they ran about 1 s of `board` each minute. The
  pane and an answer still read at once, and a new commit shows within one
  tick.
- A change's plan can list tasks under a `## Closes at merge` heading,
  one board path each. The build gate's `tasks` check passes an open
  attached task listed there, says which tasks close at merge, and misses
  a listed path that is not a task of the change's, saying to attach it or
  remove it. When the pull request merges, `changes advance` (which
  `board github merge --change` runs) and `changes sync` close each listed
  task through `tasks.done` before they write the change as done. A
  refused close leaves the change in its phase: `advance` refuses with
  `changes::land::tasks_not_closed` and `sync` reports `owed`, and a rerun
  skips the tasks already closed. `sync --dry_run true` names the tasks it
  would close and closes none. `advance` into done refuses with
  `changes::land::tasks_open` while an attached task the plan does not
  name is open, and with `changes::land::no_merge` while a named task is
  open on a change that records no pull request, which never closes a
  task. Only a level-two heading counts, a fenced code block is not read,
  and a Markdown link's target counts as a path.
- A `plans/decision/v1` can name the one board entry it decides in
  `decides`, which `board plans decide --decides <path>` sets. The
  `owner-approval` check counts only an explicit approval of the plan that
  names the revision it approves: the owner's answer choosing the
  `approve` option of an attached question that blocks the change and whose
  `approve` option names a plan revision, as the question `changes ask`
  files does; a Touch ID approval; or an owner decision whose refs name the
  change and whose `decides` is a plan revision. Any other decision or
  answer, such as a choice to revise or a Data sign-off that names the
  current revision in its refs, no longer approves the plan, and an
  existing approval decision without `decides` must be asked again. A
  board whose `.board/types/` holds the earlier `plans/decision/v1` copy
  refuses `decides` until `board plugin enable` replaces it. The check binds each approval to the plan revision it
  approved, and stops counting it once a later revision changes the plan's
  typed answers or its Outcome, Out of scope, UX or Data section, or once
  Board no longer reads that revision. A rewrite of the plan's paths, Tests
  sections or other routine parts keeps the approval, including a Touch ID
  approval, and the check's detail names the approved revision and what
  changed since. The `mockups` check counts an agreement that names a
  mockup version only for that version. It reads the version anywhere in
  the agreement's words when the agreement names one mockup, and only right
  after each mockup's name when it names several.
- A notes repository that the board reaches through two mounts, such as
  `/repos/notes-personal` and the same repository mounted again by a board
  mounted at `/repos/team`, counts as one repository. `board tasks add`,
  `board changes start` and `board notes daily` used to refuse with
  `tasks::repo::ambiguous` or `notes::repo::ambiguous`, naming both mounts.
  They now write to the mount whose pointer is in the board's own tree,
  and the output's `about` names it. Among nested mounts, the one with the
  shortest path stands for the repository. Mounts count as one repository
  when Board read them from the same clone. `board tasks available` and
  `board tasks tree` show the repository's tasks once, including after a
  sync moves a pin onto the other mount's commit. Two different
  repositories that both cover the remote still refuse as before.
- The `board-changes` mod's edit guard remembers the last verdict on each
  agent's edits of each repository, counting an edit that needed no advice.
  A later edit by the same agent whose line would say what that verdict
  said, apart from the file, runs with no line. A line that says something
  new, such as another phase, branch or change, is given in full, and so is
  advice that returns after an edit that needed none. Advice
  that a lookup failed is still given on every edit, and an edit of a
  board record is still refused every time. The memory lasts until the mod
  reloads, as on `/clear` and `/resume`.
- A refusal with `board::path::not_found` names, in a `nearest` field and
  in its hint, at most 5 entries in the deepest ancestor that resolves
  that are close to the segment that missed: first each that starts with
  it, such as `notes.md` for `notes`, then each within an edit distance of
  a third of its length, such as a stamp one minute off, then each that
  shares a long prefix with it, such as a slug cut at another length. The
  hint reads `nearest entries in /projects/p/decisions: <names>; run
  board ls /projects/p/decisions for all`. A directory of over 10,000
  entries, or one with no close name, offers none, and the hint says to
  run `board ls` as before. Board never reads a nearest path in place of
  the one asked for.
- A word an agent guesses after a plugin's name runs the capability it
  means: `changes show` and `changes status` run `changes next`,
  `changes list` runs `changes open`, and `tasks list`, `plans list` and
  `evals list` run `tasks available`, `plans open` and `evals open`, with
  the same flags. `board call <plugin>.<word>` does the same. Each prints
  `note: board changes show is board changes next` or the like on stderr.
  A plugin that offers the word itself still runs its own. `runs run` and
  `runs record` are still refused, and their hint says to write the run
  with `board put --new --var suite=<name>` and attach it with
  `board changes attach --ref`.
- `board changes plan` writes a plan it used to refuse for a fault the
  gates catch at `advance`, and lists the fault in `warnings`. A plan over
  3,000 words warns `changes::plan::too_long` and used documents over
  3,000 words in all warn `changes::plan::uses_too_long`, each with the
  word count; a plan over 6,000 words is still refused. A reference in
  `uses`, `considered` or `owed` that reaches nothing is left out of the
  plan and listed under `changes::ref::dangling`. The new warning
  `changes::plan::sections_missing` names each heading a `sections` check
  of the change's flow requires and the text lacks, in any phase, so a
  development plan learns of its two test sections at plan time. A
  directory in `uses`, a path outside the mount, a build plan with no
  paths and a plan headed by another change's title are still refused.
  `changes plan-amend` keeps its 3,000-word refusal.
- `board tasks available` does not fetch a notes clone that was fetched
  less than a minute ago and already holds what that fetch brought, so a
  sweep of calls in a row fetches once. A task pushed from elsewhere in that
  minute is listed by the first call after it. `board refs`, and each plugin
  call that asks `host.refs`, finds each path's references by a lookup built
  once per run in place of a scan of every reference for each path. The
  answers are the same as before.
- `board timing check` goes live one command at a time. A command turns
  live once it has 100 judged local results, 14 CI results once CI has
  timed it, and under 1 in 10 of its over-budget results cleared at the
  next one, and it never turns silent again. Only a live command over
  budget opens `/tasks/board-speed-<command>.md`. The first live command
  writes the zero rules `/rules/board/speed-budgets` and
  `/rules/board/speed-budgets-ci`, so the rules badge shows a live command
  over budget in every session. Once the lines changed in `/repos/board`
  since the task opened pass `overflow-agent-grace` (200 by default), a
  check run with `--session <local_ id>` files one question for the owner,
  named as that session's; the rules mod now passes it. The check also
  judges a median budget, prints each command's median budget and mode,
  and gains budgets for `ls`, `changes.advance` and `changes.brief`.
- On each push to main, timing.yml times every budgeted command on Linux
  with `cargo xtask bench --hot` on a generated board with 100 open
  changes, reporting only, and uploads the run. `board timing check
  --record` imports the newest one into `/suites/board-speed-ci` through
  gh; when gh cannot, it warns and still records the local run.

## 0.1.5 (2026-11-27)

- `board --personal requests threads` returns the thread of every pending
  request in one call, as `{"threads": [...]}`, each item shaped as
  `requests.thread` returns it. It reads the effects once and the replies at
  most once, however many threads it lists, so a caller such as Board.app's
  request relay need not run `requests.thread` once per request.
- The Board band draws a green `Ready to land N` button before Waiting when
  at least one change stands in its flow's last phase and waits only on the
  owner's checks, or is done with rechecks still owed. Pressing it opens a
  pane that lists each change with the check it waits on and the step that
  clears it. A new read-only capability, `changes.ready`, lists these rows,
  and the band's Waiting button leaves out the questions these rows name,
  so each check is counted once. Rows that fail CI, conflict with the
  branch or have a stale review still count in N; the red `· N failing`
  part stays off by default.
- `board changes plan-amend --path /changes/<name>` writes a new plan
  version that only adds text under the "What the tests prove" and "What
  the tests do not prove" headings of the change's current plan, so an
  agent no longer resends the whole plan at land for those sections. It
  takes no paths and no owed entries, which still need the owner's resend.
  It records each addition in a `changes/amendment/v1`, which `changes
  next` lists as `amended` and the pull request body lists under "Plan
  amendments". At land, `changes next` and the `test-sections` check's
  remedy name `changes plan-amend` in place of a whole-plan resend.
- A query that looks entries or records up by logical path, blob or record
  type reads an index in place of every entry, so plugin calls that look
  records up, such as `plans open` and `changes next`, answer faster. The
  rows every view returns are the same as before. The index format moves to
  31, so the first run after the upgrade builds a new index from scratch.

## 0.1.4 (2026-11-26)

- A warm `board refs`, and each plugin call that asks `host.refs`, reads the
  references of each source the index keeps a decision for, with where they
  resolved, from a new `resolutions` table, and decides only the sources
  whose decision it does not hold. The answers are the same as before. The
  index format moves to 30, so the first run after the upgrade builds a new
  index from scratch.
- `board changes open` and `board changes owed` choose which recently done
  changes may still owe a check from their flows and plan answers, read in
  a few queries, and observe only the changes left. On a board with about
  600 changes done in the last 14 days, `changes open` takes about 2.6 s in
  place of 84 s. The output is the same as before.
- `board plans decide --project <project> --decision <text> --why <text>
  --session <local_ id>` records a `plans/decision/v1` under the project's
  decisions folder and prints its path. It fills in `$type`, `decided` and
  `by`, takes `--person`, `--program`, a bare `--review` and repeated
  `--alternatives`, `--refs` and `--urls`, and refuses a call that names
  neither a session nor a person with `plans::decide::no_author`. When the
  project held nothing before, it still writes the decision and warns with
  the three nearest project names. The plans skill now records decisions
  with it in place of a hand-built record and `put --new`.
- `board put`, `put --new` and `put --batch` fill a field Board knows when
  the record leaves it out: a question's or a decision's `by`, from
  `BOARD_SESSION` or a `local_` Claude Code session id, its `asked` or
  `decided` time, a question's `status` of `open`, and a review report's
  `patch_ids`, from its `range` in the directory the put runs in. They also
  drop a field the schema does not name, at any depth, with one warning on
  stderr per field that names a known field one edit away, such as
  `/alternatives`. A field one edit from a field the record lacks is
  refused with a hint to rename it, and a fill that could not run prints
  why. `--json` lists both in `filled` and `dropped`, and `--strict`
  refuses as before. A plugin's write through its host fills and drops
  nothing.
- Board runs the command an agent meant for a few common guesses that
  clap refused: `board cat` and `board show` run `get`, `board list` runs
  `ls` and `board schema` runs `schemas`; a long flag given with hyphens
  for underscores, or the reverse, such as `--task-repo` or `--dry-run`,
  runs with the flag's own spelling; and `--path` given to `get`, `ls`,
  `check`, `log`, `refs`, `uri` or `mount update`, such as
  `board get --path /a`, is read as the path. `board cat` used to be
  refused with a hint to run `get`.
- The rules plugin's README lists each code `board rules report` can put
  in `refused`, says which are warnings that leave every row as it is, and
  says that `rules::report::run_commit_unnamed` also covers a malformed
  `versions.commit` and a run Board cannot read.
- `board schemas show` prints the schema each built-in plugin ships for its
  record types, such as `rules/report/v1`, as `board plugin enable` writes
  it, in any directory and whether or not a board enables the plugin.
  `board schemas list` does not list them.
- An `obligations` check on a change with a head fails, with git's reason,
  when git cannot list the change's diff. Before, it passed on the plan's
  paths alone. It asks `rules.for-paths` about the files a change adds 100
  to a call, so a large diff no longer sends them all at once.

## 0.1.3 (2026-11-25)

- `board rules report --path <path>` prints one `rules/report/v1` document
  of every rule, group and compliance suite that reaches a repository, for
  its CI to upload: each rule's result, each group's rolled-up state, each
  suite's newest clean full run with the commits since it, where the
  repository's own rules differ from the imposed set, every refusal, and
  the GitHub Actions run that made it. It writes nothing and exits 0
  whatever the rules found. A degraded rule is one `unreadable` row, and
  the other rules are still measured. A new `rules/group/v1` record names a
  group of rules. A board refuses rules calls with
  `board::plugin::schema_differs` until `board plugin enable rules` writes
  the two new schemas.
- Two shared record shapes join `board_plugin::shape`: `CodeName`, a code
  such as `board::ref::dangling`, and `DottedVersion`, such as `1.99.1`,
  which a version rule's `at_least` now uses with the same pattern.

## 0.1.2 (2026-11-25)

- The review gate's `paths` check passes when the change's diff edits files
  that no path of its plan matches. Its detail says that these files are
  outside the plan's paths and that the change adds them to its paths
  without a new plan, and names them. `changes pr` lists them in the pull
  request body under "Files added to the plan's paths", or says there that
  the diff could not be listed and why. An agent no longer writes its plan
  again to add a file it needs. The check still refuses with
  `changes::paths::outside_plan` when the plan has no paths or git cannot
  list the diff. The `ui-paths` and `data-paths` checks still read every
  edited file. An `obligations` check in any phase counts the obligations on
  the added files as well as on the planned paths, and says so in its
  detail. The detail names at most ten added files and the body at most 50,
  each with a count of the rest. A change in flight keeps
  the review instruction of the flow version it stored, which still says
  the gate refuses such a file; run `board changes reflow --path <change>
  --flow development` to take up the new instruction.

- Board marks its cache directory, `~/.cache/board` or
  `$XDG_CACHE_HOME/board`, as a cache. It writes `CACHEDIR.TAG` there, which
  backup tools that honour the tag skip, and on macOS it first excludes the
  directory from Time Machine, as `tmutil addexclusion` does. A directory
  Board cannot mark usually gives the warning `board::cache::unmarked`, and
  the run goes on; the next run tries again.
- `board call rules.check --path /` answers within Board's 1-second query
  limit on a board that mounts many repositories. Before, it failed there
  with `board::query::timeout` on every run, and the rules mod's session
  start fell back to asking `rules.for-path` before each edit.
- A warm `board ls` reads the decision Board kept for each mounted source
  wholly in its scope instead of deciding every source again. Its output is
  the same as a cold run's.
- The Claude Code rules mod starts a session with the rules from its last
  good check of the repository, says when that check ran, and runs
  `rules.check` again in the background with a 120-second limit. A
  repository it has not checked before waits up to 5 seconds, as before.
- A mounted source read from a partial clone, whose repository lacks some
  blobs, is kept between warm runs until its repository's objects change.
- A warm run opens a mounted repository only when its stamp changed, and
  after a working-tree change decides again only the entries the change
  could alter. A warm query, call or check of an unchanged board reuses the
  working tree's kept decision and lists no directory.
- `board changes next` and `board changes advance` read through one board
  snapshot, so each takes about half the time.
- Board records how long each hot command takes, and
  `board timing check` judges each one's p90 against the owner's budgets.
  The check is silent: it opens no task and files no question yet.
- The built-in `orders` plugin checks signed orders against
  `~/.config/board/allowed_signers`, falling back to the attention file
  with the warning `orders::signers::legacy`. `board orders pinned`,
  `verify`, `sign` and `signers` replace the `attention.*` calls, which
  keep working in this release and print their replacement.
- The handoff mod prompts a session once when it reaches the
  `handoff-context-tokens` limit, and stops telling sessions to hand off
  once the usage window is back under its share.
- `board github push` and `pr-create` no longer refuse a diff because the
  bytes of a binary file match a text rule; the result lists each binary
  file that was not scanned.
- `tasks.claim` in a notes repository whose clone still stores the
  `tasks/task/v2` schema from before the shapes migration refuses with the
  new code `tasks::repo::schema_stale`, whose hint names the commands that
  migrate the clone. Before, the claim failed with `board::record::invalid`.
  The claim syncs the clone before it checks, so a repository another clone
  migrated and pushed is claimed without a refusal.
- A review with no high finding still counts after one fix round when a
  `changes/fix/v1` record names the report and the commits that answer its
  findings.
- The development flow asks the owner's data rechecks at land without
  blocking the merge, and `board changes owed` lists each change done in
  the last 14 days that still owes one. A plan that answers `ux` none and
  `data` none needs no owner approval. `board changes ask` files one
  question whose approving option passes the owner approval, data and
  mockup checks together. A review still counts after later commits that
  edit only record paths or cleanly merge main.
- A `runs/suite/v1` record has an optional `kind`: a compliance check, a
  benchmark, an eval or a test. `runs.migrate` adds it to existing suites,
  and classifies a suite whose name ends in `-tests` as a test suite. Run
  `board plugin enable runs` first: while the board holds 0.1.1's
  `runs/suite/v1` schema, `runs.migrate` refuses with
  `board::plugin::schema_differs` and writes nothing.
- The Claude Code questions badge reads Board only for a session whose band
  is drawn.

- In the plan phase, `board changes next` and `board changes advance` ask
  the rules plugin for the rules on the plan's paths once, where they asked
  twice, and read both the obligations and the rules on the planned paths
  from that answer. The gate's checks and their details are unchanged.

- `board call tasks.add` reads each `--after` and `--refs` value as a board
  path. A value without a leading `/` is read from the current directory, and
  a value that joins paths with a comma, such as
  `--after /tasks/a.md,/tasks/b.md`, is refused with a hint to repeat the flag.
  Before, such a value was written as one prerequisite that names no task.
  Each `after` path is now written as a path in the task's own repo, as
  `parent` and `brief` are, so `/repos/work/tasks/a.md` on a task in
  `/repos/work` is written as `/tasks/a.md`, and an `after` in another repo
  is refused with `tasks::add::cross_repo`.
  The `changes.plan` descriptions now say that `uses` and `paths` can be
  given as repeated flags.

- `tasks add` says where the task went. Its output gains `about`, one
  sentence naming the repo that took the task and whether it left this Mac,
  `remote`, the remote that repo pushes to, and `leaves_this_mac`, true when
  the call pushed it. `--repo /` now needs `--reason`, and is refused without
  one with `tasks::repo::reason_needed`, which names the personal notes repo.
  The hints of `tasks::repo::origin_unread` and `tasks::repo::ambiguous` give
  a full command for each repo that could take the task. A script that
  passes `--repo /` must add `--reason`.

- `board call` takes a capability's input property that is a list of
  strings as a flag given once for each item, in the order given, such as
  `board call tasks.add --title t --after /tasks/a.md --after /tasks/b.md`.
  `--after none` alone sends an empty list. An item whose schema marks it
  as a board path is read from the current directory, as a single path flag
  is, and one that holds a comma is refused with a hint to repeat the flag.
  A list of objects, such as `considered` on `changes.plan`, still goes on
  stdin, and a call with no flag still reads its whole input, lists
  included, as JSON on stdin. `board commands list --json` marks each of these flags
  `many: true`.

- A `plans/question/v1` whose `blocks` names a `tasks/task/v3`, and an
  `artifacts/artifact/v1` whose `subject` names one, are no longer degraded
  with `board::ref::wrong_type`. Run `board plugin enable plans` and
  `board plugin enable artifacts` to replace a board's copies of the two
  schemas, then `board check` to clear the degraded records.

- `board search '<query>'` lists the records of the board and its mounts
  that match a query in the first slice of Flare's search syntax: words,
  `"phrases"`, `type:` and `from:` filters, commas for any-of and a leading
  `-` to exclude. `--json` prints the canonical query, its parsed parts,
  the results and a cursor for the next page, and `--filters` lists the
  filters and short type names. docs/commands/search.md describes it.

- `board changes plan` refuses a plan whose first heading is the title of
  another change that is not dropped, with `changes::plan::other_change`,
  and warns with `changes::plan::title_differs` when the first heading
  differs from the change's title in any other way. With
  `"correct": true` it writes a new plan for a done or dropped change, which
  keeps its phase.

- A confirmed priority in `board priorities list` also turns stale 7 days
  after its confirmation, even on a board with few commits.

- `board changes advance` moves a change that records a pull request into
  `done` only once GitHub reports the pull request merged, and records its
  merge commit. Before the merge it refuses with
  `changes::land::not_merged`, which says to merge with
  `board github merge --change`; a gh failure is
  `changes::land::gh_failed`. A run whose stop is `done` no longer lets a
  change end before its pull request merges.

- The new `map` plugin keeps a map as one `map/layout/v1` record at
  `/maps/<name>`: a sheet, titled regions, items with a stored position
  and label side, typed lines, and notes anchored to an item or a line.
  `board map check --path /maps/<name>` lists what a reader would see as
  broken, with the box it estimated for each text, and counts the findings
  the map accepts apart.

## 0.1.1 (2026-11-23)

- `board github push`, `board github pr-create` and the pre-push hook work
  with git 2.39, the git that Xcode 16 ships. Board needs git 2.39 or later.

- The new `priorities` plugin keeps the owner's priorities in one
  `priorities/list/v1` record at `/priorities`, ranked in a short and a
  long horizon. `board priorities list` gives each entry with what it
  covers and whether the owner must confirm it again; `set`, `move`,
  `drop` and `confirm` keep it current.
- `board share to` no longer copies a back-ref whose type's schema carries
  `"x-board-goes-with-share": false`, and lists it in `not_copied` with the
  reason `withheld`.
- The params plugin ships the heuristic `priorities-reconfirm`.

- A task is a `tasks/task/v3` record, which names its records and files in
  `refs`. `board tasks add` writes v3 for every task except a project, and
  `board tasks migrate` rewrites v2 tasks as v3.
- A notes repo whose `.board/types/tasks/task/` holds only `v2.schema.json`
  still gets v2 tasks from `board tasks add`, and `add` refuses `refs` there
  with `tasks::add::no_v3_schema`. To write v3 tasks there, copy
  `.board/types/tasks/task/v3.schema.json` from the board into the notes
  repo and commit it.
- A Board 0.1.0 reader does not know `tasks/task/v3`, so it lists no task
  migrated to v3. Upgrade every Board that reads a board before you run
  `board tasks migrate --yes` in it.
