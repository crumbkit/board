# tasks

A Board plugin for `tasks/task/v1` records. It extracts each task's title, status
and due date into its database, and offers two capabilities: `due` lists the
unfinished tasks due before a date, and `next` lists every unfinished task in
the order to work on them, marking each one that is blocked.

Install it from this checkout:

    uv sync --frozen --no-dev
    ln -s "$PWD" "${XDG_DATA_HOME:-$HOME/.local/share}/board/plugins/tasks"

Then run `board plugin enable tasks` in a board. The manifest lists
`tasks/task/v1` in `types`, so this writes the schema the plugin ships at
`types/tasks/task/v1.schema.json` to `.board/types/tasks/task/v1.schema.json`,
and names the plugin in `.board/board.json`.

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

`board call tasks.due` takes `{"before": "YYYY-MM-DD"}` and returns the `open`
and `waiting` tasks due before that date, by due date then path.

`board call tasks.next` takes `{"limit": n}`, where `n` is 1 to 100 and
defaults to 100. It returns the `open` and `waiting` tasks by due date, with
undated tasks last, then by path. A task is `blocked` while any task in its
`after` is not `done`; `blocked_by` names each one once, by its display path,
or as written when it reaches nothing. A task waiting on a `dropped` task stays
blocked until its `after` changes. `total` counts every unfinished task, and
`truncated` says whether the limit left some out. `next` reads the `records`
view through `board query`, and resolves each `after` through `board refs`, so
a reference through a link or either of two mounts of one commit reaches the
task it names. It lists a task whose only fault is a dangling or wrong-typed
reference. Its one `board query` also returns where every `done` task is, as
one row, so it decides which waits are over itself. It runs one `board refs`
that names every listed task with an `after`, so one call starts at most two
`board` runs however many tasks wait.

Both capabilities read a task ignoring fields they do not know, so a board
whose own schema admits more fields still has its tasks listed.

`board query` can join `p_tasks.tasks` (`blob`, `title`, `status`, `due`) with
`entries` on `blob`.

## Development

The gates, which CI runs: `uv run ruff format --check .`, `uv run ruff check .`,
`uv run pyright` and `uv run pytest`. The tests drive `$BOARD_BIN`, by default
the workspace's `target/debug/board`, so build it first with
`cargo build -p board-cli`. `plugin.json` and the schema are generated from
the pydantic models in `src/board_tasks/models.py` by `board_tasks.manifest`,
and a test checks that the committed files match.

The protocol and the `board` client come from the shared library in
`python/board-plugin/`, which `pyproject.toml` installs from this checkout.
