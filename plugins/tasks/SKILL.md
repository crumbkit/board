---
name: tasks
description: 'Before you fix a bug or build a feature, use it to check whether an open task already covers the work, and claim that task and finish it with tasks.done instead of adding a new one. Also use it to sweep the person''s tasks one at a time, marking each done, deferring, dropping or editing it, when the person asks to sweep, review or go through their tasks or to-do list, and to add a task to their own list. Start with `board -C <board> tasks available`, run in the repository you work in, which syncs the notes repos that repository''s git remote routes to and lists the tasks to sweep. SKILL.md and README.md in the plugin''s directory say more.'
---

# tasks

Keep the person's tasks. Before you start work on a bug or a feature, check
whether an open task already covers it. When the person asks, walk them
through their tasks, one task per message, and act on each answer. The person
may be on a phone, so keep each message short.

## Before you start work

1. Run `board -C <board> tasks available` from the repository you work
   in. It syncs the notes repos and shows only the tasks that are ready, so
   it can miss a deferred or blocked task that covers the work. Then run
   `board -C <board> tasks tree --root true`, which lists every task in
   every status at the synced pins. Run them in that order, because
   `tasks.tree` does not sync. Look for an open task that covers the work.
2. When one does, claim it with
   `tasks.claim --path <path> --session <your session's local_ id>` and
   finish it with `tasks.done --path <path>`. On `tasks::task::held`,
   another session holds the task: stop and tell the person.
3. When none covers the work, add one. If the work goes through the changes
   plugin, run `changes.start` without `--task`, which adds the task.
   Otherwise run `tasks.add`.
4. When the work goes through the changes plugin and you claimed a task, pass
   it with `changes.start --task <path>`.
5. Close the task with `tasks.done --path <path>` when the work it names is
   committed. For work that goes through the changes plugin, close the
   change's task when its pull request merges or when `changes.sync` ends
   the change; `changes.sync` never closes it. When you are told not to
   push, close it with `tasks.done --path <path> --push false`, which
   commits in the notes clone and pushes nothing; a later write call pushes
   that commit.

## Sweep

1. Run `board -C <board> tasks available` from the repository you work
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

To add a task, run `board -C <board> tasks add --title '<title>'` with
the fields the person gives; `contexts` and `after` go as JSON on stdin, and
`origin_unread` needs `--repo`. To point someone else at a task in a shared
board, give its URI: `board -C <board> uri <path>` prints one, such as
`board://acme/team-board/tasks/fix-login`, which `board get` reads from any
clone the reader's known boards list.

## Sweep as a lead

An agent that coordinates other agents keeps the work it owes in tasks, not in
its conversation, which forgets in a busy period. Use one id for yourself,
such as your session's `local_` id as `--session`, in every call.

1. At the start of each turn, run
   `board -C <board> tasks available --session <your id>`. `held` lists
   your tasks that wait, and `available` the ones you hold or may take.
2. Before you start a task, run
   `board -C <board> tasks claim --path <path> --session <your id>`.
   On `tasks::task::held`, another owner has it; take another task.
3. When a turn leaves work owed, add a task for it with `tasks.add` and claim
   it. Never hold owed work only in the conversation.
4. Finish a task with `tasks.done`, or `tasks.drop` when it is no longer owed.
5. Now and then, run `board -C <board> tasks available` without
   `--session`. A task held by a session that has ended is hidden from every
   other owner's sweep. Hand each one on by changing its `owner` line, as
   Edit below says.

## Keep a project tree

A project is a task with `kind: project`. Each task or project has at most one
`parent`, so the work forms a tree, and a parent waits on the open tasks under
it.

1. When agreed work belongs to a larger goal, add it under that goal:
   `tasks.add --title '<title>' --parent <path>`, with the path as
   `tasks.tree` lists it. Start a project with `--kind project`, and
   `--brief <path>` when it has a brief.
2. A task that needs a project another task holds lists the project in
   `after`. Never give a task a second parent.
3. Reshape the tree with `tasks.move --path <task> --parent <task>`, or
   `--parent none` to make it top-level. The task's path never changes, so
   no reference breaks. On `tasks::move::cycle`, choose a parent outside the
   task's tree.
4. See a project whole with `tasks.tree --path <path>`, or every tree with
   `tasks.tree --root true`. When `blocked_by` of a parent is empty, every
   task under it is finished: mark it done.

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
  `tasks::repo::not_pushed` means `--push false` kept the write on this
  machine. Any other code but `tasks::repo::not_synced` means the write is
  committed on this machine only, and a later call pushes it. Tell the
  person the code once, and go on.
- `tasks::task::finished`: the task was finished elsewhere. Go on.
- `tasks::task::held`: another owner holds the task. Go on.
- `tasks::task::busy`: call again once; then tell the person, and go on.
- `tasks::task::not_plain` or `tasks::task::not_task`: offer Edit.
- Any other code: tell the person the message, and go on.
