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
  "summary": "Keep the agent context under 1000 words.",
  "purpose": "Every agent loads it, so every word costs every session.",
  "applies_to": ["/AGENTS.md", "/CLAUDE.md"],
  "paths": ["docs/*.md"],
  "rule": {"check": "budget", "unit": "words", "max": 1000},
  "kind": "standing",
  "show": "failing",
  "sources": ["/controls/sources/context-cost"]
}
```

- `requirement` and `purpose` are required.
- `summary` is optional: the requirement in one line of at most 25 words,
  separated by single spaces. A change's brief shows it in place of the
  requirement. Without it, `for-paths` gives the requirement's first
  sentence, cut to 25 words.
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
  board's own tree it is matched by its path, so `check` under `/` lists it
  as unreadable; in a mounted board it matches nothing, and
  that board's own `board check` reports it.
- `rule` is optional. Without it, the control is a written requirement.
  A rule's `check` names its kind, which decides its other fields. There
  are four kinds of rule:
  - A budget, `check` `budget`, limits the `unit` of the files the control
    applies to: `words` (separated by any whitespace) or `bytes`, at most
    `max`. Its optional `per` says how: `total`, the default, limits the
    sum over every file, and `file` limits each file on its own, such as
    `{"check": "budget", "unit": "words", "max": 500, "per": "file"}` on
    `changes/*/plan.md`.
  - A pattern, such as
    `{"check": "pattern", "file": "/private/names.txt", "quote": "match-only"}`,
    names a file of regular expressions, one per line, that text must not match. The
    expressions use the syntax of Rust's `regex` crate, in which `^` and `$`
    match at each line. That syntax has no look-around and no
    back-references, so a line such as `(?=acme)` or `(a)\1` is invalid.
    Blank lines are skipped, and a match of no characters, such as `\b`
    finds, is never a finding. With `quote` `match-only`, a finding quotes
    what matched and never the expression, so the file can stay private;
    with `pattern`, it names the expression too.
  - A judged rule, such as `{"check": "judged", "rubric": "/rubrics/plain.md"}`,
    names a rubric that a clean-context model reads text against.
  - A version rule, such as
    `{"check": "version", "at_least": "1.99.1", "capture": "channel = \"([0-9.]+)\""}`, says
    that every version the first group of `capture` finds is at least
    `at_least`. `capture` uses the same regular expressions, with `^` and
    `$` matching at each line. Versions compare number by number, over only
    the numbers the found version names, so a pin of `26` passes
    `26.13.0` and `24.16.0` does not. A capture that is not numbers
    separated by dots, such as `stable`, is skipped. The rule holds the
    version as data: raise `at_least` when a newer release ships.

  A rule holds `check` and the fields of its kind alone, and is never null.
  A pattern rule's `file` and a judged rule's `rubric` are references, so
  `board check` marks the control
  `board::ref::dangling` when one is missing, and `controls.check` then
  fails with `controls::control::degraded`. `check` measures budgets over
  files and version rules over the versions files pin; it lists a pattern
  or judged control as stated, and only `evaluate` applies those rules.
  It also lists as stated, unmeasured, a control whose `check`, budget
  `unit`, `per` or pattern `quote` holds a value this Board does not know,
  such as one an edited copy of the schema admits, and `evaluate` gives
  such a control the verdict `stated`.
- `kind` is `standing`, the default, for a rule that stays until it is
  changed, or `obligation` for work a landed change left owed. Once the work
  that discharges an obligation is committed, `board rm` deletes the
  obligation and commits the deletion; merging the work does not clear it.
- `show` says when to show the control: `start` at session start, `edit`
  before the first edit of a file it covers, or `failing` only when `check`
  reports it failing. Left out, an obligation shows at `start`, a control
  with a budget or a version rule when `failing`, and any other control at
  `edit`. `check` reports failing only a control with a budget or a version rule, so a control without one that says
  `failing` is never shown.
- `sources` holds references to what justifies the control, such as a
  source note or an eval. Board checks them as it checks `applies_to`, so a
  dangling source makes `check` fail with `controls::control::degraded`.

A source note, `controls/source/v1`, records an outside source that controls
cite, so one note serves every control that cites it:

```json
{
  "$type": "controls/source/v1",
  "url": "https://example.com/context-cost",
  "title": "What each word of agent context costs",
  "read_on": "2026-11-01",
  "why": "It measures the cost per session of each word every agent loads."
}
```

Every field is required. `url` is an absolute `http` or `https` URL with a
host, and `read_on` is a date.

The plugin ships both schemas. `board plugin enable controls` writes them to
`.board/types/controls/control/v1.schema.json` and
`.board/types/controls/source/v1.schema.json`.

## Capabilities

- `board call controls.for-path --path /AGENTS.md` returns
  each control that applies to that path, by reference or by pattern, with
  its requirement, purpose and rule as the record holds it, `check`
  included, which is null when it has none, its
  `kind`, and its effective `show`, with the default applied. An
  obligation also carries `close`, which says how to close it: "Once the
  work this obligation names is committed, delete it with `board rm
  <path>`, which commits the deletion; merging the work does not clear
  it.", with the control's path in place of `<path>`. A standing
  control has no `close`. A
  control with a dangling reference still applies to the paths it does
  reach; an invalid control applies to nothing. An agent can ask before it edits a file. Patterns match the file
  the path resolves to, through any link or mount. A path with nothing there
  yet is matched where a file there would be, as
  `board refs --missing` reports it, so a control on a link's target applies
  to a new file under the link. Ask about a new record by its file name, such
  as `/tasks/new.json`. A path beneath a file, where nothing can be, matches
  no pattern.
- `controls.for-paths` reads `{"paths": [...]}` on stdin: files and SQLite
  `GLOB` patterns by logical path, such as
  `/repos/inbox/crates/inbox-store/**`. It returns each control that applies
  to any of them once, by the control's path, with its `summary`, or else
  the requirement's first sentence cut to 25 words, `matched`, the paths
  that reached it in the order asked, and, for an obligation, `close`, as
  `for-path` gives it. A path with no `*`, `?` or `[` is
  matched as `for-path` matches it. A pattern reaches a
  control whose `applies_to` names a path it matches, and a control one of
  whose `paths` matches a file it matches, by the file's display path. It
  also reaches a control one of whose `paths` could match some path the
  pattern matches, whether a file is there yet or not, so `/crates/new/**`
  reaches a control on `crates/*.rs`, and `/crates/store/**` reaches one on
  `*/Dockerfile*` before the first Dockerfile exists. A set such as `[ab]`
  counts as any one character there, so a pattern can reach a control that
  no path would put in both, never miss one that a path would.
  `changes.brief` calls it with a plan's paths.
- `board call controls.check --path /repos/crumbkit` checks the controls
  under that path, following any link or mount on the way. Without `--path`
  it checks `/`. A path with nothing there fails with
  `controls::board::refused`. Under `/` it lists every control, so one whose
  pattern matches nothing shows with `files: []` if measured and `files: 0` if
  stated; under another path, only those reaching a file there. `measured`
  holds the budget controls, with `used`, `max` and `files`: every file the
  budget counts under the path. A control reaching files under the path
  runs on those files alone, so a breach in another repository never
  decides this one's check; under `/` it runs on all of them. A `total`
  control's `used` is the sum over its files. A `file` control also carries `per: "file"`,
  `largest`, the file with the most usage (`null` when it has no files),
  `used`, that file's usage, and `over`, each file over `max` with its
  usage. `versions` holds the
  version controls, with `at_least`, `files`, and `behind`: each `file`,
  `line` and version `found` that is older than `at_least`. Each measured
  and version control carries `unreadable`: each `file` it reaches that is
  not a readable file, such as a directory, a degraded file or a blob a
  partial clone lacks, with Board's `code`; it counts nothing toward
  `used`. `stated` holds the rest,
  with their `paths` patterns and `files`, the number of their files under
  the path, so the output stays short; `board get <control>` shows its
  `applies_to`, `paths` and `sources`. Every item carries `kind` and the
  effective `show`, and every obligation, in `measured`, `versions` or
  `stated`, carries `close`, as `for-path` gives it. `passed` is false
  when a budget is over its max, a pin is behind or a file is unreadable.
  These are answers, so the call still exits 0, and a reader sees the
  whole report. It fails with
  `controls::capture::invalid` when a version rule's `capture` does not
  compile or has no group, and with
  `controls::control::degraded` when Board marks any control degraded, under
  any path. Each error names
  every control it applies to. A file counts once, however many references,
  patterns, links or mounts reach it, and a link's own file counts nothing.
  The call reads each file once, and resolves every control's `applies_to`
  in one pass.
  `read` lists each source the measured and stated files came from:
  `{"mount": "/", "commit": null}` for the board's own tree, and each mount
  by its display path with the commit read. A mount is read at its pin. On
  a branch, commit and run `board --checkout call controls.check --path .`
  from the checkout, and the mount is read at the checkout's HEAD.
  `checkout` appears when Board ran from a checkout of a mount without
  `--checkout`, the check read that mount or the path lies under it, and
  the checkout's HEAD is not the commit read or a tracked file there differs
  from HEAD. It holds the mount, the commit `read`, the checkout's `head`,
  `uncommitted`, the command in `run` that measures the checkout's HEAD,
  and a `note` that says so in prose. It changes neither `passed` nor the
  exit status. Uncommitted edits are read by no check, so commit them
  before the run.

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
  `control`, `part`, `rule` (the rule's `check`: `budget`, `pattern`,
  `judged` or `version`, or null),
  `status` and `findings`:
  - `status` is `passed` or `failed`; `stated` for a control without a rule
    or whose `check`, budget `unit`, `per` or pattern `quote` holds a value
    this Board does not know;
    `skipped` for a judged rule when the part failed a budget or pattern
    rule. Budget and pattern rules always run first, whatever the order of
    `controls`.
  - A finding holds `text`, the words at fault, `line`, `pattern` and
    `rewrite`. A pattern finding quotes the match and gives its line, and
    names the expression in `pattern` only with `quote` `pattern`. A judged
    finding gives `rewrite`, the model's instruction for fixing the
    sentence, and its line when the model named one. A failed budget has one
    finding with a null `line`. Every key is present, and null means none.
  - A result holds `used` and `max`, which are null except in a budget
    result. A result shows at most 20 findings and counts the rest in
    `omitted`, which is null when none is left out.

  A failed part is an answer, so the call still exits 0. It fails with
  `controls::control::not_found` when a named control is not a valid
  control in the board's own tree, `controls::rule::unreadable` when a
  pattern file or rubric cannot be read, and `controls::pattern::invalid`
  when a line of a pattern file is not a valid expression. That error names
  the line by its number. With `quote` `match-only` it adds only the column
  of a syntax error, because the parser's own message can quote part of the
  expression; with `pattern` it quotes the line and the parser's message.
  The call reads every pattern file and rubric once, and compiles each
  pattern file once per control.

  A judged rule starts the agent `BOARD_CONTROLS_AGENT` names, a JSON array
  of strings, as `review.run` starts `BOARD_REVIEW_AGENT` (the review
  plugin's README explains the variable and how to run the agent without
  tools). The agent runs in a new empty directory, leads its own process
  group, and gets the rubric and the part's numbered lines on stdin. Its
  answer, or the `result` of `claude -p --output-format json`'s envelope,
  must end with a JSON object holding `findings`; otherwise the call fails
  with `controls::agent::bad_output`. The part's text is untrusted, and a
  verdict is no stronger than the agent's resistance to instructions in it.
  The variable is read only when a judged rule has a part to read, so free
  rules need no agent; without it the call fails with
  `controls::agent::not_set`, and with a malformed one
  `controls::agent::invalid`. An agent that cannot start, exits non-zero or
  runs out of time fails the call with `controls::agent::not_started`,
  `failed` or `timeout`; the refusal names only the program, never its
  arguments. Put a credential in the variables the capability's `env`
  declares, never in the argv.

  When the argv names the model with `--model` and a full model id, each
  verdict is kept in `$XDG_CACHE_HOME/board-controls/judged/` (else
  `~/.cache/board-controls/judged/`), in a file named by the SHA-256 of the
  whole prompt, which holds the rubric and the text, and the argv. A retry
  then judges only text that changed. Pin the model like this:

  ```sh
  export BOARD_CONTROLS_AGENT='["claude", "-p", "--model", "claude-sonnet-4-5-20250929", "--output-format", "json", "--tools", ""]'
  ```

  A model alias such as `sonnet`, or a model set in
  `~/.claude/settings.json`, is not part of the key, so a pinned argv names
  a full id. Without `--model`, the model can change without the argv
  showing it, so no verdict is kept and every part is judged on every call.
  The cache is trusted state with the same standing as the controls: any
  process running as the same person can write a verdict there. Deleting
  the directory costs only new judgements. The judged rules share 29
  minutes from the start of the call, which the plugin enforces itself,
  since Board puts no deadline on a built-in call.

- `board call controls.from-standards --standards /repos/<standards> --map
  /repos/<adopter>/rust-standards.toml --into /controls/<adopter>/standards`
  reads the adopter's map, a TOML file whose `profiles` lists the profiles
  it adopts, and each `<standards>/profiles/<profile>.md`. Every row of a
  table whose header has `ID`, `Rule` and `Where it applies` columns
  becomes a control at `<into>/<id in lower case>`. `requirement` is the
  rule, `summary` its first sentence cut to 25 words, `sources` the
  profile file, and `paths` the cell's inline-code patterns anchored at the
  map's directory. The profile is read as `CommonMark` with GitHub's
  tables, so a code span in double backticks holds a backtick, and `\|`
  inside a cell is `|`. A pattern is a glob relative to the adopter's root, in
  which `**/` may match no directory; each `**/` becomes both nothing and
  `*/`, so `**/*.rs` becomes `repos/inbox/*.rs` and `repos/inbox/*/*.rs`.
  GLOB's `*` also matches `/`, so a control can match more files than the
  pattern meant, never fewer. The call writes nothing. Its `lines` are
  `board put --batch` lines: one write per rule, expecting the blob held
  there or `none`, and one removal per control under `into` that no rule
  gives any more. `profiles` gives each profile's `name`, its `file` and
  how many `rules` it gave. Apply the lines with
  `board --json call controls.from-standards ... | jq -c '.data.output.lines[]' | board put --batch`.
  It fails with `controls::standards::map_invalid` when the map has no
  `profiles` list of names, `controls::standards::unreadable` when the map
  or a profile cannot be read, `controls::standards::no_pattern` when a
  rule's Where it applies cell holds no inline code,
  `controls::standards::empty_rule` when its Rule cell is empty,
  `controls::standards::bad_id` when its ID holds anything but ASCII
  letters, digits, `-` and `_`, and `controls::standards::duplicate_rule`
  when two rules give the same ID, ignoring case, in one profile or in
  two, or when the map lists a profile twice. Inline HTML
  in a cell, such as `<br>`, reads as a space, and an ID written as inline
  code is read without its backticks.

Board refuses every call with `board::plugin::schema_differs` when the board's
copy of either schema differs from the one the plugin ships;
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
generates the record schemas from `ControlRecord` and `SourceRecord` in
`src/records.rs`. Its `types/` directory holds them as committed goldens,
which a test compares with what the structs generate; `SNAPSHOTS=overwrite`
rewrites them. The same test keeps this repository's own copies under
`.board/types/controls/` equal to them. A control's properties carry titles,
as the former Python plugin generated them.
`rule` is one object listing every kind's fields, with a `oneOf` that picks
the kind, because Board checks no reference under `anyOf` or `oneOf`.

This directory keeps `SKILL.md` and the mod. The tests of the plugin through
the binary are in `crates/board-cli/src/tests/built_in/controls.rs` and the
files beside it in `controls/`, which run the same commands in process.
The judged tests start a stub agent script, never a model.
`claude plugin test claude-code` runs the mod's tests.
