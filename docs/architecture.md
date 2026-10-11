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
`board::config::invalid`. Every file git sees
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
Schemas are JSON Schema 2020-12. A released type never changes; a change is a
new version.

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

The report is `board/put-report/v1`: `writes`, each with its Location, display
path, `previous` blob (null for a new file), `blob` and `degraded` (as `check`
reports it, or null); and `commit` (null when none was made).

`board mount update <path>` takes the lock, then reads the mount pointer at
`path`, which it does not follow. Anything other than a mount pointer there is
`board::mount::invalid`. It reads the pointer's `follow` ref in the mounted
repository (missing: `board::mount::follow_missing`) and, if the commit
changed, rewrites `commit` through `put` with `--expect` set to the pointer's
blob. The report, `board/mount-update-report/v1`, gives the path, the old and
new commit and whether it wrote.

## Commands

| Command | Does |
| --- | --- |
| `board init` | write a minimal `.board/board.json` at this git repository's root unless one exists; commit nothing |
| `board check [path]` | refresh, then report every entry; exit 1 if any is degraded |
| `board get <path>` | resolve and print a file's stored bytes, up to 16 MiB; `--json` prints a record with its Location, whose source is `worktree` or a commit id, and its blob, and refuses other files with the code saying why; a directory is `board::record::not_a_record` |
| `board ls <path>` | list a directory without following pointers, each entry with the status `check` gives it |
| `board refs <path>...` | references to and from each object, all from one check of the board |
| `board put <path>` | validate and write a record |
| `board mount update <path>` | advance a mount to its `follow` |
| `board query <sql>` | read-only SQL over the index |
| `board plugin list` / `find` | installed plugins and their capabilities |
| `board plugin enable <name>` | write the plugin's schemas, then name it in `.board/board.json` |
| `board call <plugin>.<capability>` | call a capability; see docs/plugins.md |

Each command returns one typed report. `--json` prints it as a document tagged
`board/<command>-report/v1`, or `board/error/v1` on failure. Exit codes: 0 done,
1 failed, 2 a refused command line (`board::usage::invalid`).

`-C <dir>` before the command runs it as if Board started in `<dir>`. Several
combine as in git: a relative one is taken from the one before. A `<dir>` naming
no directory is `board::usage::invalid`.

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
pointer, a `plugin list` capability. Text names an entry by its logical path,
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

Codes are `board::<area>::<reason>`, spelled once in code and never changed
after release. [architecture/errors.md](architecture/errors.md) lists the v0
set and the values an error names in text and in `board/error/v1`.

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

These budgets are macos-15 medians on the generated 10,000-record board,
measured at M4 and rounded up. The exported old corpus (about 8,000 records) is
the other reference board.

| Operation | Budget |
| --- | --- |
| `board get`, warm | 5 ms |
| `board ls` of a record's directory, warm | 500 ms |
| `board check`, warm, nothing changed | 500 ms |
| simple `board query`, warm | 500 ms |
| full index, cold | 1 s |

`cargo xtask bench` is the timing job. It runs on a generated board in CI,
reports every run and fails above three times a budget. The owner times the
private export locally with `--board`. The work-count tests are the hard gate.

Targets are the speed an agent loop or a web view needs. None is met or
enforced yet:

| Operation | Target |
| --- | --- |
| `board check`, warm, nothing changed | 100 ms |
| simple `board query`, warm | 100 ms |

### Timing log

Every `board` run whose command line parses appends one JSON line, tagged
`board/timing/v1`, to `$XDG_CACHE_HOME/board/timings.jsonl` (by default
`~/.cache/board/`), beside the boards' indexes. Help, `--version`, completion
and a refused command line record nothing. The log is created readable only by
its owner (mode 0600). `BOARD_TIMINGS=0` turns recording off. A plugin inherits
`BOARD_TIMINGS` and `XDG_CACHE_HOME`, so the runs it starts record to the same
log.

A record holds the start time (`at_ms`), Board's `version`, the subcommands
run (`command`, such as `plugin find`, never the values given), the plugin call
`depth`, the board's absolute path (`root`), the `exit` code and `error` code,
`total_us`, and `phases`. Each phase gives its
`name`, its start after the run's (`at_us`), its time (`us`) and how often it
was `entered`, in the order the phases started. A record has only the phases
its run entered:

| Phase | What it times |
| --- | --- |
| `parse` | parsing the command line |
| `open` | finding and opening the board |
| `resolve` | resolving the path that `get`, `ls`, `refs` or a scoped `check` names |
| `index.open` | opening the index |
| `snapshot` | refreshing the index and deciding every entry, for `check`, `ls`, `query` and `refs` |
| `index.refresh` | the refresh, within `snapshot` |
| `view` | filling the views with what the snapshot decided, for `query` |
| `query.sql` | running the SQL statement |
| `plugin.extract` | extracting records for one plugin, in `query` |
| `plugin.call` | running a capability |
| `plugin.reply` | from starting a plugin process to the first byte of its reply, once for each process that replied |
| `put.validate` | checking a record before it is written |
| `put.write` | writing the records and committing |
| `emit` | printing the output |

`work` holds the counts a run has: `entries`, `hashed`, `parsed`, `validated`,
`trees`, `rows`, and `started`, the plugin processes a call or a query's
extraction started. Each extraction batch and each call starts one. `plugin.reply`
includes the plugin's own work, such as the `board query` it runs, which
records its own line one `depth` deeper. In a `query`, it sums the replies of
the extraction batches.

Before a line would take the log past 1 MiB, the log is renamed to
`timings.jsonl.1`, replacing the one before. The rename takes no lock, so two
runs that fill the log at once can lose the older file. The run that renames
second also loses its own record, because the log it tried to rename is
already gone. Recording happens after the output is printed. A record that cannot be written is dropped silently, and
the run's output and exit code stay as they were. Read the log with `jq`. The
`root` of each record shows the names of your directories, so check the log
before you attach it to a bug report.
