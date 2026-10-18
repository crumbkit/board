# Architecture

This is the v0 design. Built so far: logical paths, resolution and walking,
links and mounts, records, schemas and references, the index, canonical form,
plugins with extraction, and the commands under Commands. PLAN.md says what
comes next.

Board is a fast, validated view of a git tree. Git does history, branching,
merging and transport. Board indexes what is in a tree, validates the records
it understands, resolves references through links and mounts, and makes
records easy to read and write.

## Boards and entries

A board is the working tree of a git repository whose root holds
`.board/board.json`. Board finds it by searching upward from the current
directory, as git does. Both names must be listed in that exact spelling, so
`.Board/board.json` holds no board even where the file system ignores case. No
board is `board::config::not_found`, and a `board.json` Board cannot read is
`board::config::invalid`.

When the search finds no board but the directory is in a git repository,
Board reads the known boards in `$XDG_CONFIG_HOME/board/config.json`
(`~/.config/board/config.json` by default), shaped
`{"boards": ["/Users/me/Code/board"]}`. A relative path is read against the
file's directory. A missing or empty file lists none, and any other file
that is not this shape is `board::config::invalid`. Board refreshes each
known board's index and reads the mounts in its working tree. A board
listed under several spellings counts once. A mount
matches when its repository has the same git common directory as the
directory's repository, so a linked worktree matches too. With exactly one
match, Board runs as that board, as git runs from a subdirectory. The prefix
is the mount's path joined with the directory's path beneath the
repository's root, such as `/repos/crumbkit/docs`; for now only the timing
log records it. A directory inside the git directory has no prefix and
matches nothing. No match is `board::config::not_found`. When the file lists
boards, the message says they were read and none mounts the repository, and
the hint says to add a mount; otherwise the hint says to list a board. More
than one match, counting every mount of every known board, is
`board::config::ambiguous`; `-C` then picks a board. A known board that
cannot be opened or refreshed is passed over, and a `not_found` message
names it. `-C` into a mounted repository finds its board the same way.

Every file git sees
under the root is an entry: tracked files, and untracked files that are not
ignored. `.git` is never an entry. An entry has a path, a blob hash, a size, a
kind and a status.

| Kind | What it is |
| --- | --- |
| record | a `.json` file whose top-level object has a `$type` string; `.board/board.json` is one |
| pointer | a record of type `board/mount/v1` (a link or a mount) |
| schema | a file under `.board/types/` ending `.schema.json` |
| json | any other `.json` file |
| file | anything else: Markdown, images, code |

An entry's status is `ok` or `degraded` with exactly one code (see Codes).
Board never refuses to index a file; the worst it says is a path and a code.
A file or directory that cannot be read is `board::repo::unreadable`. A
degraded directory passes its code to every entry beneath it. Working-tree
files are hashed as stored, with no clean filter, so a file under end-of-line
conversion or LFS shows as uncommitted. A symlink is a `file` entry and is
never followed; Board's links are pointers. A submodule is degraded with
`board::repo::submodule` and never descended.

`.board/board.json` is `board/config/v1`:

```json
{
  "$type": "board/config/v1",
  "commit_on_put": true,
  "plugins": ["tasks"]
}
```

## Records

`$type` is `<namespace>/<kind>/v<n>`. Namespace and kind are lowercase ASCII
letters, digits and hyphens; `n` is a positive integer. The `board` namespace is
reserved for Board's own documents. A record's schema is
`.board/types/<namespace>/<kind>/v<n>.schema.json` in the same board as the
record, so a mounted board's records are checked against its own schemas.
Schemas are JSON Schema 2020-12. After a release, a changed type is a new
version; until a release or a second user, it changes in place.

Parsing is strict, and the first failure is the entry's code: over 16 MiB
(`too_large`), not UTF-8, git conflict markers (`merge_conflict`), not JSON
(`parse`), a duplicate key at any depth, then a number that a double or a
64-bit integer cannot hold exactly (`lossy_number`). These rules follow RFC
7493 (I-JSON), except that an integer may use the full 64-bit range. A
malformed `$type` is `record::bad_type`; a type with no schema file is
`record::unknown_type`, and so is one whose schema file git ignores and does
not track, or whose schema path is spelled in another case on disk. In `check`
and `put` alike, a record whose schema file cannot be read is
`repo::unreadable`, though `get` still returns it. A file under
`.board/types/board/` is `schema::invalid`, because Board's own types are
checked by Rust types. A schema that does not compile is `schema::invalid`,
and so is each record of its type. `record::invalid` names the first failing
instance pointer in sorted order.

Board's own documents (`board/*`) refuse unknown fields. User records follow
their schema. Board knows `board/config/v1`, `board/mount/v1` and
`board/plugin-manifest/v1` wherever they are stored. A manifest is checked by its content alone; a
broken one is `plugin::bad_manifest`. Any other `board/*` type is
`record::unknown_type`.

### Typed references

A schema marks a string as a reference with `"format": "board-ref"`, and may
name the types it accepts with `"x-board-type"` (a string or a list of exact
`$type` values). A reference is an absolute logical path, read in the referring
record's board. Board finds references through `properties`,
`patternProperties`, `additionalProperties`, `items`, `prefixItems`, `allOf`
and local `$ref`; branches of `anyOf`, `oneOf` and `if` still validate but are
not searched. A string that is not a logical path is `ref::malformed`. `check`
reports the first failing reference in document order: `ref::dangling`, or,
when the schema names types, `ref::wrong_type` for anything but a record of one
of them. Without `x-board-type`, a reference may reach any entry, such as a
Markdown file.

A mounted board's schemas are untrusted. Board never fetches a remote `$ref`,
and runs `pattern` on a linear-time regex engine. A schema is `schema::invalid`
when one record location would expand to more than 256 subschemas through
`allOf`, `anyOf`, `oneOf`, `not`, `if`, `then`, `else`, `dependentSchemas`,
`$ref` and `$dynamicRef`, or when a `$ref` or `$dynamicRef` is not `#` or a
JSON pointer into the schema.

### Canonical form

Every file Board writes is canonical: keys sorted by code point, two-space
indentation, one key or array element per line, empty `{}` and `[]` on one
line, `\n` endings, one trailing newline and no byte-order mark. Strings are
escaped as RFC 8785 (JCS) escapes them: only `"`, `\` and control characters.
JCS sorts keys by UTF-16 unit instead of code point. Numbers print as
`serde_json` prints them: an integer as an integer, and any other number as the
shortest decimal that reads back to the same double, so `1e2` becomes `100.0`.
When a double lies exactly halfway between two equally short decimals, either
is read and `serde_json`'s choice is written.
Board reads any valid JSON; only `put` rewrites a file into canonical form. One
key per line lets git's line merge combine most concurrent edits.

Mergiraf is the recommended merge driver for the rest. Board ships no
configuration for it. A user adds `*.json merge=mergiraf` to `.gitattributes`
and sets git's `merge.mergiraf.driver` to
`mergiraf merge --git %O %A %B -s %S -x %X -y %Y -p %P -l %L`. A merged file
may not be canonical until the next `put`.

## Paths, links and mounts

A logical path names an entry from the root of the board that holds it. A
pointer is a record that links to a path in the same board or mounts another
repository at an exact commit. [architecture/paths.md](architecture/paths.md)
holds the segment rules, addressing, shadowing and case collisions, how
resolution follows links and mounts, and which paths a write refuses.

## The index

The index is a SQLite cache of what the tree holds. It is disposable:
deleting it changes no answer, only speed.
[architecture/index.md](architecture/index.md) holds where it lives, its
tables and views, how a refresh decides what to redo, and `board query` and
`board refs`.

## Writes

`board put <path>` reads one JSON record from stdin, then:

1. takes `board.lock` in the worktree's git directory, waiting up to 5 seconds
   and then refusing with `board::put::locked`. The steps below run under the
   lock, so a record is checked against the board it is written to, and other
   writers wait while it is checked;
2. checks it with the codes `check` uses: size and strict parse, `$type`,
   schema, validity, and that references parse. A reference need not resolve.
   Input that is not one JSON object with a `$type` is `board::put::invalid`.
   `board/mount/v1` and `board/config/v1` are checked as their own types;
3. resolves and checks the path, as architecture/paths.md says under Writes;
4. with `--expect <blob>`, or `--expect none` for a new file, refuses with
   `board::put::stale` unless the target's current blob matches. Without
   `--expect`, a put overwrites;
5. writes canonical bytes through a temporary file and a rename. A kill leaves
   the old file or the new one; Board claims kill safety, not durability across
   power loss;
6. commits, when `commit_on_put` is set, as `git commit <paths>` does: only the
   written paths, onto HEAD. The author is git's `user.name` and `user.email`;
   without them the put is refused before writing with
   `board::put::no_identity`. During a merge, cherry-pick, revert or rebase it
   is refused before writing with `board::put::locked`. No hooks run. If HEAD
   moved meanwhile, the files are written and not committed, with
   `board::put::head_moved`. Any other failure is `board::put::failed`, and
   says whether the commit was made.

`board put --batch` reads at most 10,000 JSON Lines of `{path, expect,
record}`, then takes the lock; when no line parses, it is refused without the
lock. Every line is checked against one snapshot of the working tree, which
lists each directory once, and every expect is compared before any write; two
lines resolving to one file, one invalid line or one stale expect refuse the
whole batch, listing each failing line with its code, as
[architecture/errors.md](architecture/errors.md) describes. Writes go in input
order, then one commit.

The report is `board/put-report/v2`, whose `data` holds `writes`, each with its Location, display
path, `previous` blob (null for a new file), `blob` and `degraded` (as `check`
reports it, or null); and `commit` (null when none was made).

`board mount update <path>` takes the lock, then reads the mount pointer at
`path`, which it does not follow. Anything other than a mount pointer there is
`board::mount::invalid`. It reads the pointer's `follow` ref in the mounted
repository (missing: `board::mount::follow_missing`) and, if the commit
changed, rewrites `commit` through `put` with `--expect` set to the pointer's
blob. The report, `board/mount-update-report/v2`, gives the path, the old and
new commit and whether it wrote.

A put of one record through a mount writes it in the mounted repository and
moves the pin, as [architecture/writes.md](architecture/writes.md) says.

## Commands

| Command | Does |
| --- | --- |
| `board init` | write a minimal `.board/board.json` at this git repository's root unless one exists; commit nothing |
| `board check [path]` | refresh, then report every entry; exit 1 if any is degraded |
| `board get <path>...` | resolve and print each file's stored bytes, up to 16 MiB each, all from one run; see below |
| `board ls <path>` | list a directory without following pointers, each entry with the status `check` gives it |
| `board refs <path>...` | references to and from each object, and values in each target, all from one check of the board; see docs/architecture/index.md |
| `board put <path>` | validate and write a record |
| `board mount update <path>` | advance a mount to its `follow` |
| `board query <sql>` | read-only SQL over the index |
| `board plugin list` / `find` | installed plugins, and every capability or those that match the filters given |
| `board plugin enable <name>` | write the plugin's schemas, then name it in `.board/board.json` |
| `board call <plugin>.<capability>` | call a capability; see docs/plugins.md |

`board get` serves an agent that reads several records: it names them all in
one run instead of starting one run each. With one path, text output is the
file's bytes. With several, each file follows a `==> <path> <==` line, as
`head` prints them, and a blank line separates files. That text cannot always
be split back into each file's exact bytes, for example when a file lacks a
final newline; `--json` is the exact form. It is always one
`board/get-report/v2`, for one path too. `data` holds, in the order
asked, each file's Location, whose source is `worktree` or a commit id, its
blob, its bytes, and either its `record` or the `code` saying why it is not
one. On an answer, `code: board::record::not_a_record` means a readable file
that holds no record, such as markdown. The bytes are a JSON string under
`text` when they are UTF-8, and `base64` otherwise. A path that names a
directory or nothing is refused in `failures`, with the `index` of its
path; the other paths are still answered, and the run
exits 1. One run takes at most 10,000 paths; more is `board::cli::usage`. Like
every command, `get` collects its output in memory and writes it at exit. It
reads each file only once the answers before it are rendered, and only while
the output, counted in the format being printed, is under 64 MiB; the first
path is always read. One `board::output::over_budget` failure names the later
paths in `left_out`, to ask for in another run. So a run holds at most
64 MiB, one more answer, and the short errors of the paths it left. The plugin
library's `Board.get` does that. `controls.check` reads every
file its measured controls reach this way.

Each command returns one typed report. `--json` prints it as crumbkit's
`board/<command>-report/v2`, or `board/failure/v1` on failure;
[architecture/errors.md](architecture/errors.md) gives both shapes. Exit codes: 0 done,
1 failed, 2 a refused command line (`board::cli::usage`).

`-C <dir>` before the command runs it as if Board started in `<dir>`. Several
combine as in git: a relative one is taken from the one before. A `<dir>` naming
no directory is `board::cli::usage`.

Tab completion covers commands, flags and logical paths, through
`clap_complete`'s dynamic engine. `BOARD_COMPLETE=<shell> board`, with no other
argument, prints a script for bash, zsh, fish, elvish or powershell; on each Tab
the script runs `BOARD_COMPLETE=<shell> board -- <words>`. Any other command
line runs as usual whatever `BOARD_COMPLETE` holds. Install it in fish with
`BOARD_COMPLETE=fish board | source` in `config.fish`, in bash with
`eval "$(BOARD_COMPLETE=bash board)"` in `.bashrc`, and in zsh with
`eval "$(BOARD_COMPLETE=zsh board)"` in `.zshrc`.

`Board::complete` resolves the directory the typed path ends in, through links,
mounts and any `-C` (a leading `~/` is the home directory), then
`Repository::names` lists that one level without hashing, validating or
indexing anything: about 20 ms in a directory of 5,000 records. A directory
ends in `/`. A link or mount is offered without one, since a listing does not
follow pointers; type the `/` to continue through it. A typed word's quotes
and `\` escapes are removed first. fish completes a name with a space typed
with `\ ` or inside quotes. bash is offered names with `\` escapes, so a space
must be typed as `\ `: bash does not complete inside quotes, and it splits a
word at `:`. zsh is offered names with `:` and `\` escaped. A `-C` after the
cursor is ignored. Completion writes
nothing and prints no error into the shell.

`check`, `ls`, `refs`, `plugin list` and `plugin find` show 100 entries unless
`--limit` or `--all` says otherwise; an entry is a printed line: a `refs`
pointer, a `plugin list` plugin. Text names an entry by its logical path,
or its file when it has none or is shadowed; JSON gives both.
STANDARDS.md 23 to 26 hold the rules.

## Crates

| Crate | Owns | Depends on |
| --- | --- | --- |
| `board-model` | pure types and decisions: logical paths, Locations, record types, codes and their hints, refusals, canonical JSON, the duplicate-key parse | serde, serde_json, thiserror |
| `board-tree` | every read and write of git objects and the working tree; resolution; walking a board and every board its mounts reach | board-model, gix |
| `board-validate` | JSON Schema validation of record bytes against schema bytes | board-model, jsonschema |
| `board-index` | the index, the timing log and the cache directory | board-model, board-tree, rusqlite |
| `board-plugin` | manifests, discovery, child processes, signals | board-model, board-validate, rustix and signal-hook (Unix) |
| `board-ops` | every operation as a typed function returning its report | all of the above |
| `board-cli` | the `board` binary: arguments (clap), configuration, the one printer | board-model, board-ops, board-plugin, board-tree |

`board-cli` only parses arguments, reads configuration and prints, so a later
adapter, such as a server, needs no logic of its own. Rust code names the
app only through `DISPLAY_NAME` (`Board`) and `BINARY_NAME` (`board`) in
`board-model`; neither governs codes, the `board` type namespace, crate names
or paths. A plugin runs the installed executable by its name, so a rename also
changes plugins such as `tasks`.

## Codes

Codes are `board::<area>::<reason>`, spelled once in code, and change as
STANDARDS.md 3 and 4 say. [architecture/errors.md](architecture/errors.md)
lists the codes and the values an error names in text and in
`board/failure/v1`.

## Testing

`board-scenario` builds real git repositories in temporary directories, so
links, mounts, cycles and writes are tested against git itself. STANDARDS.md 19
and 20 set the property and work-count tests. Its degraded seed board
(`board_scenario::seed`) holds an entry for every degraded code `check`
reports, each listed with its code. A test requires `board check --json` to
give exactly those codes from a cold index, a warm one and one in memory.
Another requires commands run against it and a few other fixtures to produce
every code except the codes it lists as not reproduced.

## Performance

Each operation has a time budget, measured on a generated 10,000-record board,
and `cargo xtask bench` fails above three times a budget.
[architecture/performance/budgets.md](architecture/performance/budgets.md)
lists the budgets and targets. Every `board` run whose command line parses
appends a record of its phases to a timing log, described in
[architecture/performance/timing.md](architecture/performance/timing.md).
