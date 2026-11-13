# tasks

A Board plugin, built into Board as `crates/board-plugin-tasks`, for tasks.
It reads `tasks/task/v2` Markdown tasks with Board's query. `available` sorts
the tasks of the repos in scope into the tasks to work on now, those due soon,
the stale ones and those an owner holds. `tree` lists tasks and projects as
one tree. `add`, `claim`, `done`, `defer`, `drop`, `reviewed` and `move` write
tasks.

Run `board plugin enable tasks` in a board, which writes the schema the
plugin ships to `.board/types/tasks/task/` and names the plugin in
`.board/board.json`. A notes repo needs a copy of the schema in its own
`.board/types/tasks/task/` for its tasks to be checked. An installed copy of
the former Python plugin under `$XDG_DATA_HOME/board/plugins/tasks` must be
removed, or Board refuses every tasks call with `board::plugin::duplicate`.

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
owner: {"program": "claude-code", "session": "local_7f3a2b1c-0d9e-4f8a-b7c6-5d4e3f2a1b0c"}
parent: /tasks/ship-the-release.md
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
- `contexts` are slugs: lowercase letters and digits in words joined by `-`.
- `owner` is the `Actor` who holds the task: an object of `session` (a
  `local_` id), `program` (a slug such as `claude-code`), `person` (a
  slug or an email) and `agent` (the name the agent was given), each
  optional and at least one present, written as one line of JSON. `claim`
  sets it.
- `parent` names the one task this task is part of, by its path in the
  task's own repo, as `after` does, and `board check` reports it the same way.
  A second task that needs the same work lists it in `after` instead.
- `kind` is `task`, as when it is left out, or `project`. A project may name
  its brief, such as `/projects/flows/README.md`, in `brief`, a reference to
  any file in the repo. Any task or project may be the parent of either.
- `id` and `pinned` are fields Reflect writes, and the plugin ignores them.
  `id` is the note's id, a slug. `pinned` is `true` or `false`, or the
  note's place in Reflect's sidebar, a whole number.

A task holds no other field. `board check` reports a task with a field the
schema does not name, such as a misspelt `defr`, as `board::record::invalid`,
and `put` refuses to write one with that code, naming the field. Keep notes
that no field names in the body. Its scope is the repo that holds it.

A task's file name is its slug, set from the title when `add` writes it. It
never changes: a new title leaves the file where it is, so every reference to
the task, such as a `parent` or an `after`, keeps working.

## Capabilities

`board tasks available` chooses the repos in scope, syncs them, and
returns four sections of their `open` tasks:

- `available`: the tasks not deferred, not waiting on an unfinished task in
  their `after`, and past their `from_hour`;
- `due_soon`: every task due within 7 days, overdue included, available or not;
- `stale`: the tasks neither available nor deferred that have no `reviewed`,
  or were reviewed more than 14 days ago. A deferred task, `someday` included,
  is left out until its deferral ends;
- `held`: with an owner, given by `--session`, `--program`, `--person` and
  `--agent`, or by `--owner` alone (see `claim`), the tasks that owner holds that are not available, because they are
  blocked, deferred or before their `from_hour`. It is empty without one.

Each section lists at most 100 tasks by repo, title and path, with `total`
counting all of them. Each task carries `blocked_by`, the unfinished tasks it
waits on, and `waiting`, how many tasks wait on it. A parent waits on its open
subtasks as if they were in its `after`: `blocked_by` lists its open `after`
targets, then each open child, and under a finished child that child's own
open ones. So a parent is available once every task under it is done or
dropped, and the agent then marks it done; nothing closes it on its own. Each
task also carries `parent`, `subtasks` and `tree_path`, as Projects and trees
says. `--context <tag>` keeps the
tasks with that tag in every section, and an owner keeps the tasks that
owner or nobody holds; `waiting` still counts every task. Each task also
carries its `owner`, or null. So a lead agent that calls
`tasks.available --session <id>` sees every task it holds, available or not,
with the unowned ones it may take.

The scope is the board's own tree, `/`, with the notes repos the board mounts.
`--repo <path>` reads that one, and refuses any other path with
`tasks::repo::not_found`, listing the repos. Otherwise the remote, from
`--remote` or the caller's `origin`, chooses `/`, the personal repo and every
repo whose `notes/repo/v1` record covers it. Called outside any git
repository, every repo is in scope. Called inside one whose origin git does
not give, because none is set or git failed or ran past 10 s, the scope is `/`
and the personal repo, and `origin_unread` says why.

The mounted repos in scope sync one after another, without pushing. Board
stops a sync that runs past 5 s, with git and every process git started, and
the repo is listed with `tasks::repo::sync_timeout`. So a call whose scope
holds a repo whose remote hangs takes about 5 s more for that repo, and
leaves no lock behind. A mount's pin moves
only when its sync succeeded and the HEAD it reports differs from the pin, and
only then are the tasks read again. So a call where nothing changed runs one
query, and one refs when a task has an `after` or a `parent`. `unsynced` lists each
repo whose sync or pin failed, with its code, and the call reads that repo at
its pin. `degraded` counts the Markdown files under a `tasks/` directory in
scope that are not a readable task. `repos` lists the repos read.

An `after` or a `parent` that reaches a task in another repo than its task's
own, such as a task in `/` naming one in a mount, is not followed and never
blocks; `cross_repo` lists each one, with its `path` and its `after` or
`parent`. Two clock edges are left as they are: a
`from_hour` before the day starts falls on the next date, and on a day whose
clock changes for daylight saving, `from_hour` is counted with the offset in
force at the call, so it can be an hour off.

`available` reads each task's frontmatter from `bodies` and checks it with the
plugin's model. A task file is degraded, whatever its status, when check
degrades it for more than a dangling or wrong-typed reference, or when the
model refuses a record the board's schema admits.

## Projects and trees

The `parent` links of a repo's tasks, in every status, make a tree. A link
that reaches no task, or one in another repo, leaves its task at the top. A
loop of links is cut at the task with the first path in it, which then counts
as a top-level task and is listed in `cycles`.

- `parent` is the parent's path in the board, or null for a top-level task.
- `subtasks` is `{"open": n, "total": m}`, counting every task under it.
- `tree_path` joins the slugs from the top-level task down to the task, such
  as `ship-flows-editor/dispatch-owns-flow-documents/choose-store`. A move
  changes it, and nothing else.

`board tasks tree --path <task>` returns the task and every task under
it, nested, in every status, so finished pieces still show. Each node carries
`path`, `title`, `status`, `kind`, `brief`, `owner`, `blocked_by`,
`subtasks`, `tree_path` and `children`, which are sorted by title. A project
under a task is one of its children, so the task's tree holds the project's
tasks too. `brief` is a path in the board. A path that holds no task is
refused with `tasks::task::not_found`.

`board tasks tree --root true` returns `roots`, every top-level task of
the repos in scope with its tree: at most 100, open ones first and then by
title, with `total` counting them all. The scope is chosen as `available`
chooses it, with `--repo` and `--remote`. Giving both `--path` and `--root
true`, or neither, is refused with `tasks::tree::path_or_root`. Both forms
read the repos at their pins without syncing, and list `repos`, `cycles` and
`cross_repo` for the repos read. A tree costs one query, and one refs when a
task has an `after` or a `parent`.

## Writing tasks

`board tasks add --title <title>` writes a new open task to
`tasks/<slug>.md`, its slug derived from the title as notes derives a note's,
with `-2` and on added when the name is taken. It takes `--text` for the body,
and `--due`, `--defer`, `--repeat`, `--ephemeral` and `--from_hour`; `after`
and `contexts`, which are lists, come as JSON on stdin. `--repo` names the
repo. Otherwise it is the one repo whose record covers the remote, from
`--remote` or the caller's `origin`, else the personal repo, else `/`.
`--parent <path>`, `--kind project` and `--brief <path>` write those fields.
`--parent` and `--brief` take paths in the board, as `tree` lists them, and
are written as paths in the task's own repo. A path in another repo is refused
with `tasks::add::cross_repo`, and a parent that holds no task with
`tasks::add::parent_not_found`, so `add` never writes a parent that reaches
nothing. Two
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
`tasks::task::finished`; `reviewed` marks any task. `claim` with `--session`,
`--program`, `--person` or `--agent` sets `owner` to the actor they name,
refusing `tasks::claim::no_owner` when they name nobody. `--owner`, the
one flag a released Board took, names the owner in a single text, read as
the shapes migration read old owners: a `local_` id in it is the session,
Codex or Claude the program, the owner's own name the person, and the
whole text the agent's name unless it is only one of those, so `lead-1` is the agent
`lead-1`. `--owner` beside any of
the other four is refused with `tasks::owner::twice`. It claims an open
task that nobody or the same owner holds, where two owners that both name a
session are the same when their sessions match, and refuses one another owner holds with `tasks::task::held`, whose `owner` field names
the holder. `done`, `defer`, `drop` and `reviewed` leave `owner` as it is, so
a finished task still says who held it. No call clears `owner`; edit the task
to hand it on.

`move --path <task> --parent <task>` sets the task's `parent`, written as its
path in the task's own repo, and `--parent none` removes the line. Both take
paths in the board, as `tree` lists them, and act on a task in any status.
`move` changes nothing else, `reviewed` included. It reads the tree after the
sync and refuses a parent that is the task or under it with
`tasks::move::cycle`, one in another repo with `tasks::move::cross_repo`, and
a path that holds no task with `tasks::move::parent_not_found`.

Each call but `move` sets `reviewed` to today. Today is the person's day, which starts at
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
pushed by a later call. A sync that finds the same task changed here and on the remote merges
them, keeps the remote's version and records a `board/conflict/v1` under
`/conflicts/`. The call writes with `board put` and the blob it read, reads again and retries when the
task or the clone moved, and refuses with `tasks::task::busy` when the task
holds an edit not committed in its clone or kept changing. A new read that
finds the text the refused write sent counts as that write. It then pushes and
moves the mount's pin to the pushed commit, and `unpushed` holds the code of a
push that failed; the write stays committed in the clone.
`unsynced` and `unpushed` are both `tasks::repo::not_synced` for the board's
own tree, which is never synced. `commit` is null when the board's own tree
does not commit puts.

`done --push false` makes the sync before the write pull without pushing, and
skips the push after it, so the task is closed in a commit in the clone alone
and the mount's pin moves to that commit. An earlier commit the clone held
stays unpushed too. `unpushed` is then `tasks::repo::not_pushed`, and the
next write call that pushes, such as `reviewed` or a `done` without the flag,
pushes the commits. `tasks.available` syncs without pushing, so it does not
push them.

## The sweep skill

The directory also holds `SKILL.md`, the instructions an agent follows to
sweep the person's tasks one at a time, in Claude Code or Codex. From a phone
it works through a Claude Code session on the machine that holds the board,
such as one under Remote Control; a cloud session has no board. Link the
directory into the agents' shared skills to install it:

    ln -s "$PWD" ~/.agents/skills/tasks

Claude Code reads `~/.claude/skills`; link it there too unless that directory
is itself a link to `~/.agents/skills`.

## Tests

`crates/board-cli/src/tests/built_in/tasks.rs` and its modules run the
`board` binary in process against boards that mount notes repos with bare
remotes. They are the former pytest suite: the sweep's sections, scope and
routing, syncs, pins and their failures, blockers, trees, every write and
its refusals, the day across UTC offsets and `day_start`, and the schema.
The crate's own tests write through a real mount that a second Board writes
to between the read and the put, and one whose HEAD moves after Board left
the file uncommitted, and count the queries and refs a read makes.
