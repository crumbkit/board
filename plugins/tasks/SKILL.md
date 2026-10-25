---
name: tasks
description: 'Sweep the person''s tasks one at a time, marking each done, deferring, dropping or editing it, and add a task. Use it when the person asks to sweep, review or go through their tasks or to-do list, or to add a task to their own list. Start with `board -C <board> call tasks.available`, run in the repository you work in, which syncs the notes repos that repository''s git remote routes to and lists the tasks to sweep. SKILL.md and README.md in the plugin''s directory say more.'
---

# tasks

Walk the person through their tasks, one task per message, and act on each
answer. The person may be on a phone, so keep each message short.

## Sweep

1. Run `board -C <board> call tasks.available` from the repository you work
   in. Add `--context <tag>` when the person names a place, such as
   `errands`. Tell the person once, in a line each, when `unsynced`,
   `cross_repo` or `origin_unread` is non-empty or non-null, or `degraded` is
   non-zero.
2. Sweep `due_soon`, then `available`, then `stale`. Show a task once, even
   when two sections hold it. When a section's `total` is more than its
   tasks, say how many were left out.
3. For each task, send one message: its title, its repo, and only those of
   `due`, `defer`, `blocked_by` and `waiting` that are non-empty, non-zero or
   non-null. Then offer numbered choices, so one digit answers:
   1. Done: `tasks.done --path <path>`.
   2. Tomorrow: `tasks.defer --path <path> --until not-today`.
   3. Later: offer 1. not-this-week, 2. soon, 3. someday, or 4. a date
      `YYYY-MM-DD`, then `tasks.defer --path <path> --until <answer>`.
   4. Drop: `tasks.drop --path <path>`.
   5. Edit: see below.
   6. Keep: `tasks.reviewed --path <path>`, which a stale task needs to leave
      the stale section.
4. The sweep is complete when every task listed has an answer, or the person
   stops. Then say how many were done, deferred and dropped.

To add a task, run `board -C <board> call tasks.add --title '<title>'` with
the fields the person gives; `contexts` and `after` go as JSON on stdin, and
`origin_unread` needs `--repo`.

## Edit

1. Run `board -C <board> --json get <path>`, which gives the task's `text`
   and `blob`.
2. Change the text as the person says, keeping the frontmatter valid.
3. Write it with `board -C <board> put --batch`, one JSON line on stdin:
   `{"path": "<path>", "expect": "<blob>", "text": "<new text>"}`. On
   `board::put::stale`, or `board::mount::stale` in `fields.refusals`, run
   `board -C <board> mount update <mount>` for the mount case, then go back
   to step 1 once.
4. Run `tasks.reviewed --path <path>`, which pushes the edit and sets
   `reviewed`.

## Results

- `unsynced` or `unpushed` set to `board::sync::conflict` means the clone
  conflicts with its remote, and nothing is pushed until the person fixes
  it: `git pull --rebase` in the clone, then `board mount update <mount>`.
  Any other code but `tasks::repo::not_synced` means the write is committed
  on this machine only, and a later call pushes it. Tell the person the code
  once, and go on.
- `tasks::task::finished`: the task was finished elsewhere. Go on.
- `tasks::task::busy`: call again once; then tell the person, and go on.
- `tasks::task::not_plain` or `tasks::task::not_task`: offer Edit.
- Any other code: tell the person the message, and go on.
