# Changelog

## Unreleased

- A built-in plugin call keeps the board snapshots it took across other
  sessions' commits, until a refresh of the shared index or its own effects
  move them, so a plan-phase `changes next` no longer takes a new snapshot
  after each foreign commit. Its two snapshots always show one board. It
  still takes new ones after its own
  put, sync, mount update or decide, a nested call that may write, a process
  in the board's tree, and a process or read-only installed plugin during
  which HEAD moved, and before it takes its second snapshot when HEAD
  moved since the first. A host a call opens on the same board shares its
  snapshots. Each record a plugin call writes after reading it now carries
  the blob it read as its expect: the flow version that `changes start`,
  `reflow`, `migrate` and `changes run --sandbox` write, a boundary
  crossing, which expects whatever its path holds, valid or not, and the
  task autosync rewrites. When another writer stores that path first, the
  call fails with
  `board::put::stale` and writes nothing, and a rerun reads the newer
  record. Of two concurrent starts of one flow version, the second is now
  refused as stale.

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
