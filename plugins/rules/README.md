# rules

A plugin built into Board for requirements on a board's files. Each requirement is a
`rules/rule/v1` record stored in the board, so it is versioned with the
files it governs. `rules.evaluate` applies a rule's checker to given text,
such as a pull request body, instead of files.

## The rule record

```json
{
  "$type": "rules/rule/v1",
  "requirement": "Keep the agent context under 1000 words.",
  "summary": "Keep the agent context under 1000 words.",
  "purpose": "Every agent loads it, so every word costs every session.",
  "applies_to": ["/AGENTS.md", "/CLAUDE.md"],
  "paths": ["docs/*.md"],
  "checker": {"check": "budget", "unit": "words", "max": 1000},
  "kind": "standing",
  "show": "failing",
  "sources": ["/rules/sources/context-cost"]
}
```

- `requirement` and `purpose` are required.
- `summary` is optional: the requirement in one line of at most 25 words,
  separated by single spaces. A change's brief shows it in place of the
  requirement. Without it, `for-paths` gives the requirement's first
  sentence, cut to 25 words.
- `applies_to` holds references to entries. Board checks them, so renaming or
  deleting a referenced file makes the rule `board::ref::dangling` in
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
- `checker` is optional. Without it, the rule is a written requirement,
  and `check` reports it as described. A checker's `check` names its kind,
  which decides its other fields. There are four kinds of checker:
  - A budget, `check` `budget`, limits the `unit` of the files the rule
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
  - A judged checker, such as `{"check": "judged", "rubric": "/rubrics/plain.md"}`,
    names a rubric that a clean-context model reads text against.
  - A version checker, such as
    `{"check": "version", "at_least": "1.99.1", "capture": "channel = \"([0-9.]+)\""}`, says
    that every version the first group of `capture` finds is at least
    `at_least`. `capture` uses the same regular expressions, with `^` and
    `$` matching at each line. Versions compare number by number, over only
    the numbers the found version names, so a pin of `26` passes
    `26.13.0` and `24.16.0` does not. A capture that is not numbers
    separated by dots, such as `stable`, is skipped. The checker holds the
    version as data: raise `at_least` when a newer release ships.

  A checker holds `check` and the fields of its kind alone, and is never null.
  A pattern checker's `file` and a judged checker's `rubric` are references, so
  `board check` marks the rule
  `board::ref::dangling` when one is missing, and `rules.check` then
  fails with `rules::rule::degraded`. `check` measures budgets over
  files and version rules over the versions files pin; it lists a pattern
  or judged rule as stated, and only `evaluate` applies those checkers.
  It also lists as stated, unmeasured, a rule whose `check`, budget
  `unit`, `per` or pattern `quote` holds a value this Board does not know,
  such as one an edited copy of the schema admits, and `evaluate` gives
  such a rule the verdict `stated`.
- `kind` is `standing`, the default, for a rule that stays until it is
  changed, or `obligation` for work a landed change left owed. Once the work
  that discharges an obligation is committed, `board rm <path> -m
  '<message naming the work>'` deletes the obligation in a commit whose
  message names the work; merging the work does not clear it.
- `owed_by`, for an obligation, references what owes the work: a
  `github/pull/v1`, a `review/report/v1` or a `plans/decision/v1`. The
  record's other text may still say so in words. `check`, `for-path` and
  `for-paths` return it. On a standing rule, `check` warns
  `rules::rule::owed_by_standing` in its `warnings`.
- `show` says when to show the rule: `start` at session start, `edit`
  before the first edit of a file it covers, or `failing` only when `check`
  reports it failing. Left out, an obligation shows at `start`, a rule
  with a budget or a version checker when `failing`, and any other rule at
  `edit`. `check` reports failing only a rule with a budget or a version checker, so a rule without one that says
  `failing` is never shown.
- `sources` holds references to what justifies the rule, such as a
  source note or an eval. Board checks them as it checks `applies_to`, so a
  dangling source makes `check` fail with `rules::rule::degraded`.

A source note, `rules/source/v1`, records an outside source that rules
cite, so one note serves every rule that cites it:

```json
{
  "$type": "rules/source/v1",
  "url": "https://example.com/context-cost",
  "title": "What each word of agent context costs",
  "read_on": "2026-11-01",
  "why": "It measures the cost per session of each word every agent loads."
}
```

Every field is required. `url` is an absolute `http` or `https` URL with a
host, and `read_on` is a date.

The plugin ships both schemas. `board plugin enable rules` writes them to
`.board/types/rules/rule/v1.schema.json` and
`.board/types/rules/source/v1.schema.json`. It also writes the former
`controls/control` and `controls/source` schemas under
`.board/types/controls/`, which Board still reads until a later pull request
of the rename deletes them.

## Capabilities

- `board rules for-path --path /AGENTS.md` returns
  each rule that applies to that path, by reference or by pattern, with
  its requirement, purpose and checker as the record holds it, `check`
  included, which is null when it has none, its
  `kind`, and its effective `show`, with the default applied. An
  obligation also carries `close`, which says how to close it: "Once the
  work this obligation names is committed, delete it with `board rm
  <path> -m '<message naming the work>'`; merging the work does not clear
  it.", with the rule's path in place of `<path>`. A standing
  rule has no `close`. A
  rule with a dangling reference still applies to the paths it does
  reach; an invalid rule applies to nothing. An agent can ask before it edits a file. Patterns match the file
  the path resolves to, through any link or mount. A path with nothing there
  yet is matched where a file there would be, as
  `board refs --missing` reports it, so a rule on a link's target applies
  to a new file under the link. Ask about a new record by its file name, such
  as `/tasks/new.json`. A path beneath a file, where nothing can be, matches
  no pattern.
- `rules.for-paths` reads `{"paths": [...]}` on stdin: files and SQLite
  `GLOB` patterns by logical path, such as
  `/repos/inbox/crates/inbox-store/**`. It returns each rule that applies
  to any of them once, by the rule's path, with its `summary`, or else
  the requirement's first sentence cut to 25 words, `matched`, the paths
  that reached it in the order asked, and, for an obligation, `close`, as
  `for-path` gives it. A version rule also carries `versions_found`, the
  number of versions its capture finds, older or not, in the files that
  both an asked path and the rule reach, read as `check` reads them at
  the commit each mount pins. A file path counts the file there, and a
  pattern counts each file it matches that the rule's `paths` match or
  its `applies_to` names. The count is unknown and `versions_found` is left
  out when one of those files cannot be read, such as a file not written
  yet or a directory, or when a pattern matches no file yet, because a file
  written there could pin a version. A path with no `*`, `?` or `[` is
  matched as `for-path` matches it. A pattern reaches a
  rule whose `applies_to` names a path it matches, and a rule one of
  whose `paths` matches a file it matches, by the file's display path. It
  also reaches a rule one of whose `paths` could match some path the
  pattern matches, whether a file is there yet or not, so `/crates/new/**`
  reaches a rule on `crates/*.rs`, and `/crates/store/**` reaches one on
  `*/Dockerfile*` before the first Dockerfile exists. A set such as `[ab]`
  counts as any one character there, so a pattern can reach a rule that
  no path would put in both, never miss one that a path would.
  `changes.brief` calls it with a plan's paths.
- `board rules check --path /repos/crumbkit` checks the rules
  under that path, following any link or mount on the way. Without `--path`
  it checks `/`. A path with nothing there fails with
  `rules::board::refused`. Under `/` it lists every rule, so one whose
  pattern matches nothing shows with `files: []` if measured and `files: 0` if
  stated; under another path, only those reaching a file there. `measured`
  holds the budget rules, with `used`, `max` and `files`: every file the
  budget counts under the path. A rule reaching files under the path
  runs on those files alone, so a breach in another repository never
  decides this one's check; under `/` it runs on all of them. A `total`
  rule's `used` is the sum over its files. A `file` rule also carries `per: "file"`,
  `largest`, the file with the most usage (`null` when it has no files),
  `used`, that file's usage, and `over`, each file over `max` with its
  usage. `versions` holds the
  version rules, with `at_least`, `files`, and `behind`: each `file`,
  `line` and version `found` that is older than `at_least`. Each measured
  and version rule carries `unreadable`: each `file` it reaches that is
  not a readable file, such as a directory, a degraded file or a blob a
  partial clone lacks, with Board's `code`; it counts nothing toward
  `used`. `stated` holds the rest,
  with their `paths` patterns and `files`, the number of their files under
  the path, so the output stays short; `board get <rule>` shows its
  `applies_to`, `paths` and `sources`. Every item carries `kind` and the
  effective `show`, and every obligation, in `measured`, `versions` or
  `stated`, carries `close`, as `for-path` gives it. `passed` is false
  when a budget is over its max, a pin is behind or a file is unreadable.
  These are answers, so the call still exits 0, and a reader sees the
  whole report. It fails with
  `rules::capture::invalid` when a version checker's `capture` does not
  compile or has no group, and with
  `rules::rule::degraded` when Board marks any rule degraded, under
  any path. Each error names
  every rule it applies to. A file counts once, however many references,
  patterns, links or mounts reach it, and a link's own file counts nothing.
  The call reads each file once, and resolves every rule's `applies_to`
  in one pass.
  `read` lists each source the measured and stated files came from:
  `{"mount": "/", "commit": null}` for the board's own tree, and each mount
  by its display path with the commit read. A mount is read at its pin. On
  a branch, commit and run `board --checkout rules check --path .`
  from the checkout, and the mount is read at the checkout's HEAD.
  `checkout` appears when Board ran from a checkout of a mount without
  `--checkout`, the check read that mount or the path lies under it, and
  the checkout's HEAD is not the commit read or a tracked file there differs
  from HEAD. It holds the mount, the commit `read`, the checkout's `head`,
  the command in `run` that measures the checkout's HEAD, and a `note`
  that says so in prose. `uncommitted` is true when HEAD is the commit read
  and a tracked file differs from it; when HEAD differs, the working tree
  is not scanned and `uncommitted` is left out. The entry changes neither
  `passed` nor the exit status. Uncommitted edits are read by no check, so
  commit them before the run.

- `rules.evaluate` checks named parts of text, such as a pull request's
  title and body, against the checkers of rules. It reads its input on stdin:

  ```sh
  echo '{"rules": ["/rules/short-title"], "parts": {"pr.title": "..."}}' \
    | board rules evaluate
  ```

  `rules` lists the rules by their paths in the board's own tree, and
  `parts` maps each part's name to its text. Every rule applies to every
  part, and a rule named twice is checked once. The output is `passed`,
  false when any result failed, and `results`, one per rule and part,
  ordered by rule as given and then by part name. Each result holds
  `rule`, `part`, `checker` (the checker's `check`: `budget`, `pattern`,
  `judged` or `version`, or null),
  `status` and `findings`:
  - `status` is `passed` or `failed`; `stated` for a rule without a checker
    or whose `check`, budget `unit`, `per` or pattern `quote` holds a value
    this Board does not know;
    `skipped` for a judged checker when the part failed a budget or pattern
    checker. Budget and pattern checkers always run first, whatever the order of
    `rules`.
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
  `rules::rule::not_found` when a named rule is not a valid
  rule in the board's own tree, `rules::checker::unreadable` when a
  pattern file or rubric cannot be read, and `rules::pattern::invalid`
  when a line of a pattern file is not a valid expression. That error names
  the line by its number. With `quote` `match-only` it adds only the column
  of a syntax error, because the parser's own message can quote part of the
  expression; with `pattern` it quotes the line and the parser's message.
  The call reads every pattern file and rubric once, and compiles each
  pattern file once per rule.

  A judged checker starts the agent `BOARD_RULES_AGENT` names, a JSON array
  of strings, as `review.run` starts `BOARD_REVIEW_AGENT` (the review
  plugin's README explains the variable and how to run the agent without
  tools). The agent runs in a new empty directory, leads its own process
  group, and gets the rubric and the part's numbered lines on stdin. Its
  answer, or the `result` of `claude -p --output-format json`'s envelope,
  must end with a JSON object holding `findings`; otherwise the call fails
  with `rules::agent::bad_output`. The part's text is untrusted, and a
  verdict is no stronger than the agent's resistance to instructions in it.
  The variable is read only when a judged checker has a part to read, so free
  checkers need no agent; without it the call fails with
  `rules::agent::not_set`, and with a malformed one
  `rules::agent::invalid`. An agent that cannot start, exits non-zero or
  runs out of time fails the call with `rules::agent::not_started`,
  `failed` or `timeout`; the refusal names only the program, never its
  arguments. Put a credential in the variables the capability's `env`
  declares, never in the argv.

  When the argv names the model with `--model` and a full model id, each
  verdict is kept in `$XDG_CACHE_HOME/board-rules/judged/` (else
  `~/.cache/board-rules/judged/`), in a file named by the SHA-256 of the
  whole prompt, which holds the rubric and the text, and the argv. A retry
  then judges only text that changed. Pin the model like this:

  ```sh
  export BOARD_RULES_AGENT='["claude", "-p", "--model", "claude-sonnet-4-5-20250929", "--output-format", "json", "--tools", ""]'
  ```

  A model alias such as `sonnet`, or a model set in
  `~/.claude/settings.json`, is not part of the key, so a pinned argv names
  a full id. Without `--model`, the model can change without the argv
  showing it, so no verdict is kept and every part is judged on every call.
  The cache is trusted state with the same standing as the rules: any
  process running as the same person can write a verdict there. Deleting
  the directory costs only new judgements. The judged checkers share 29
  minutes from the start of the call, which the plugin enforces itself,
  since Board puts no deadline on a built-in call.

- `board rules from-standards --standards /repos/<standards> --map
  /repos/<adopter>/rust-standards.toml --into /rules/<adopter>/standards`
  reads the adopter's map, a TOML file whose `profiles` lists the profiles
  it adopts, and each `<standards>/profiles/<profile>.md`. Every row of a
  table whose header has `ID`, `Rule` and `Where it applies` columns
  becomes a rule at `<into>/<id in lower case>`. `requirement` is the
  rule, `summary` its first sentence cut to 25 words, `sources` the
  profile file, and `paths` the cell's inline-code patterns anchored at the
  map's directory. The profile is read as `CommonMark` with GitHub's
  tables, so a code span in double backticks holds a backtick, and `\|`
  inside a cell is `|`. A pattern is a glob relative to the adopter's root, in
  which `**/` may match no directory; each `**/` becomes both nothing and
  `*/`, so `**/*.rs` becomes `repos/inbox/*.rs` and `repos/inbox/*/*.rs`.
  GLOB's `*` also matches `/`, so a rule can match more files than the
  pattern meant, never fewer. The call writes nothing. Its `lines` are
  `board put --batch` lines: one write per rule, expecting the blob held
  there or `none`, and one removal per rule under `into` that no rule
  gives any more. `profiles` gives each profile's `name`, its `file` and
  how many `rules` it gave. Apply the lines with
  `board --json rules from-standards ... | jq -c '.data.output.lines[]' | board put --batch`.
  It fails with `rules::standards::map_invalid` when the map has no
  `profiles` list of names, `rules::standards::unreadable` when the map
  or a profile cannot be read, `rules::standards::no_pattern` when a
  rule's Where it applies cell holds no inline code,
  `rules::standards::empty_rule` when its Rule cell is empty,
  `rules::standards::bad_id` when its ID holds anything but ASCII
  letters, digits, `-` and `_`, and `rules::standards::duplicate_rule`
  when two rules give the same ID, ignoring case, in one profile or in
  two, or when the map lists a profile twice. Inline HTML
  in a cell, such as `<br>`, reads as a space, and an ID written as inline
  code is read without its backticks.

Board refuses every call with `board::plugin::schema_differs` when the board's
copy of either schema differs from the one the plugin ships;
`board plugin enable rules` replaces the board's copy. When Board refuses a request the plugin makes, such as
a malformed path, the plugin fails with `rules::board::refused`, carrying
Board's code and hint.

The capabilities run in Board's own process and read the board as
`board query`, `board refs` and `board get` do. They consider only the rules in the board's own working tree,
never those in mounted boards, though those rules' patterns reach mounted
files.

## Enabling

Board has the plugin built in, so nothing is installed:

    board plugin enable rules

An older install at `${XDG_DATA_HOME:-$HOME/.local/share}/board/plugins/rules`
makes `call` and `plugin enable` fail with `board::plugin::duplicate`.
Remove it; it is usually a symlink to a checkout, so remove the link, not the
checkout.

The directory also holds `SKILL.md`, the instructions an agent follows when it
works in a board that holds rules. Link the directory into the agent's
skills to install it:

    ln -s "$PWD" ~/.claude/skills/rules

### The Claude Code mod

`claude-code/` holds a Claude Code mod. It adds the repository's rules to
each session's context when the session starts, and names a rule on a file
before an edit of that file runs. Wherever it lists an obligation, it prints
the obligation's `close` line beneath it. Load it with
`claude --plugin-dir "$PWD/claude-code"`, or name that directory in
`CLAUDE_CODE_PLUGIN_DIRS`. It runs `board` from `PATH`, `~/.local/bin`,
`/opt/homebrew/bin` or `/usr/local/bin`. It needs Claude Code 2.1.308 or
later, which has `$.session.append`.
[docs/plugins/rules-claude-code.md](../../docs/plugins/rules-claude-code.md)
explains the design.

## Developing

The plugin is the crate `crates/board-plugin-rules`. It generates the
manifest's input and output schemas from Rust structs with schemars, and
generates the record schemas from `RuleRecord` and `SourceRecord` in
`src/records.rs`. Its `types/` directory holds them as committed goldens,
which a test compares with what the structs generate; `SNAPSHOTS=overwrite`
rewrites them. The same test keeps this repository's own copies under
`.board/types/rules/` equal to them. A rule's properties carry titles,
as the former Python plugin generated them.
`checker` is one object listing every kind's fields, with an `allOf` of the kinds, each required when `check` names it,
because Board checks no reference under `anyOf` or `oneOf`.

This directory keeps `SKILL.md` and the mod. The tests of the plugin through
the binary are in `crates/board-cli/src/tests/built_in/rules.rs` and the
files beside it in `rules/`, which run the same commands in process.
The judged tests start a stub agent script, never a model.
`claude plugin test claude-code` runs the mod's tests.

`rules.migrate --owed_by true [--apply true]` sets `owed_by` on each
obligation that lacks it. Three rules are tried in order, and the first that
gives exactly one record decides. The first rule reads the pull request the
requirement names as `owner/repo#n`, or as `#n` after a word such as `PR` or
`pull`, with any numbers joined to it by `and` or commas, and a mount in
`applies_to` whose clone's origin gives the owner and repository. A number the board holds as an issue record is no match. The second takes the one
`review/report/v1` among `sources`, and the third takes the one
`plans/decision/v1` among them. A pull request the board holds no record of is
listed under `commands` with the `github.ref` command that writes it, and
nothing is set. Two records under one rule are listed under `ambiguous` with
`rules::migrate::ambiguous`. An obligation no rule matches is listed under
`unmatched`. A record that cannot be read is listed under `unread` with
`rules::migrate::unread`. Without `--apply` it writes nothing, and without
`--owed_by` it neither reads nor writes. With both, every rewrite lands in one
commit whose trailer is `Board-Bulk: migrate controls`. Each nested board runs
it itself.

`rules.migrate --rename true` renames what a board calls a control to a rule,
history included. It moves each `controls/control/v1` and
`controls/source/v1` from `/controls/` to `/rules/` as a `rules/rule/v1` or
`rules/source/v1`, and a rule's `rule` becomes its `checker`. A boundary's
`checks[].control`, a crossing verdict's `control` and `rule`, a plan's
`owed[].control` and a flow check's `controls` take their new names, and a
flow's `controlled-paths` and `planned-controlled` checks become `rule-paths`
and `planned-rule-paths`. In every record's strings and every Markdown file,
each `/controls/` path, in this board or under a mount, becomes `/rules/`;
`controls.<capability>` becomes `rules.<capability>`; and the old type names
become the new ones. crumbkit/board's own `controls/rules/<name>` becomes
`rules/<name>`. A flow version stored at the hash of its JSON is stored
again at the hash of its new JSON and the old version is kept, so a change
in a mounted board that still names it keeps reading. Every record in this
board that named the old version names the new one, so each change is pinned
to it.

Attention orders, effects and each order's context are never rewritten,
since they carry a signature; the records they name are rewritten as any
other. The orders and effects that hold an old name are listed under
`signed`. Each of these is listed under `kept`, with why, and left for a
person to change: a mount pointer under `/controls/`; a board config that
names `controls`, since a plugin may not write the config; a file under
`/controls/` that is not UTF-8 text, or whose JSON does not parse as a
record; a flow that would change but is not stored at the hash of its JSON;
and the records under `/changes/control-becomes-rule/` and
`/changes/rule-rename-rest/`, which describe the rename in the old names. Two flow versions that rename to the same JSON are
stored once, and both are re-pointed to it. A reference into a mounted board
that has not migrated yet resolves through the moved prefix until it does.

The dry run lists each write under `writes`, with `to` for a move, and gives
a `digest`. `--apply true --digest <digest>` writes them in one commit whose
trailer is `Board-Bulk: rename controls to rules`, or refuses with
`rules::migrate::stale` and writes nothing when its writes differ from the dry
run's, as when a record changed since. A file Board cannot read refuses the
whole rename with `rules::migrate::unread`, before any write. A second run
lists no writes. It reads and writes only the board's own tree, so each board
runs it itself.
