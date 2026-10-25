# tasks

A Board plugin for tasks. It reads `tasks/task/v1` JSON records and
`tasks/task/v2` Markdown tasks, and extracts each task's fields into its
database. It offers three capabilities: `available` sorts the v2 tasks of the
repos in scope into the tasks to work on now, those due soon and the stale
ones; `due` lists the unfinished v1 tasks due before a date; and `next` lists
every unfinished v1 task in the order to work on them, marking each one that is
blocked.

Install it from this checkout:

    uv sync --frozen --no-dev
    ln -s "$PWD" "${XDG_DATA_HOME:-$HOME/.local/share}/board/plugins/tasks"

Then run `board plugin enable tasks` in a board. The manifest lists
`tasks/task/v1` and `tasks/task/v2` in `types`, so this writes the schemas the
plugin ships under `types/tasks/task/` to `.board/types/tasks/task/`, and names
the plugin in `.board/board.json`. A notes repo needs a copy of the v2 schema in
its own `.board/types/tasks/task/` for its tasks to be checked.

## The v2 task

A v2 task is a Markdown file under a `tasks/` directory of its repo, such as
`/tasks/write-the-plan.md` or `/projects/board/tasks/write-the-plan.md`. Its
frontmatter is the record and its body is the task's prose:

```markdown
---
$type: tasks/task/v2
title: Write the plan
status: open
due: 2026-10-30
defer: not-this-week
after: [/tasks/agree-scope.md]
from_hour: 17
contexts: [errands]
reviewed: 2026-10-25
---
The plan covers ...
```

- `title` and `status` are required. `status` is `open`, `done` or `dropped`.
- `due` and `reviewed` are dates, `YYYY-MM-DD`.
- `defer` is a date, a date and time with its offset, or a horizon counted from
  `reviewed`: `not-today` (1 day), `not-this-week` (6), `soon` (20) or
  `someday` (90). A horizon without `reviewed` defers nothing. A value longer
  than a date is read as a date and time, with `T` or `t`; one without an
  offset is local time. A task whose dates cannot be read counts as degraded.
- `after` lists the v2 tasks this task waits on, by their path in the task's own
  repo, `.md` included. `board check` reports `board::ref::dangling` and
  `board::ref::wrong_type` as for v1.
- `repeat` is `daily`, `weekdays`, `weekly-<mon..sun>`, `monthly-<1..31>` or
  `annually-<MM-DD>`. `ephemeral: true`, which needs `repeat`, marks a
  repeating task whose missed days roll forward to one instance today, with no
  backlog. No capability acts on either yet.
- `from_hour`, from 0 to 23, is the hour of the person's day from which the
  task is available. An hour before `BOARD_DAY_START` falls on the next date.
- `contexts` are lowercase tags, `[a-z0-9-]+`.

A task may hold other fields, such as the `id` and `pinned` that Reflect
writes; the plugin ignores them. So a misspelt field, such as `defr`, is
admitted and has no effect. Its scope is the repo that holds it.

## The task record

```json
{
  "$type": "tasks/task/v1",
  "title": "Write the plan",
  "status": "open",
  "due": "2026-10-20",
  "notes": "/projects/board/plan.md",
  "after": ["/tasks/agree-scope"]
}
```

- `title` and `status` are required. `status` is `open`, `waiting`, `done` or
  `dropped`. A task is unfinished while it is `open` or `waiting`.
- `due` is a date, `YYYY-MM-DD`.
- `notes` references a file, usually Markdown, that holds the task's prose:
  its phase, pull request, plan and review.
- `after` lists references to the `tasks/task/v1` records this task waits on.
  `board check` reports one that reaches nothing as `board::ref::dangling`, and
  one that reaches anything but a task as `board::ref::wrong_type`.

The record has no other fields. A project is a directory, `/projects/<name>/`,
holding a `README.md`, so a task names no project. Git history holds a task's
earlier states, so a task has no id, timestamp or revision.

## Capabilities

`board call tasks.available` chooses the repos in scope, syncs them, and
returns three sections of their `open` v2 tasks:

- `available`: the tasks not deferred, not waiting on an unfinished task in
  their `after`, and past their `from_hour`;
- `due_soon`: every task due within 7 days, overdue included, available or not;
- `stale`: the tasks neither available nor deferred that have no `reviewed`,
  or were reviewed more than 14 days ago. A deferred task, `someday` included,
  is left out until its deferral ends.

Each section lists at most 100 tasks by repo, title and path, with `total`
counting all of them. Each task carries `blocked_by`, the unfinished tasks it
waits on, and `waiting`, how many tasks wait on it. `--context <tag>` keeps the
tasks with that tag in every section; `waiting` still counts every task.

The scope is the board's own tree, `/`, with the notes repos the board mounts.
`--repo <path>` reads that one, and refuses any other path with
`tasks::repo::not_found`, listing the repos. Otherwise the remote, from
`--remote` or the caller's `origin`, chooses `/`, the personal repo and every
repo whose `notes/repo/v1` record covers it. Called outside any git
repository, every repo is in scope. Called inside one whose origin git does
not give, because none is set or git failed or ran past 10 s, the scope is `/`
and the personal repo, and `origin_unread` says why.

The mounted repos in scope sync at once, without pushing, while the call reads
the tasks at the pins it started with. A sync that runs past 5 s is left to
finish on its own, since stopping Board mid-sync would leave its lock behind,
and the repo is listed with `tasks::repo::sync_timeout`. Board's fetch has no
deadline, so a remote that hangs holds the clone's lock until git gives up.
Until then, every call whose scope holds that repo takes about 5 s and lists
it with `tasks::repo::sync_timeout`, because Board waits 5 s for the lock, as
long as the deadline. A notes write to that clone is refused with
`board::put::locked`. A mount's pin moves
only when its sync succeeded and the HEAD it reports differs from the pin, and
only then are the tasks read again. So a call where nothing changed starts one
`board query`, then the syncs beside one `board refs`. `unsynced` lists each
repo whose sync or pin failed, with its code, and the call reads that repo at
its pin. `degraded` counts the Markdown files under a `tasks/` directory in
scope that are not a readable open v2 task. `repos` lists the repos read.

An `after` that reaches a task in another repo than its task's own, such as a
task in `/` naming one in a mount, is not followed and never blocks;
`cross_repo` lists each one. Two clock edges are left as they are: a
`from_hour` before the day starts falls on the next date, and on a day whose
clock changes for daylight saving, `from_hour` is counted with the offset in
force at the call, so it can be an hour off.

`board call tasks.due --before YYYY-MM-DD` returns the `open` and `waiting`
tasks due before that date, by due date then path.

`board call tasks.next` returns at most 100 tasks; `--limit n` lists at most
`n`, from 1 to 100. It returns the `open` and `waiting` tasks by due date, with
undated tasks last, then by path. A task is `blocked` while any task in its
`after` is not `done`; `blocked_by` names each one once, by its display path,
or as written when it reaches nothing. A task waiting on a `dropped` task stays
blocked until its `after` changes. `total` counts every unfinished task, and
`truncated` says whether the limit left some out. `next` reads the `records`
view through `board query`, and resolves each `after` through `board refs`, so
a reference through a link or either of two mounts of one commit reaches the
task it names. It lists a task whose only fault is a dangling or wrong-typed
reference. Its one `board query` returns only unfinished tasks. It runs one
`board refs --field /status --field /$type` that names every listed task with
an `after`; a wait is over when its target is a task whose status is `done`
and which check degrades for nothing but a dangling or wrong-typed reference.
So one call starts at most two `board` runs, each bounded by the tasks shown
however many are done.

Both capabilities read a task ignoring fields they do not know, so a board
whose own schema admits more fields still has its tasks listed.

`board query` can join `p_tasks.tasks` (`blob`, `title`, `status`, `due`), and
for v2 `p_tasks.tasks_v2` (`blob`, `title`, `status`, `due`, `defer`,
`from_hour`, `reviewed`, `contexts` as a JSON list, and `waits`, how many
entries `after` holds), with `entries` on `blob`.

## Development

The gates, which CI runs: `uv run ruff format --check .`, `uv run ruff check .`,
`uv run pyright` and `uv run pytest`. The tests drive `$BOARD_BIN`, by default
the workspace's `target/debug/board`, so build it first with
`cargo build -p board-cli`. `plugin.json` and the schema are generated from
the pydantic models in `src/board_tasks/models.py` by `board_tasks.manifest`,
and a test checks that the committed files match.

The protocol and the `board` client come from the shared library in
`python/board-plugin/`, which `pyproject.toml` installs from this checkout.
