# tasks

This plugin is still a Python program (docs/plugins/python.md). A new plugin
is a crate built into Board (docs/plugins/built-in/writing.md).

A Board plugin for tasks. It reads `tasks/task/v2` Markdown tasks through
`board query`. `available` sorts the tasks of the repos
in scope into the tasks to work on now, those due soon, the stale ones and
those an owner holds. `add`, `claim`, `done`, `defer`, `drop` and `reviewed`
write tasks.

Install it from this checkout:

    uv sync --frozen --no-dev
    ln -s "$PWD" "${XDG_DATA_HOME:-$HOME/.local/share}/board/plugins/tasks"

Then run `board plugin enable tasks` in a board. The manifest lists
`tasks/task/v2` in `types`, so this writes the schema the plugin ships under
`types/tasks/task/` to `.board/types/tasks/task/`, and names the plugin in
`.board/board.json`. In a board enabled before this release, running
`board plugin enable tasks` again removes `.board/types/tasks/task/v1.schema.json`,
which the plugin no longer ships. While a `tasks/task/v1` record is left, it
keeps that schema and lists it in `kept`. A notes repo
needs a copy of the schema in its own `.board/types/tasks/task/` for its tasks to be checked.

## The task

A task is a Markdown file under a `tasks/` directory of its repo, such as
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
owner: lead-7f3a
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
- `after` lists the tasks this task waits on, by their path in the task's own
  repo, `.md` included. `board check` reports one that reaches nothing as
  `board::ref::dangling`, and one that reaches anything but a task as
  `board::ref::wrong_type`.
- `repeat` is `daily`, `weekdays`, `weekly-<mon..sun>`, `monthly-<1..31>` or
  `annually-<MM-DD>`. `ephemeral: true`, which needs `repeat`, marks a
  repeating task whose missed days roll forward to one instance today, with no
  backlog. `done` acts on both, as Writing tasks says.
- `from_hour`, from 0 to 23, is the hour of the person's day from which the
  task is available. An hour before `BOARD_DAY_START` falls on the next date.
- `contexts` are lowercase tags, `[a-z0-9-]+`.
- `owner` names the agent or person who holds the task, such as a session id
  or a handle. It is free-form and not empty. `claim` sets it.

A task may hold other fields, such as the `id` and `pinned` that Reflect
writes; the plugin ignores them. So a misspelt field, such as `defr`, is
admitted and has no effect. Its scope is the repo that holds it.

## Capabilities

`board call tasks.available` chooses the repos in scope, syncs them, and
returns four sections of their `open` tasks:

- `available`: the tasks not deferred, not waiting on an unfinished task in
  their `after`, and past their `from_hour`;
- `due_soon`: every task due within 7 days, overdue included, available or not;
- `stale`: the tasks neither available nor deferred that have no `reviewed`,
  or were reviewed more than 14 days ago. A deferred task, `someday` included,
  is left out until its deferral ends;
- `held`: with `--owner <id>`, the tasks that owner holds that are not
  available, because they are blocked, deferred or before their `from_hour`.
  It is empty without `--owner`.

Each section lists at most 100 tasks by repo, title and path, with `total`
counting all of them. Each task carries `blocked_by`, the unfinished tasks it
waits on, and `waiting`, how many tasks wait on it. `--context <tag>` keeps the
tasks with that tag in every section, and `--owner <id>` keeps the tasks that
owner or nobody holds; `waiting` still counts every task. Each task also
carries its `owner`, or null. So a lead agent that calls
`tasks.available --owner <id>` sees every task it holds, available or not,
with the unowned ones it may take.

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
and the repo is listed with `tasks::repo::sync_timeout`. Board kills a fetch
still running after 60 s, so a remote that hangs holds the clone's lock for
at most a minute per sync. While the remote hangs, every call whose scope
holds that repo takes about 5 s and lists it with `tasks::repo::sync_timeout`,
because its own sync stalls or Board waits 5 s for the lock, as long as the
deadline. A notes write to that clone within a minute of such a call is
refused with `board::put::locked`. A mount's pin moves
only when its sync succeeded and the HEAD it reports differs from the pin, and
only then are the tasks read again. So a call where nothing changed starts one
`board query`, then the syncs beside one `board refs`. `unsynced` lists each
repo whose sync or pin failed, with its code, and the call reads that repo at
its pin. `degraded` counts the Markdown files under a `tasks/` directory in
scope that are not a readable task. `repos` lists the repos read.

An `after` that reaches a task in another repo than its task's own, such as a
task in `/` naming one in a mount, is not followed and never blocks;
`cross_repo` lists each one. Two clock edges are left as they are: a
`from_hour` before the day starts falls on the next date, and on a day whose
clock changes for daylight saving, `from_hour` is counted with the offset in
force at the call, so it can be an hour off.

`available` reads each task's frontmatter from `bodies` and checks it with the
plugin's model. A task file is degraded, whatever its status, when check
degrades it for more than a dangling or wrong-typed reference, or when the
model refuses a record the board's schema admits.

## Writing tasks

`board call tasks.add --title <title>` writes a new open task to
`tasks/<slug>.md`, its slug derived from the title as notes derives a note's,
with `-2` and on added when the name is taken. It takes `--text` for the body,
and `--due`, `--defer`, `--repeat`, `--ephemeral` and `--from_hour`; `after`
and `contexts`, which are lists, come as JSON on stdin. `--repo` names the
repo. Otherwise it is the one repo whose record covers the remote, from
`--remote` or the caller's `origin`, else the personal repo, else `/`. Two
repos that could each hold it are refused with `tasks::repo::ambiguous`. A
caller inside a git repository whose origin git does not give is refused with
`tasks::repo::origin_unread`, since the task may belong to a work repo; pass
`--remote` or `--repo`.

`claim`, `done`, `defer --until`, `drop` and `reviewed` take the task's
`--path` in the board, such as `/repos/personal/tasks/call-the-bank.md`. The path is not
checked against the caller's scope, so any task in the board can be edited by
its path. `done` sets `status: done`. A task with `repeat` stays open instead,
deferred to its next occurrence, never earlier than the date its `defer`
ends. That date is a dated `defer` as it is, or a horizon counted from
`reviewed`, as `tasks.available` counts it. The occurrence counts from that
date, so a missed day comes back, or from today without one. An `ephemeral`
task counts from the later of that date and today, so a missed day is not
kept.
`monthly-31` falls on the month's last day when it is shorter, and
`annually-02-29` on 28 February.
`defer --until` takes a date or a horizon, and writes a horizon as the date it
ends, counted from today. `drop` sets `status: dropped`. `claim`, `done`,
`defer` and `drop` refuse a task that is not open with
`tasks::task::finished`; `reviewed` marks any task. `claim --owner <id>`
sets `owner` on an open task that nobody or the same owner holds, and refuses
one another owner holds with `tasks::task::held`, whose `owner` field names
the holder. `done`, `defer`, `drop` and `reviewed` leave `owner` as it is, so
a finished task still says who held it. No call clears `owner`; edit the task
to hand it on.

Each call sets `reviewed` to today. Today is the person's day, which starts at
`BOARD_DAY_START`, so a call at 01:00 with a day that starts at 07:00 writes
yesterday's date. The calls edit the frontmatter's lines in place. Each sets
the line of a key it writes, or adds one before the closing `---`, as
`key: "value"`, and keeps every other byte. A key whose line is not one plain
scalar, such as a block or a list, or a key given twice, is refused with
`tasks::task::not_plain`. A path with no file is refused with
`tasks::task::not_found`. One that is not a valid task under a `tasks/`
directory, such as one with `repeat: fortnightly`, is refused with
`tasks::task::not_task`, and `board check` says what is wrong. A task in a
mount without `daily/`, which is not a notes repo, is refused with
`tasks::repo::not_found`.

A call syncs a mounted repo, pushing, before it writes. It writes after any
sync failure, such as an unreachable remote, and `unsynced` holds the failure's
code, such as `board::sync::failed`. The write is committed in the clone and
pushed by a later call. A clone left in `board::sync::conflict` stays there
until it is mended by hand: run `git pull --rebase` in the clone, resolve the
conflict, then run `board mount update <mount>`. The call writes with `board put` and the blob it read, reads again and retries when the
task or the clone moved, and refuses with `tasks::task::busy` when the task
holds an edit not committed in its clone or kept changing. A new read that
finds the text the refused write sent counts as that write. It then pushes and
moves the mount's pin to the pushed commit, and `unpushed` holds the code of a
push that failed; the write stays committed in the clone.
Both are `tasks::repo::not_synced` for the board's own tree, which is never
synced. `commit` is None when the board's own tree does not commit puts.

## The sweep skill

The directory also holds `SKILL.md`, the instructions an agent follows to
sweep the person's tasks one at a time, in Claude Code or Codex. From a phone
it works through a Claude Code session on the machine that holds the board,
such as one under Remote Control; a cloud session has no board. Link the
directory into the agents' shared skills to install it:

    ln -s "$PWD" ~/.agents/skills/tasks

Claude Code reads `~/.claude/skills`; link it there too unless that directory
is itself a link to `~/.agents/skills`.

## Development

The gates, which CI runs: `uv run ruff format --check .`, `uv run ruff check .`,
`uv run pyright` and `uv run pytest`. The tests drive `$BOARD_BIN`, by default
the workspace's `target/debug/board`, so build it first with
`cargo build -p board-cli`. `plugin.json` and the schema are generated from
the pydantic models in `src/board_tasks/models.py` by `board_tasks.manifest`,
and a test checks that the committed files match.

The protocol and the `board` client come from the shared library in
`python/board-plugin/`, which `pyproject.toml` installs from this checkout.
