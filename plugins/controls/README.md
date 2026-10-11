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
  matches `/`.
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
  nothing. An agent can ask before it edits a
  file. Patterns match the file the path resolves to, through any link. A
  path with nothing there yet is matched where a file there would be, as
  `board refs --missing` reports it, so a control on a link's target applies
  to a new file under the link. Ask about a new record by its file name, such
  as `/tasks/new.json`. A path beneath a file, where nothing can be, matches
  no pattern, and neither does a new file under a mount, since patterns match
  only the board's own working tree.
- `board call controls.check` takes `{}` and measures every budget control.
  It returns each control's usage and its files. It fails, so `board call`
  exits 1, with `controls::budget::exceeded` when a control is over its
  budget, and with `controls::control::degraded` when Board marks a control
  degraded, for example because a file it references is missing. Each error
  names every control it applies to. It fails with
  `controls::file::unreadable` when a control reaches something Board cannot
  read as a file, such as a directory or a shadowed file. CI can run it as a gate. A file reached
  by both a reference and a pattern counts once, and a link's own file counts
  nothing. `check` resolves the references of every budget control that has
  `applies_to` with one `board refs` for each 10,000 controls. It reads every
  file the controls reach with `board get`, each file once however many
  controls reach it, in as few runs as Board's limits allow. It finds the
  files each control's patterns match with `board query` pages of up to
  10,000 pairs of control and file, so a board may match more than that.
  Each page is a separate run; a file added or removed between pages may or
  may not be counted, but no pair is skipped or counted twice.

Both fail with `controls::control::unreadable` when a control is valid in the
board but holds fields this plugin does not read, because the board's copy of
the schema was changed. When Board refuses a request the plugin makes, such as
a malformed path, the plugin fails with `controls::board::refused`, carrying
Board's code and hint.

Both capabilities read the board through `board query`, `board refs` and
`board get`, and only consider controls in the board's own working tree,
not in mounted boards. The plugin extracts nothing and writes nothing to the
database Board gives it.

## Installing

    uv sync --frozen --no-dev
    ln -s "$PWD" "${XDG_DATA_HOME:-$HOME/.local/share}/board/plugins/controls"
    board plugin enable controls

## Developing

The plugin talks to Board through the shared library in
`python/board-plugin/`, which `pyproject.toml` installs from this checkout, so
install the plugin by linking its directory in this repository.

The gates, which CI runs: `uv run ruff format --check .`, `uv run ruff check .`,
`uv run pyright` and `uv run pytest`. The tests drive `$BOARD_BIN`, by default
the workspace's `target/debug/board`, so build it first with
`cargo build -p board-cli`. `plugin.json` and the shipped schema are generated
from the pydantic models in `src/board_controls/models.py` by
`board_controls.manifest`, and a test checks that the committed files match.
