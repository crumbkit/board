# tasks

A Board plugin for `lr/task/v1` records. It extracts each task's title, status
and due date into its database, and offers two capabilities: `due` lists the
unfinished tasks due before a date, and `next` lists every unfinished task in
the order to work on them, marking each one that is blocked.

Install it from this checkout:

    uv sync --frozen --no-dev
    ln -s "$PWD" "${XDG_DATA_HOME:-$HOME/.local/share}/board/plugins/tasks"

Then enable it in a board's `.board/board.json` with `"plugins": ["tasks"]`.

## The task record

The schema is at `types/lr/task/v1.schema.json`. A board needs a copy at
`.board/types/lr/task/v1.schema.json`. The manifest does not list it in
`types` yet, because Board refuses a plugin that ships a type outside its own
namespace, so `board plugin enable tasks` does not write it. Copy it by hand.

```json
{
  "$type": "lr/task/v1",
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
- `after` lists references to the `lr/task/v1` records this task waits on.
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
reference. It runs one `board refs` for each listed task with an `after`, and
a call has 30 seconds, so the limit is 100. Batching those lookups into one
`board refs` is the way to raise it.

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
