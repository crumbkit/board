# Architecture

This is the v0 design. Built so far: logical paths, resolution and walking,
links and mounts, records, schemas and references, the index, canonical form,
plugins with extraction, and the commands `init`, `check`, `get`, `ls`, `query`,
`refs`, `put`, `mount update`, `plugin list`, `plugin find` and `call`. PLAN.md
says what comes next.

Board is a fast, validated view of a git tree. Git does history, branching,
merging and transport. Board indexes what is in a tree, validates the records
it understands, resolves references through links and mounts, and makes
records easy to read and write.

## Boards and entries

A board is the working tree of a git repository whose root holds
`.board/board.json`. Board finds it by searching upward from the current
directory, as git does; no board is `board::config::not_found`, and a
`board.json` Board cannot read is `board::config::invalid`. Every file git sees
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
`record::unknown_type`. A file under `.board/types/board/` is `schema::invalid`,
because Board's own types are checked by Rust types. A schema that does not
compile is `schema::invalid`, and so is each record of its type.
`record::invalid` names the first failing instance pointer in sorted order.

Board's own documents (`board/*`) refuse unknown fields. User records follow
their schema. Board knows `board/config/v1`, `board/mount/v1` and
`board/plugin-manifest/v1` wherever they are stored, so a repository holding a
plugin's source checks clean. A manifest is checked by its content alone; a
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

## Logical paths

A logical path is `/` alone, the board's root, or `/` followed by segments
separated by `/`. A segment is non-empty UTF-8 of at most 255 bytes, without
`/`, NUL or control characters, and is not `.` or `..`. A path is at most 4,096
bytes. A record's file name adds `.json`, so its segment is at most 250 bytes.
Paths are absolute and are read against the root of the board that holds them.
The working tree must also fit the OS's `PATH_MAX` (1,024 bytes on macOS, 4,096
on Linux) with its checkout prefix; Board keeps its own limit because paths
read through mounts never touch the file system.

In a directory, a segment `x` names one of:

- a record or pointer stored as `x.json`;
- a directory `x/`;
- a non-JSON file whose whole name is `x`, such as `notes.md`.

A `.json` file is addressed only without its extension, so every entry has one
address. A record `x.json` and a directory `x/` may sit side by side, so a
record can have children: `/x` names the record, `/x/y` continues into the
directory, and `ls /x` lists the directory. Any other match of one segment to
more than one entry, including a pointer beside a directory, degrades each with
`board::path::shadowed`, and resolution refuses with the same code. Two names in
one directory that differ only by ASCII case are degraded with
`board::path::case_collision`, because a macOS working tree cannot hold both;
resolution still matches names exactly. A name that is not a valid segment is
`board::path::malformed` and cannot be reached. A path that names nothing,
including one that continues through a file, is `board::path::not_found`. A
repository that cannot be opened, or a missing or corrupt object, is
`board::repo::unreadable`.

## Pointers: links and mounts

A pointer is a record of type `board/mount/v1` holding exactly one target:

```json
{ "$type": "board/mount/v1", "path": "/work/company-name/projects/x" }
```

```json
{
  "$type": "board/mount/v1",
  "repo": "~/src/company-board",
  "follow": "main",
  "commit": "3f9a…"
}
```

The first is a link to a path in the same board. The second is a mount of
another repository at an exact commit. `follow` names what `board mount update`
advances `commit` to; `commit` is what Board reads. v0 mounts local
repositories only; a URL is `board::mount::remote_unsupported`.

A pointer at `dir/name.json` makes `/dir/name` its point: resolution continues
through it. The semantics are POSIX symlinks':

- Reaching a link restarts at the link's `path` in the board that holds it,
  then continues with the remaining segments. Reaching a mount continues at the
  root of the mounted tree at `commit`.
- A resolution follows at most 40 pointers (`board::resolve::too_many_hops`).
  Returning to a board and path already visited is `board::resolve::cycle`.
- An absolute path in a mounted board means that board's path.
- A pointer whose target is missing is degraded with `board::ref::dangling`;
  nothing else breaks.
- The root board is read from its working tree, uncommitted edits included and
  marked. A
  mounted board is read from committed objects at `commit`, so a board at a
  given commit fully determines everything under its mounts.
- Mounting brings data and schemas, never code. A mounted board's plugin list
  is ignored.

Resolution returns a `Location`: the board (repository and source, either the
working tree or a commit), the physical path, the object (a blob hash, or a
directory), and every pointer followed. A Location's identity is its board,
source and physical path; two logical paths that reach one identity are one
object, reported once. The display path replaces every link with its target and
keeps mount points.

Resolving follows pointers. Walking (`ls`, the index) does not: a walk reports
a pointer as an entry, as `find` does without `-L`. The index includes every
board reachable through mounts, each commit indexed once however many places
mount it. Only `board-tree` reads pointers.

## The index

The index is SQLite under `$XDG_CACHE_HOME/board/<root-hash>/` (by default
`~/.cache/board/<root-hash>/`), where `<root-hash>` is the SHA-256 of the board
root's canonical path. It is disposable: deleting it changes no answer, only
speed. SQLite is bundled, so every platform runs the same one.

| Table | Holds | Keyed by |
| --- | --- | --- |
| `sources` | the root working tree and each mounted commit: its root tree and the repository it was first read from | source: `worktree` or a commit id |
| `trees` | every entry in a tree: name, mode, object, logical segment and path code | tree |
| `worktree` | each working-tree entry's logical path, path code, blob and stat (size, times, inode, mode) | path |
| `blobs` | parse result in the role the path gives (`json`, `schema`, `reserved_schema`): kind, type, code; a schema's code says whether it compiles | blob, role |
| `bodies` | the canonical form of each record that parses | blob |
| `validations` | schema result: code, failing JSON pointer | record blob, schema blob |
| `refs` | references a valid record makes, in order: JSON pointer, target path, accepted types | record blob, schema blob, ordinal |
| `extracted` | the blobs each plugin version has handled; a query sees those that are valid records now | plugin, version, blob |
| `mounts` | each mount point and the commit it reaches, or its code | source, path |
| `state` | index format and when the last refresh began | key |

The `entries` view joins these across the root and its mounts, one row per
entry: `source`, `path`, `logical` (the display path), `blob`, `size`, `kind`,
`type`, `status`, `code`, `change`, `role` and `schema` (the schema blob a
record was checked against). The `records` view is its rows of kind `record`,
with `source`, `path`, `logical`, `blob`, `type`, `status`, `code` and `body`;
`body` is NULL when the record does not parse. Tables, views and columns are an
interface agents write SQL against; changing one changes the index format. A
commit mounted in several places appears once, with `logical` through the
shortest display path, then the first lexically. Everything derived is keyed by
content (blob or tree hash), never by commit or time, so branches share results.

Each command refreshes the index first, doing work in proportion to what
changed: it hashes only working-tree files whose stat changed, as git does, and
parses and validates only blobs it has not seen. `check`, `refs` and `query`
use the index; `get` and `ls` resolve directly.

`board query '<sql>'` runs one read-only statement against the index. Each
enabled plugin's database is attached read-only as `p_<name>`. Rows return as
JSON, at most 100 unless `--limit` (at most 10,000) says otherwise, with
`truncated` set when rows were left out. The report is `columns` and `rows`,
each row an array in column order; `plugins` lists any plugin whose extraction
failed. SQL that does not parse, would write, or holds more than one statement
is `board::query::invalid`; ATTACH, DETACH and PRAGMA statements count as
writes, while table-valued pragma functions such as `pragma_table_info` are
admitted. A query still running after 1 s is `board::query::timeout`. A query
sees each fact table filtered to what the current tree reaches, so it answers
the same warm as cold; `main.<table>` is the whole cache, which keeps facts for
other branches.

`board refs <path>` reports each reference the record makes, resolved now with
its status, and each record whose references reach its identity, whatever path
they name.

## Writes

`board put <path>` reads one JSON record from stdin, then:

1. checks it with the codes `check` uses: size and strict parse, `$type`,
   schema, validity, and that references parse. A reference need not resolve.
   Input that is not one JSON object with a `$type` is `board::put::invalid`.
   `board/mount/v1` and `board/config/v1` are checked as their own types;
2. resolves the path, following pointers, so a write through a link lands at
   its target; put does not create a missing target, and refuses with its
   resolution code. A path through a mount is `board::put::through_mount`. A
   segment git keeps, `.git` under any spelling HFS+ or NTFS opens as it, is
   `board::path::malformed`, in `mount update` too. Missing directories are
   created. A write that would leave its own path degraded is refused with that
   path's code, such as `path::shadowed`;
3. takes `board.lock` in the worktree's git directory, waiting up to 5 seconds
   and then refusing with `board::put::locked`;
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
record}`. Every line is checked and every expect compared under the lock before
any write; two lines resolving to one file, one invalid line or one stale
expect refuse the whole batch, listing each failing line with its code (in
`board/error/v1` as `fields.refusals`). Writes go in input order, then one
commit.

The report is `board/put-report/v1`: `writes`, each with its Location, display
path, `previous` blob (null for a new file) and `blob`; and `commit` (null when
none was made).

`board mount update <path>` names a mount pointer, which it does not follow;
anything else there is `board::mount::invalid`. It reads the pointer's `follow`
ref in the mounted repository (missing: `board::mount::follow_missing`) and, if
the commit changed, rewrites `commit` through `put` with `--expect` set to the
pointer's blob. The report, `board/mount-update-report/v1`, gives the path, the
old and new commit and whether it wrote.

## Commands

| Command | Does |
| --- | --- |
| `board init` | write a minimal `.board/board.json` at the root of the git repository here, unless one exists; commit nothing |
| `board check [path]` | refresh, then report every entry; exit 1 if any is degraded |
| `board get <path>` | resolve and print a file's stored bytes, up to 16 MiB; `--json` prints a record with its Location, and refuses any other file with the code that says why it is not one; a directory is `board::record::not_a_record` |
| `board ls <path>` | list a directory without following pointers |
| `board refs <path>` | references to and from an object |
| `board put <path>` | validate and write a record |
| `board mount update <path>` | advance a mount to its `follow` |
| `board query <sql>` | read-only SQL over the index |
| `board plugin list` / `find` | installed plugins and their capabilities |
| `board call <plugin>.<capability>` | call a capability; see docs/plugins.md |

Each command returns one typed report. `--json` prints it as a document tagged
`board/<command>-report/v1`, or `board/error/v1` on failure. Exit codes: 0 done,
1 failed (including degraded entries for `check`), 2 the command line was
refused, with `board::usage::invalid`.

`check`, `ls`, `refs`, `plugin list` and `plugin find` show 100 entries unless
`--limit` or `--all` says otherwise; an entry is a printed line, so `refs`
counts each pointer and `plugin list` each capability. `check` puts degraded
entries first. Text names an entry by its logical path, or by its file when it
has none or is shadowed; JSON gives both. A
cut listing ends with a stderr note giving the count shown and the total; JSON
carries `total` and `truncated`. An error prints `error: <code>: <message>`,
then `hint: <next step>`; `board/error/v1` carries the same `hint`. A refused
record names the failing JSON pointer and why. STANDARDS.md 23 to 26 hold the
rules.

## Crates

| Crate | Owns | Depends on |
| --- | --- | --- |
| `board-model` | pure types and decisions: logical paths, Locations, record types, codes, canonical JSON, the duplicate-key parse | serde, serde_json, thiserror |
| `board-tree` | every read and write of git objects and the working tree; resolution; walking a board and every board its mounts reach | board-model, gix |
| `board-validate` | JSON Schema validation of record bytes against schema bytes | board-model, jsonschema |
| `board-index` | the index and the cache directory | the three above, rusqlite |
| `board-plugin` | manifests, discovery, child processes | board-model, board-validate, board-index, rustix (Unix) |
| `board-ops` | every operation as a typed function returning its report | all of the above but `board-cli` |
| `board-cli` | the `board` binary: arguments (clap), configuration, the one printer | `board-ops` |

`board-cli` only parses arguments, reads configuration and prints, so a later
adapter, such as a server, needs no logic of its own. Rust code names the
app only through `DISPLAY_NAME` (`Board`) and `BINARY_NAME` (`board`) in
`board-model`; neither governs codes, the `board` type namespace, crate names
or paths. A plugin runs the installed executable by its name, so a rename also
changes plugins such as `tasks`.

## Codes

Codes are `board::<area>::<reason>`, spelled once in code and never changed
after release. The v0 set:

| Area | Reasons |
| --- | --- |
| `json` | `parse`, `merge_conflict`, `duplicate_key`, `too_large`, `not_utf8`, `lossy_number` |
| `record` | `bad_type`, `unknown_type`, `invalid`, `not_a_record` |
| `config` | `not_found`, `invalid` |
| `schema` | `invalid` |
| `ref` | `malformed`, `dangling`, `wrong_type` |
| `path` | `malformed`, `shadowed`, `case_collision`, `not_found` |
| `repo` | `unreadable`, `submodule` |
| `mount` | `invalid`, `repo_missing`, `commit_missing`, `remote_unsupported`, `follow_missing` |
| `resolve` | `cycle`, `too_many_hops` |
| `index` | `failed`: the disposable index could not be used, even in memory |
| `query` | `invalid`: the SQL does not parse, would write, or is more than one statement |
| `query` | `timeout`: the statement ran longer than 1 s |
| `put` | `stale`, `locked`, `through_mount`, `invalid`, `no_identity`, `head_moved`, `failed` (the file could not be written) |
| `plugin` | see docs/plugins.md |
| `usage` | `invalid`: the command line was refused, exit 2 |

## Testing

`board-scenario` builds real git repositories in temporary directories, so
links, mounts, cycles and writes are tested against git itself. STANDARDS.md 19
and 20 set the property and work-count tests.

## Performance

These budgets are macos-15 medians on the generated 10,000-record board,
measured at M4 and rounded up. The exported old corpus (about 8,000 records) is
the other reference board.

| Operation | Budget |
| --- | --- |
| `board get`, warm | 5 ms |
| `board check`, warm, nothing changed | 500 ms |
| simple `board query`, warm | 500 ms |
| full index, cold | 1 s |

`cargo xtask bench` is the timing job. It runs on a generated board in CI,
reports every run and fails above three times a budget. The owner times the M1
export locally with `--board`, since that data stays out of the repository.
The work-count tests are the hard gate.

Budgets are regression alarms, set from measurement. Targets are the speed an
agent loop or a web view needs. None is met or enforced yet:

| Operation | Target |
| --- | --- |
| `board check`, warm, nothing changed | 100 ms |
| simple `board query`, warm | 100 ms |
