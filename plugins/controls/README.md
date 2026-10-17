# controls

A Board plugin for requirements on a board's files. Each requirement is a
`controls/control/v1` record stored in the board, so it is versioned with the
files it governs.

## The control record

```json
{
  "$type": "controls/control/v1",
  "requirement": "Keep the agent context under 1000 words.",
  "purpose": "Every agent loads it, so every word costs every session.",
  "applies_to": ["/AGENTS.md", "/CLAUDE.md"],
  "paths": ["docs/*.md"],
  "rule": {"budget": "words", "max": 1000}
}
```

- `requirement` and `purpose` are required.
- `applies_to` holds references to entries. Board checks them, so renaming or
  deleting a referenced file makes the control `board::ref::dangling` in
  `board check`.
- `paths` holds SQLite `GLOB` patterns, matched against each entry's path
  from the board's root, such as `docs/*.md`. In a `GLOB` pattern `*` also
  matches `/`. Patterns reach files in mounted boards, by their path through
  the mount: `*.md` covers a mount's Markdown files, and `docs/*.md` does not
  reach a board mounted at `vendor/lib`. Both capabilities match a file by
  the one display path Board's index gives it: through the shortest mount
  when a commit is mounted more than once, whichever path `for-path` was
  asked. A file with a malformed path has no such name. In the
  board's own tree it is matched by its path, so `check` fails with
  `controls::file::unreadable`; in a mounted board it matches nothing, and
  that board's own `board check` reports it.
- `rule` is optional. Without it, the control is a written requirement.
  With a budget, it limits the sum of the `words` (separated by any
  whitespace) or `bytes` of every file the control applies to.

The plugin ships the record's schema. `board plugin enable controls` writes it
to `.board/types/controls/control/v1.schema.json`.

## Capabilities

- `board call controls.for-path` takes `{"path": "/AGENTS.md"}` and returns
  each control that applies to that path, by reference or by pattern, with
  its requirement, purpose and rule. A control with a dangling reference
  still applies to the paths it does reach; an invalid control applies to
  nothing. An agent can ask before it edits a file. Patterns match the file
  the path resolves to, through any link or mount. A path with nothing there
  yet is matched where a file there would be, as
  `board refs --missing` reports it, so a control on a link's target applies
  to a new file under the link. Ask about a new record by its file name, such
  as `/tasks/new.json`. A path beneath a file, where nothing can be, matches
  no pattern.
- `board call controls.check` takes an optional path, such as
  `{"path": "/repos/crumbkit"}`, `/` when left out, and checks it where any
  link or mount leads. A path with nothing there fails with
  `controls::board::refused`. Under `/` it lists every control, so one whose
  pattern matches nothing shows with no files; under another path, only
  those reaching a file there. `measured` holds the budget controls, with
  usage and `files`: every file the budget counts, inside the path or not.
  `stated` holds the rest, with `files` under the path only, and never fails
  the call. It fails, exiting 1, with `controls::budget::exceeded` when a
  measured control is over budget; `controls::control::degraded` when Board
  marks any control degraded, under any path; and
  `controls::file::unreadable` when a measured control reaches something
  that is not a readable file, such as a directory. Each error names every
  control it applies to. A file counts once, however many references,
  patterns, links or mounts reach it, and a link's own file counts nothing.
  Reads are batched: one `board refs` per 10,000 controls, `board get` once
  per file, and `board query` pages of 10,000 control–file pairs, so a file
  changed between pages may or may not be counted.

Board refuses both calls with `board::plugin::schema_differs` when the board's
copy of the control schema differs from the one the plugin ships. When Board refuses a request the plugin makes, such as
a malformed path, the plugin fails with `controls::board::refused`, carrying
Board's code and hint.

Both capabilities read the board through `board query`, `board refs` and
`board get`. They consider only the controls in the board's own working tree,
never those in mounted boards, though those controls' patterns reach mounted
files. The plugin extracts nothing and writes nothing to the
database Board gives it.

## Installing

    uv sync --frozen --no-dev
    ln -s "$PWD" "${XDG_DATA_HOME:-$HOME/.local/share}/board/plugins/controls"
    board plugin enable controls

The plugin runs `.venv/bin/python` directly, so run `uv sync --frozen --no-dev`
again after pulling a change to `uv.lock`.

## Developing

The plugin talks to Board through the shared library in
`python/board-plugin/`, which `pyproject.toml` installs from this checkout, so
install the plugin by linking its directory in this repository.

The gates, which CI runs: `uv run ruff format --check .`, `uv run ruff check .`,
`uv run pyright` and `uv run pytest`. The tests drive `$BOARD_BIN`, by default
the workspace's `target/debug/board`, so build it first with
`cargo build -p board-cli`. `plugin.json` and the shipped schema are generated
from the TypedDicts in `src/board_controls/shapes.py` by
`board_controls.manifest`, and a test checks that the committed files match.
