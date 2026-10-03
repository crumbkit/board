# Architecture

This is the v0 design. PLAN.md says which parts are built.

Board is a fast, validated view of a git tree. Git does history, branching,
merging and transport. Board indexes what is in a tree, validates the records
it understands, resolves references through links and mounts, and makes
records easy to read and write.

## Boards and entries

A board is a directory tree whose root holds `.board/board.json`. It is
normally the root of a git repository. Every file git sees under the root is an
entry: tracked files, and untracked files that are not ignored. `.git` is never
an entry. An entry has a path, a blob hash, a size, a kind and a status.

| Kind | What it is |
| --- | --- |
| record | a `.json` file whose top-level object has a `$type` string |
| pointer | a record of type `board/mount/v1` (a link or a mount) |
| schema | a file under `.board/types/` ending `.schema.json` |
| json | any other `.json` file |
| file | anything else: Markdown, images, code |

An entry's status is `ok` or `degraded` with exactly one code (see Codes).
Board never refuses to index a file; the worst it says is a path and a code.
An entry under a directory that resolution cannot reach (a shadowed or
malformed name) takes that directory's code.

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

Parsing is strict and bounded: UTF-8, at most 16 MiB, no duplicate keys at any
depth, and serde_json's nesting limit. A file that holds git conflict markers
gets its own code, so a bad merge is named as one.

Board's own documents (`board/*`) refuse unknown fields. User records follow
their schema: a schema that allows extra fields gets extra fields.

### Typed references

A schema marks a string as a reference with `"format": "board-ref"`, and may
name the types it accepts with `"x-board-type"` (a string or a list). A
reference is an absolute logical path. `put` refuses a reference that does not
parse. `check` reports, on the referring record, a reference that does not
resolve or resolves to the wrong type.

Schemas from a mounted board are untrusted input. Board validates with
`pattern` on a linear-time regex engine and never fetches a remote `$ref`.

### Canonical form

Every file Board writes is canonical: keys sorted by code point, two-space
indentation, one key or array element per line, `\n` endings, one trailing
newline. Golden bytes freeze this. Board reads any valid JSON; only `put`
rewrites a file into canonical form. One key per line is what lets git's line
merge combine most concurrent edits.

## Logical paths

A logical path is `/` alone, the board's root, or `/` followed by segments
separated by `/`. A segment is
non-empty UTF-8 of at most 255 bytes, without `/`, NUL or control characters,
and is not `.` or `..`. A path is at most 4,096 bytes. Paths are absolute and
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
`<root-hash>` is the SHA-256 of the board root's canonical path. It is
disposable: deleting it changes no answer, only speed.

| Table | Holds | Keyed by |
| --- | --- | --- |
| `sources` | the root working tree and each mounted repository and commit | source |
| `trees` | every entry in a source: path, blob, mode | source, path |
| `blobs` | parse result: kind, type, status, code | blob |
| `validations` | schema result: status, code, failing JSON pointer | record blob, schema blob |
| `refs` | references a record makes: JSON pointer, target path | blob |
| `mounts` | each mount point and the source it reaches | source, path |
| `state` | index format, root commit and working-tree stamp | key |

The `entries` view joins `trees`, `blobs` and `validations` across the root and
its mounts, with logical paths. Everything derived is keyed by content (blob or
tree hash) and never by commit or time, so branches share results and a fact is
true at a commit exactly when its blob is in that commit's tree.

Each command refreshes the index first: compare the root commit and working
tree with `state`; diff old and new trees; hash only the working-tree files
whose metadata changed; parse and validate only blobs not yet in `blobs` and
`validations`. Editing a schema invalidates exactly the validations that used
it, because they are keyed by its blob. A change to the index format deletes
the index and rebuilds it.

`board query '<sql>'` runs one read-only statement against the index (opened
with `query_only`). Each enabled plugin's database is attached read-only as
`p_<name>`. Rows return as JSON, at most 500 unless `--limit` (at most 10,000)
says otherwise, with `truncated` set when rows were left out. There is no query
language beyond SQL: an agent writes SQL.

## Writes

`board put <path>` reads one JSON record from stdin, then:

1. checks it as a record: `$type` present, schema found, schema valid,
   references parse;
2. resolves the path; a path through a mount is refused with
   `board::put::through_mount`, and a link writes to its target;
3. with `--expect <blob>` (or `--expect none` for a new file), refuses with
   `board::put::stale` unless the current blob matches;
4. writes canonical bytes to a temporary file and renames it into place;
5. commits, when `commit_on_put` is set.

Board holds `board.lock` in the repository's git directory, created
exclusively as git creates its own lock files, from step 3 to step 5. A lock left by a crash is refused with
`board::put::locked`, and the message says to remove it, as git does.

`board put --batch` reads JSON Lines of `{path, expect, record}`, checks every
record before writing any, and makes one commit. A crash after some renames
leaves those files as uncommitted edits, which the next `check` shows.

`board mount update <path>` reads `follow` in the mounted repository and
rewrites `commit` through `put`.

## Commands

| Command | Does |
| --- | --- |
| `board check [path]` | refresh, then report every entry; exit 1 if any is degraded |
| `board get <path>` | resolve and print a record with its Location |
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
| `board-plugin` | manifests, discovery, child processes | board-model, board-index |
| `board-cli` | the `board` binary: arguments, configuration, the one printer | all of the above |

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
| `json` | `parse`, `merge_conflict`, `duplicate_key`, `too_large`, `not_utf8` |
| `record` | `bad_type`, `unknown_type`, `invalid` |
| `schema` | `invalid` |
| `ref` | `malformed`, `dangling`, `wrong_type` |
| `path` | `malformed`, `shadowed`, `case_collision`, `not_found` |
| `repo` | `unreadable`, `submodule` |
| `mount` | `invalid`, `repo_missing`, `commit_missing`, `remote_unsupported` |
| `resolve` | `cycle`, `too_many_hops` |
| `put` | `stale`, `locked`, `through_mount`, `invalid` |
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

These budgets are first guesses, to be measured and set at milestone M4. The
reference boards are the exported old corpus (about 8,000 records) and a
generated 10,000-record board.

| Operation | Budget |
| --- | --- |
| `board get`, warm | 50 ms |
| `board check`, warm, nothing changed | 200 ms |
| simple `board query`, warm | 100 ms |
| full index, cold | 10 s |

The timing job reports every run and fails above three times a budget. The
work-count tests are the hard gate.
