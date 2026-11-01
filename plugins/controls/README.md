# controls

A plugin built into Board for requirements on a board's files. Each requirement is a
`controls/control/v1` record stored in the board, so it is versioned with the
files it governs. `controls.evaluate` applies a control's rule to given text,
such as a pull request body, instead of files.

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
  There are three kinds of rule:
  - A budget limits the `words` (separated by any whitespace) or `bytes` of
    the files the control applies to. Its optional `per` says how: `total`,
    the default, limits the sum over every file, and `file` limits each file
    on its own, such as `{"budget": "words", "max": 500, "per": "file"}` on
    `changes/*/plan.md`.
  - A pattern, such as
    `{"pattern": "/private/names.txt", "show": "match-only"}`, names a file
    of regular expressions, one per line, that text must not match. The
    expressions use the syntax of Rust's `regex` crate, in which `^` and `$`
    match at each line. That syntax has no look-around and no
    back-references, so a line such as `(?=acme)` or `(a)\1` is invalid.
    Blank lines are skipped, and a match of no characters, such as `\b`
    finds, is never a finding. With `show` `match-only`, a finding quotes
    what matched and never the expression, so the file can stay private;
    with `pattern`, it names the expression too.
  - A judged rule, such as `{"judged": "/rubrics/plain.md"}`, names a
    rubric that a clean-context model reads text against. Board does not
    apply judged rules yet; see `controls.evaluate` below.

  A rule holds the fields of exactly one kind, and is never null. A pattern
  file and a rubric are references, so `board check` marks the control
  `board::ref::dangling` when one is missing, and `controls.check` then
  fails with `controls::control::degraded`. `check` measures budgets over
  files; it lists a pattern or judged control as stated, and only
  `evaluate` applies those rules.

The plugin ships the record's schema. `board plugin enable controls` writes it
to `.board/types/controls/control/v1.schema.json`.

## Capabilities

- `board call controls.for-path --path /AGENTS.md` returns
  each control that applies to that path, by reference or by pattern, with
  its requirement, purpose and rule, which is left out when it has none. A
  control with a dangling reference
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
  it checks `/`. A path with nothing there fails with
  `controls::board::refused`. Under `/` it lists every control, so one whose
  pattern matches nothing shows with `files: []` if measured and `files: 0` if
  stated; under another path, only those reaching a file there. `measured`
  holds the budget controls, with `used`, `max` and `files`: every file the
  budget counts, inside the path or not. A `total` control's `used` is the
  sum over its files. A `file` control also carries `per: "file"`,
  `largest`, the file with the most usage (`null` when it has no files),
  `used`, that file's usage, and `over`, each file over `max` with its
  usage, so the error names the files to shorten. `stated` holds the rest,
  with their `paths` patterns and `files`, the number of their files under
  the path, so the output stays short; `board get <control>` shows its
  `applies_to` and `paths`. A stated control never fails the call. It
  fails, exiting 1, with `controls::budget::exceeded` when a measured
  control is over budget;
  `controls::control::degraded` when Board marks any control degraded, under
  any path; and `controls::file::unreadable` when a measured control reaches
  something that is not a readable file, such as a directory. Each error names
  every control it applies to. A file counts once, however many references,
  patterns, links or mounts reach it, and a link's own file counts nothing.
  The call reads each file once, and resolves every control's `applies_to`
  in one pass.

- `controls.evaluate` checks named parts of text, such as a pull request's
  title and body, against controls' rules. It reads its input on stdin:

  ```sh
  echo '{"controls": ["/controls/short-title"], "parts": {"pr.title": "..."}}' \
    | board call controls.evaluate
  ```

  `controls` lists the controls by their paths in the board's own tree, and
  `parts` maps each part's name to its text. Every control applies to every
  part, and a control named twice is checked once. The output is `passed`,
  false when any result failed, and `results`, one per control and part,
  ordered by control as given and then by part name. Each result holds
  `control`, `part`, `rule` (`budget`, `pattern`, `judged`, or null),
  `status` and `findings`:
  - `status` is `passed` or `failed`; `stated` for a control without a rule;
    `skipped` for a judged rule when the part failed a budget or pattern
    rule. Budget and pattern rules always run first, whatever the order of
    `controls`.
  - A finding holds `text`, the words at fault, and `line` when it has one.
    A pattern finding quotes the match, and adds `pattern` only with
    `show` `pattern`. A failed budget has one finding without a line.
  - A budget result adds `used` and `max`. A result shows at most 20
    findings and counts the rest in `omitted`.

  A failed part is an answer, so the call still exits 0. It fails with
  `controls::control::not_found` when a named control is not a valid
  control in the board's own tree, `controls::rule::unreadable` when a
  pattern file or rubric cannot be read, and `controls::pattern::invalid`
  when a line of a pattern file is not a valid expression. That error names
  the line by its number. With `show` `match-only` it adds only the column
  of a syntax error, because the parser's own message can quote part of the
  expression; with `pattern` it quotes the line and the parser's message.
  A judged rule that has a part to read fails the call with
  `controls::judged::unavailable`: the agent that judges text lands in a
  follow-up change, so leave judged controls out of `controls` until then.
  The call reads every pattern file and rubric once, and compiles each
  pattern file once per control.

Board refuses every call with `board::plugin::schema_differs` when the board's
copy of the control schema differs from the one the plugin ships;
`board plugin enable controls` replaces the board's copy. When Board refuses a request the plugin makes, such as
a malformed path, the plugin fails with `controls::board::refused`, carrying
Board's code and hint.

The capabilities run in Board's own process and read the board as
`board query`, `board refs` and `board get` do. They consider only the controls in the board's own working tree,
never those in mounted boards, though those controls' patterns reach mounted
files.

## Enabling

Board has the plugin built in, so nothing is installed:

    board plugin enable controls

An older install at `${XDG_DATA_HOME:-$HOME/.local/share}/board/plugins/controls`
makes `call` and `plugin enable` fail with `board::plugin::duplicate`.
Remove it; it is usually a symlink to a checkout, so remove the link, not the
checkout.

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

The plugin is the crate `crates/board-plugin-controls`. It generates the
manifest's input and output schemas from Rust structs with schemars, and
generates the control schema from `ControlRecord` in `src/records.rs`. Its
`types/` directory holds that schema as a committed golden, which a test
compares with what the struct generates; `SNAPSHOTS=overwrite` rewrites it.
Its properties carry titles, as the former Python plugin generated them.
`rule` is one object listing every kind's fields, with a `oneOf` that picks
the kind, because Board checks no reference under `anyOf` or `oneOf`.

This directory keeps `SKILL.md`, the mod and a uv project that only tests
the plugin through the real binary. The gates, which CI runs:
`uv run ruff format --check .`, `uv run ruff check .`, `uv run pyright` and
`uv run pytest`. The tests drive `$BOARD_BIN`, by default the workspace's
`target/debug/board`, so build it first with `cargo build -p board-cli`.
`claude plugin test claude-code` runs the mod's tests.
