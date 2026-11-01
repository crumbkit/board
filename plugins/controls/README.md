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
- `basis` holds references to the records the control rests on, such as
  the `decisions/decision/v1` it comes from (plugins/decisions/README.md).
  Board checks them as it checks `applies_to`. A control whose `basis` names
  a decision copies the decision's rule into `requirement`. Changing the rule
  means a new decision that supersedes the old one and a new control citing
  it; until the old control is deleted, `check` marks it `basis_superseded`.
  Only a personal-board control can name a personal-board decision, since an
  absolute reference in a mounted board means that board's own path.
- `summary` is one line of at most 120 characters that names the code the
  control covers and what it asks, for a listing that must stay short.
- `repos` lists repo records (docs/architecture/paths/repos.md), such as
  `/repos/app`, as references Board checks. With `repos`, each pattern in
  `paths` is matched against a file's path from the root of each listed
  repository, so `src/secrets/*` covers `/repos/app/src/secrets/key.rs`, and
  the control reaches nothing outside them. A repository the board does not
  mount still matches, through the files a caller names to `check`. A
  repository is matched by the path written in `repos`, so name it by its
  own path, not through a link.
- `adds_step` names a workflow step that the work on the control's files
  gains. Nothing adds the step yet; `check` stores and shows it.
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
  With a budget, it limits the `words` (separated by any whitespace) or
  `bytes` of the files the control applies to. Its optional `per` says how:
  `total`, the default, limits the sum over every file, and `file` limits
  each file on its own, such as
  `{"budget": "words", "max": 500, "per": "file"}` on `changes/*/plan.md`.

A control ends when it is deleted, and git history keeps it.

The plugin ships the record's schema. `board plugin enable controls` writes it
to `.board/types/controls/control/v1.schema.json`.

## Capabilities

- `board call controls.for-path --path /AGENTS.md` returns
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
- `board call controls.check --path /repos/crumbkit` checks the controls
  under that path, following any link or mount on the way. Without `--path`
  it checks the repository Board was started in, as Board finds a
  repository from a directory (docs/architecture/paths/repos.md): `/` in the
  board's own repository or one of its linked worktrees, else the first
  mount, by path, whose clone shares the directory's git directory, else the
  repo record whose `remote` is the stored form of the directory's origin. A
  repo record with a checkout is matched by its clone only, since Board
  gives plugins no pointer's body. When none matches, it checks nothing and
  says why in `next`, with `path` null. From any checkout, then:

      board --personal call controls.check

  A path with nothing there fails with `controls::board::refused`. Under the
  root of a board it lists every control of that board, so one whose
  pattern matches nothing shows with `files: []` if measured and `files: 0`
  if stated; under another path, only those reaching a file there.
  `measured` holds the budget controls, with `used`, `max` and `files`:
  every file the budget counts, inside the path or not. A `total` control's
  `used` is the sum over its files. A `file` control also carries
  `per: "file"`, `largest`, the file with the most usage (`null` when it has
  no files), `used`, that file's usage, and `over`, each file over `max` with
  its usage, so the error names the files to shorten. `stated` holds the
  rest, with their `paths` patterns and `files`, the number of their files
  under the path, so the output stays short; `board get <control>` shows its
  `applies_to` and `paths`. Each control in either list also has `board`,
  the root of the board that holds it, `adds_step` (null when it adds none),
  `basis`, and `basis_superseded` when a decision in its board supersedes a
  decision its `basis` names. Each list holds at most `--limit` controls, 1
  to 10,000 and 100 by default, by the control's path, and `total` and
  `truncated` say for each how many applied and whether the limit cut it.
  The budgets are checked before the cut, so a control left out still
  fails the call. A stated control never fails the call. It
  fails, exiting 1, with `controls::budget::exceeded` when a measured
  control is over budget;
  `controls::control::degraded` when Board marks any control it reads
  degraded, under any path; and `controls::file::unreadable` when a measured
  control reaches something that is not a readable file, such as a
  directory. Each error names every control it applies to. A file counts
  once, however many references, patterns, links or mounts reach it, and a
  link's own file counts nothing. Reads are batched: one `board refs` per
  10,000 controls, `board get` once per file, and `board query` pages of
  10,000 control–file pairs, so a file changed between pages may or may not
  be counted.

  A path inside a mounted board, such as a repository mounted at
  `/repos/board` that is itself a board, also reads that board's own
  controls at the commit the mount pins, and says `own_controls: "read"`.
  They are read as data, with `board query`, `board refs` and `board get`,
  and nothing in the repository runs. Their references are read in their
  own board, and their `paths`, and their `repos`, are anchored at the
  mount's path, so they match beneath `/repos/board/`. A path that is a repo
  record without a checkout has no tree to read, so the output says
  `own_controls: "not_read"`, and `next` suggests mounting it.

  The index holds a repository's mounted commit, not the files a branch
  changes, and an unmounted repository has no files in the board. So the
  caller can name files: `files` lists paths relative to the path checked,
  such as `src/main.rs`. Then a control applies when it matches one of them
  by pattern or reaches it by reference, and a stated control's `files`
  counts the named files it matches; a budget still measures the files the
  board holds. An array is not a flag, so the input goes on stdin as JSON:

      git diff --name-only origin/main... | jq -R . | jq -sc '{path: "/repos/app", files: .}' \
        | board --personal call controls.check

Board refuses both calls with `board::plugin::schema_differs` when the board's
copy of the control schema differs from the one the plugin ships;
`board plugin enable controls` replaces the board's copy. When Board refuses a request the plugin makes, such as
a malformed path, the plugin fails with `controls::board::refused`, carrying
Board's code and hint.

Both capabilities read the board through `board query`, `board refs` and
`board get`. `for-path` considers only the controls in the board's own
working tree, though their patterns reach mounted files, and matches a
control with `repos` as `check` does. `check` also reads the own controls of
the mounted board its path is in.

## Installing

    uv sync --frozen --no-dev
    ln -s "$PWD" "${XDG_DATA_HOME:-$HOME/.local/share}/board/plugins/controls"
    board plugin enable controls

The plugin runs `.venv/bin/python` directly, so run `uv sync --frozen --no-dev`
again after pulling a change to `uv.lock`.

The directory also holds `SKILL.md`, the instructions an agent follows when it
works in a board that holds controls. Link the directory into the agent's
skills to install it:

    ln -s "$PWD" ~/.claude/skills/controls

### The Claude Code mod

`claude-code/` holds a Claude Code mod. It adds the repository's controls to
each session's context when the session starts, and names a control on a file
before an edit of that file runs. Load it with
`claude --plugin-dir "$PWD/claude-code"`, or name that directory in
`CLAUDE_CODE_PLUGIN_DIRS`. It runs `board` from `PATH`, `~/.local/bin`,
`/opt/homebrew/bin` or `/usr/local/bin`. It needs Claude Code 2.1.308 or
later, which has `$.session.append`.
[docs/plugins/controls-claude-code.md](../../docs/plugins/controls-claude-code.md)
explains the design.

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
`claude plugin test claude-code` runs the mod's tests.
