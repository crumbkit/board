# Architecture

This is the v0 design. Built so far: logical paths, resolution and walking,
links and mounts, records, schemas and references, the index, canonical form,
plugin manifests and discovery, and the commands `check`, `get`, `ls`,
`query`, `refs`, `put`, `mount update`, `plugin list`, `plugin find` and
`call`.
PLAN.md says what comes next.

Board is a fast, validated view of a git tree. Git does history, branching,
merging and transport. Board indexes what is in a tree, validates the records
it understands, resolves references through links and mounts, and makes
records easy to read and write.

## Boards and entries

A board is the working tree of a git repository whose root holds
`.board/board.json`. Board finds it by searching upward from the current
directory, as git does; no board is `board::config::not_found`, and a
`board.json` Board cannot read is `board::config::invalid`. Every file git sees under the root is an
entry: tracked files, and untracked files that are not ignored. `.git` is never
an entry. An entry has a path, a blob hash, a size, a kind and a status.

| Kind | What it is |
| --- | --- |
| record | a `.json` file whose top-level object has a `$type` string; `.board/board.json` is one |
| pointer | a record of type `board/mount/v1` (a link or a mount) |
| schema | a file under `.board/types/` ending `.schema.json` |
| json | any other `.json` file |
| file | anything else: Markdown, images, code |

An entry's status is `ok` or `degraded` with exactly one code (see Codes).
Board never refuses to index a file; the worst it says is a path and a code.
A file or directory that cannot be read is degraded with
`board::repo::unreadable`. Working-tree files are hashed as stored, with no
clean filter (end-of-line conversion, LFS), so a file under one shows as
uncommitted.
A directory is not an entry, so a degraded directory name passes its code to
every entry beneath it.

A symlink is a `file` entry and is never followed; Board's links are pointers.
A submodule is degraded with `board::repo::submodule` and never descended:
mounts do that job.

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
record, so a mounted board's records are checked against the mounted board's
schemas. Schemas are JSON Schema 2020-12. A released type never changes; a
change is a new version.

Parsing is strict and bounded, and the first failure is the entry's code: over
16 MiB (`too_large`), not UTF-8, git conflict markers (`merge_conflict`), not
JSON (`parse`, including a byte-order mark or nesting past serde_json's limit),
then a duplicate key at any depth, then a number that a double or a 64-bit
integer cannot hold exactly (`lossy_number`), so nothing reads a rounded value.
A `$type` that is not a string of the form
above is `record::bad_type`; a type with no schema file is
`record::unknown_type`. Board's own types are checked by Rust types, so a file
under `.board/types/board/` is `schema::invalid`. A schema that does not compile
is `schema::invalid`, and so is each record of its type, which cannot be
checked. `record::invalid` names the first failing instance pointer in sorted
order, so the report is deterministic.

Board's own documents (`board/*`) refuse unknown fields. User records follow
their schema: a schema that allows extra fields gets extra fields.

### Typed references

A schema marks a string as a reference with `"format": "board-ref"`, and may
name the types it accepts with `"x-board-type"` (a string or a list). A
reference is an absolute logical path, read in the referring record's board;
`x-board-type` names exact `$type` values. Board finds references through
`properties`, `patternProperties`, `additionalProperties`, `items`,
`prefixItems`, `allOf` and local `$ref`; branches of `anyOf`, `oneOf` and
`if` still validate but are not searched. A string that is not a logical path
is `ref::malformed`. `check` reports the first failing reference in document
order on the referring record: `ref::dangling`, or, when the schema names
types, `ref::wrong_type` for anything but a record of one of them. Without
`x-board-type`, a reference may reach any entry, such as a record's Markdown.

Schemas from a mounted board are untrusted input. Board validates with
`pattern` on a linear-time regex engine and never fetches a remote `$ref`.

### Canonical form

Every file Board writes is canonical: keys sorted by code point, two-space
indentation, one key or array element per line, empty `{}` and `[]` on one
line, `\n` endings, one trailing newline and no byte-order mark. Strings escape
only `"`, `\` and control characters. Numbers are normalised: an integer as an
integer, any other number as the shortest decimal that reads back to the same
double, so `1e2` becomes `100.0`. A number that cannot be held exactly never
parses, so it is never rounded. Golden bytes freeze this. Board reads any valid
JSON; only `put` rewrites a file into canonical form. One key per line is what
lets git's line merge combine most concurrent edits.

## Logical paths

A logical path is `/` alone, the board's root, or `/` followed by segments
separated by `/`. A segment is
non-empty UTF-8 of at most 255 bytes, without `/`, NUL or control characters,
and is not `.` or `..`. A path is at most 4,096 bytes. A record's file name
adds `.json`, so a record's segment is at most 250 bytes. Paths are absolute and
are read against the root of the board that holds them.

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
degraded with `board::path::malformed` and cannot be reached. A path that names
nothing, including one that continues through a file, is refused with
`board::path::not_found`. A repository that cannot be opened, or a missing or
corrupt object, is `board::repo::unreadable`.

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
repositories only; a URL is degraded with `board::mount::remote_unsupported`.

A pointer at `dir/name.json` makes `/dir/name` its point: resolution continues
through it. The semantics are POSIX symlinks':

- Resolution reads segments in order. Reaching a link restarts at the link's
  `path` in the board that holds the link, then continues with the remaining
  segments. Reaching a mount continues with the remaining segments at the root
  of the mounted tree at `commit`.
- A resolution follows at most 40 pointers (`board::resolve::too_many_hops`).
  Returning to a board and path already visited is `board::resolve::cycle`.
- An absolute path in a mounted board means that board's path. Mounting never
  changes what a path inside the mounted board means.
- A pointer whose target is missing is degraded with `board::ref::dangling`;
  nothing else breaks.
- The root board is read from its working tree, uncommitted edits included and
  marked. A mounted board is read from committed objects at `commit`, so a
  board at a given commit fully determines everything under its mounts.
- Mounting brings data and schemas, never code. A mounted board's plugin list
  is ignored.

Resolution returns a `Location`: the board (repository and source, either the
working tree or a commit), the physical path, the object (a blob hash, or a
directory), and every pointer followed. A Location's identity is its board, source and physical path. Two
logical paths that reach one identity are one object; backlinks and query
results report the identity once. The display path replaces every link with its
target and keeps mount points.

Resolving follows pointers. Walking (`ls`, the index) does not: a walk reports
a pointer as an entry, as `find` does without `-L`. The index includes every
board reachable through mounts, each indexed once per commit however many
places mount it.

Only `board-tree` reads pointers. Every other crate receives Locations.

## The index

The index is SQLite under `$XDG_CACHE_HOME/board/<root-hash>/` (by default
`~/.cache/board/<root-hash>/`), where
`<root-hash>` is the SHA-256 of the board root's canonical path, on macOS
too. It is disposable: deleting it changes no answer, only speed. An index
another process holds past a five-second wait is skipped for that run in
favour of one in memory; a corrupt index, or one of another format, is
deleted and rebuilt. SQLite is bundled, so every platform runs the same one.
A run whose index is in memory says so in one line on stderr.

| Table | Holds | Keyed by |
| --- | --- | --- |
| `sources` | the root working tree and each mounted commit: its root tree and the repository it was first read from | source: `worktree` or a commit id |
| `trees` | every entry in a tree: name, mode, object, logical segment and path code | tree |
| `worktree` | each working-tree entry's logical path, path code, blob and stat (size, times, inode, mode) | path |
| `blobs` | parse result in the role the path gives (`json`, `schema`, `reserved_schema`): kind, type, code; a schema's code says whether it compiles | blob, role |
| `validations` | schema result: code, failing JSON pointer | record blob, schema blob |
| `refs` | references a valid record makes, in order: JSON pointer, target path, accepted types | record blob, schema blob, ordinal |
| `mounts` | each mount point and the commit it reaches, or its code | source, path |
| `state` | index format, root commit and when the last refresh began | key |

The `entries` view joins these across the root and its mounts, one row per
entry: `source`, `path`, `logical` (the display path), `blob`, `size`, `kind`,
`type`, `status`, `code`, `change`, `role` and `schema` (the schema blob a
record was checked against). Tables, views and columns are an
interface agents write SQL against; changing one changes the index format.
A commit is one source however many places mount it, so its entries appear
once, with `logical` through the first route: the shortest display path, then
the first lexically. A scoped check takes an entry reached by any route at or
beneath the scope. Text output names a mounted entry by its logical path.
Each run decides `kind`, `code` and `change` and keeps them in a temporary
table on its own connection, so `entries` shows what that run checked and
nothing decided per run is stored.
Everything derived is keyed by content (blob or
tree hash) and never by commit or time, so branches share results and a fact is
true at a commit exactly when its blob is in that commit's tree.

Each command refreshes the index first: compare the root commit and working
tree with `state`; diff old and new trees; hash only the working-tree files
whose stat changed, or whose modification time is not before the second the
last refresh began, as git does; parse and validate only blobs not yet in `blobs` and
`validations`. Editing a schema invalidates exactly the validations that used
it, because they are keyed by its blob. Whether a reference or pointer
resolves depends on the whole tree, not one blob, so `check` recomputes it
from `refs` on each run rather than storing it. `check`, `refs` and `query`
use the index; `get` and `ls` resolve directly.

`board query '<sql>'` runs one read-only statement against the index (opened
with `query_only`). Each enabled plugin's database is attached read-only as
`p_<name>`. Rows return as JSON, at most 500 unless `--limit` (at most 10,000)
says otherwise, with `truncated` set when rows were left out. The report is
`columns` and `rows`, each row an array in column order, since a join can repeat
a column name; `plugins` lists any plugin whose extraction failed. SQL that does
not parse, would write, or holds more than one statement is
`board::query::invalid`. A query sees each fact table filtered to what the
current tree reaches, so it answers the same warm as cold; `main.<table>` is the
whole cache, which keeps facts for other branches. There is no query language beyond SQL: an agent writes SQL.

`board refs <path>` reports each reference the record makes, resolved now with
its status, and each record whose references reach it. A reference reaches it
when it resolves to its identity, whatever path it names.

## Writes

`board put <path>` reads one JSON record from stdin, then:

1. checks it as a record, with the codes `check` uses: strict parse within the
   record size bound, `$type` present, schema found and valid, the record valid
   against it, references parse. A reference need not resolve. Input that is
   not one JSON object with a `$type` is `board::put::invalid`. `board/mount/v1`
   and `board/config/v1` are checked as their own types;
2. resolves the path, following pointers, including a link at the last
   segment, so a write through a link lands at its target. A path through a
   mount is `board::put::through_mount`. Missing directories are created. A
   write that would leave its own path degraded is refused with that path's
   code, such as `path::case_collision` or `path::shadowed`. A link at the last
   segment whose target does not exist is refused with its resolution code;
   put does not create a link's target;
3. takes `board.lock` in the worktree's git directory, created exclusively as
   git creates its lock files, waiting up to 5 seconds and then refusing with
   `board::put::locked`, whose message says to remove the lock if no put is
   running;
4. with `--expect <blob>`, or `--expect none` for a new file, refuses with
   `board::put::stale` unless the target's current blob matches. Without
   `--expect`, a put overwrites;
5. writes canonical bytes to `.<name>.board-tmp` beside the target, fsyncs it,
   renames it into place and fsyncs the directory. An existing file keeps its
   mode. A kill leaves the old file or the new one; a leftover temporary file is
   an ordinary file that `check` shows. Board claims kill safety, not durability
   across power loss;
6. commits, when `commit_on_put` is set, as `git commit <paths>` does: only the
   written paths, onto HEAD, with git's index updated for them under git's own
   `index.lock`. The author is git's `user.name` and `user.email`; without them
   the put is refused before writing with `board::put::no_identity`. During a
   merge, cherry-pick, revert or rebase it is refused before writing with
   `board::put::locked`, as git refuses a partial commit then. No hooks run.
   The commit object is made before the renames, and HEAD moves only if it
   still holds the commit read under the lock; otherwise the report says the
   files were written and not committed, with `board::put::head_moved`. Any
   other failure is `board::put::failed`, and says whether the commit was made.

`board put --batch` reads at most 10,000 JSON Lines of `{path, expect,
record}`. Every line is checked and every expect compared under the lock
before any write; two lines resolving to one file, one invalid line or one
stale expect refuse the whole batch, listing each failing line with its code
(in `board/error/v1` as `fields.refusals`). Writes go in input order, then one
commit.

The report is `board/put-report/v1`: `writes`, each with its Location, display
path, `previous` blob (null for a new file) and `blob`; and `commit` (null when
none was made).

`board mount update <path>` names a mount pointer, which it does not follow;
anything else there is `board::mount::invalid`. It reads the pointer's `follow` ref in the mounted repository (missing:
`board::mount::follow_missing`) and, if the commit changed, rewrites `commit`
through `put` with `--expect` set to the pointer's blob. The report,
`board/mount-update-report/v1`, gives the path, the old and new commit and
whether it wrote.

## Commands

| Command | Does |
| --- | --- |
| `board check [path]` | refresh, then report every entry; exit 1 if any is degraded |
| `board get <path>` | resolve and print a record's stored bytes; `--json` adds its Location; anything but a record is `board::record::not_a_record` |
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
refused. Reads report every entry even after a failure; a write changes
everything it was asked to or nothing.

## Crates

| Crate | Owns | Depends on |
| --- | --- | --- |
| `board-model` | pure types and decisions: logical paths, Locations, record types, codes, canonical JSON, the duplicate-key parse | serde, serde_json, thiserror |
| `board-tree` | every read and write of git objects and the working tree; resolution and walking | board-model, gix |
| `board-validate` | JSON Schema validation of record bytes against schema bytes | board-model, jsonschema |
| `board-index` | the index and the cache directory | the three above, rusqlite |
| `board-plugin` | manifests, discovery, child processes | board-model, board-validate, board-index |
| `board-ops` | every operation as a typed function returning its report | all of the above but `board-cli` |
| `board-cli` | the `board` binary: arguments (clap), configuration, the one printer | `board-ops` |

Every operation is a typed library function that returns its report.
`board-cli` only parses arguments, reads configuration and prints, so a later
adapter such as `board serve` needs no logic of its own.

`deny.toml` lets only `board-tree` depend on gix, only `board-index` on rusqlite
and only `board-validate` on jsonschema. `clippy.toml` names the owner of each
effect. No crate uses an async runtime.

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
| `put` | `stale`, `locked`, `through_mount`, `invalid`, `no_identity`, `head_moved`, `failed` (the file could not be written) |
| `plugin` | see docs/plugins.md |

## Testing

A scenario builder makes real git repositories in temporary directories. These
scenarios run in every test run:

- a plain path; a link in one board; a mount; a link into a mount;
- nested mounts; one board mounted twice (indexed once, one identity);
- an absolute path inside a mounted board (read against that board);
- cycles: link to link, and a board that mounts its own parent;
- 39, 40 and 41 hops;
- a dangling link; a link whose target vanishes when its mount's commit moves;
- a record beside its directory of children; a pointer and a directory at one
  path; names differing only by case;
- a write through a link (lands at the target) and through a mount (refused);
- an uncommitted edit in the root beside committed mounted content.

Property tests over generated boards check that resolution always ends; that
resolving an identity's display path returns that identity; that a link
resolves as its target does; that a walk never yields an identity twice; that
a result depends only on the root tree and the path; and that **every query
answers the same from a cold rebuild as from a warm index**.

Work-count tests are deterministic: after one record changes, a refresh parses
exactly one blob; resolving through `n` pointers reads at most `n` pointer
records.

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
reports every run and fails above three times a budget. The owner times the
M1 export locally with `--board`, since that data stays out of the repository.
The work-count tests are the hard gate.
