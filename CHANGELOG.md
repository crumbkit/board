# Changelog

## Unreleased

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
