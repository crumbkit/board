# Architecture

This is the v0 design. Built so far: logical paths, resolution and walking,
links and mounts, records, schemas and references, the index, canonical form,
plugins, and the commands under Commands. PLAN.md says what
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
`board::config::invalid`. A dangling `board.json` symlink is
`board::symlink::dangling`.

When the search finds no board but the directory is in a git repository,
Board reads the known boards in `$XDG_CONFIG_HOME/board/config.json`
(`~/.config/board/config.json` by default), shaped
`{"boards": ["/Users/me/Code/board"]}`. A relative path is read against the
file's directory. A missing or empty file lists none, and any other file
that is not this shape is `board::config::invalid`. So is a `day_start`
that is not `HH:MM`. Board refreshes each
known board's index and reads the mounts in its working tree. A board
listed under several spellings counts once. A mount
matches when its repository has the same git common directory as the
directory's repository, so a linked worktree matches too. With exactly one
match, Board runs as that board, as git runs from a subdirectory. The prefix
is the mount's path joined with the directory's path beneath the
repository's root, such as `/repos/crumbkit/docs`, and the timing log
records it. A directory inside the git directory has no prefix and
matches nothing. No match is `board::config::not_found`. When the file lists
boards, the message says they were read and none mounts the repository, and
the hint says to add a mount; otherwise the hint says to list a board. More
than one match, counting every mount of every known board, is
`board::config::ambiguous`; `-C` then picks a board. A known board that
cannot be opened or refreshed is passed over, and a `not_found` message
names it. `-C` into a mounted repository finds its board the same way.

The same file may name the user's personal board, as in
`{"boards": [...], "personal": "/Users/me/Code/board"}`, read against the
file's directory when relative. `--personal` runs as that board from its
root, from any directory: inside another board, in a repository no board
mounts, or outside git. Any `-C` is taken from that root. A file
that names no personal board, or names a directory that is not a board's
root, refuses `--personal` with `board::config::not_found`, and the hint
names the file.

A path given to `get`, `ls`, `refs`, `check` or `put` that does not start
with `/` is relative to the prefix. In a board, the prefix is the directory's
path beneath the board's root, such as `/controls`, and the timing log
records it. `-C` sets the directory, as in git, so `board -C ~/Code/board ls .`
lists `/`. A run with no prefix reads from `/`. `.` and `./` are the prefix
itself, and `x` and `./x` are `x` beneath it. One trailing `/`, which a shell
adds to a directory, is dropped. The result must be a logical path, so any
other `.` or `..` segment is refused with `board::cli::usage`. The refusal
names the path as typed, and for a `..` that stays at or below `/` its hint
gives the absolute path. Board reads the prefix only for a relative path.
A directory whose name is not a logical path, such as one holding a tab,
refuses a relative path with `board::path::malformed`.
`mount update` takes only an absolute path. A run
from a mounted repository reads the mount's commit, never the files on disk.

Every file git sees
under the root is an entry: tracked files, and untracked files that are not
ignored. `.git` is never an entry. An entry has a path, a blob hash, a size, a
kind and a status.

| Kind | What it is |
| --- | --- |
| record | a `.json` file whose top-level object has a `$type` string, or [typed Markdown](architecture/records/frontmatter.md); `.board/board.json` is one |
| pointer | a record of type `board/mount/v1` (a link or a mount) |
| schema | a file under `.board/types/` ending `.schema.json` |
| json | any other `.json` file |
| file | anything else: Markdown, images, code |

An entry's status is `ok` or `degraded` with exactly one code (see Codes).
Board never refuses to index a file; the worst it says is a path and a code.
A file or directory that cannot be read is `board::repo::unreadable`. A
degraded directory passes its code to every entry beneath it. Working-tree
files are hashed as stored, with no clean filter, so a file under end-of-line
conversion or LFS shows as uncommitted. A symlink to a file in the repository
reads as that file (docs/architecture/paths/symlinks.md). A submodule is degraded with
`board::repo::submodule` and never descended.

`.board/board.json` is `board/config/v1`:

```json
{
  "$type": "board/config/v1",
  "commit_on_put": true,
  "plugins": ["tasks"],
  "store": {"backend": "directory", "location": "/Users/me/objects"}
}
```

`store`, when set, is where `put --file` sends a large or binary file, as
[architecture/attachments.md](architecture/attachments.md) says.

## Records

`$type` is `<namespace>/<kind>/v<n>`. Namespace and kind are lowercase ASCII
letters, digits and hyphens; `n` is a positive integer. The `board` and `ui`
namespaces are reserved for Board's own documents. A record's schema is
`.board/types/<namespace>/<kind>/v<n>.schema.json` in the same board as the
record, so a mounted board's records are checked against its own schemas.
Schemas are JSON Schema 2020-12. Board's own `board/*` types change in place
until a release or a second user, and after that a changed one is a new
version. A user's type changes when its owner adds the next version with
`board type add`, or edits its schema file by hand.

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
their schema. Board knows `board/config/v1`, `board/mount/v1`,
`board/plugin-manifest/v1`, `board/attachment/v1`, and the effects types
`board/effect/v1` and `board/seen/v1` wherever they are stored. A manifest is checked by its content alone; a
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

A file ending `.md` is Markdown, and each of its links that names a board path
is a reference. CommonMark decides what a link is: inline, reference and image
links count, even in a footnote, and text in code or HTML does not. An
autolink, a link with a URL scheme or starting `//`, and a fragment alone name
no board path. Each segment is percent-decoded, so `%2e%2e` is `..`. A
link is then read against the file's path as RFC 3986 5.2 reads a URL
path: from the root when it starts `/`, else from the file's directory, with
`.` and `..` removed and `..` stopping at the root. The query and fragment are
ignored, and a final `.json` is dropped, so `../tasks/a.json` reaches
`/tasks/a`. A link that is then not a logical path, or has a segment
decoding to `/`, names nothing. `refs` lists each link as written. A link to
nothing degrades the file with `ref::dangling`, as a reference does a record.

A capability's input schema marks a string property as a path with
`"format": "board-path"`. `board call` reads such a flag's value as a path
argument is read, relative to the prefix, before it validates the input; a
value that is not then a logical path is sent as given. JSON on stdin is
sent as given. The format means nothing anywhere else, such as in a record.

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

1. takes `board.lock` in the worktree's git directory, trying every 20
   milliseconds for 5 seconds, or for as long as `--wait` says (`500ms`,
   `30s`, `2m`, `1h`), and then refusing with `board::put::locked`. That wait
   is the only retry: concurrent puts each land in their own commit without
   the caller retrying. The steps below
   run under the lock, so a record is checked against the board it is written
   to, and other writers wait while it is checked;
2. checks it with the codes `check` uses: size and strict parse, `$type`,
   schema, validity, and that references parse. A reference need not resolve.
   Input that is not one JSON object with a `$type` is `board::put::invalid`.
   `board/mount/v1`, `board/config/v1` and `board/attachment/v1` are checked
   as their own types;
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

`board put <path> --file` writes stdin as a file, as given, at the path that
is the file's whole name, such as `/notes/plan.md`. It takes steps 1, 3, 4, 5
and 6, checks the size and frontmatter claiming a `$type`, and follows
Markdown links for the report. A path that names a directory,
or ends `.json`, which Board reads as a record, is `board::put::not_a_file`.
Bytes that are UTF-8 are text, and text holding NUL is `board::put::nul`,
whose hint gives the first NUL's byte offset; a file already in the tree that
holds NUL is read as before.
With a `store` in `.board/board.json`, a file over 1 MiB or not UTF-8 is
uploaded under its digest and a `board/attachment/v1` record is written in
its place, as [architecture/attachments.md](architecture/attachments.md)
says.

`board put --batch` reads at most 10,000 JSON Lines of `{path, expect,
record}`, `{path, expect, text}` or `{path, expect, remove}`, where `text` is written as a file, then
takes the lock with the same wait; when no line parses, it is refused without the lock. A line
holding both `record` and `text`, or neither and no `remove`, is `board::put::invalid`. A
record line at a schema's path, such as `/.board/types/lr/note/v1.schema`,
takes no expect and is checked first, as `type add` checks it; an equal schema
is committed if HEAD lacks it, unless a symlink holds it. Every line is checked against one snapshot of
the working tree, and every expect is compared before any write; two
lines resolving to one file, one invalid line or one stale expect refuse the
whole batch, listing each failing line with its code, as
[architecture/errors.md](architecture/errors.md) describes. Writes go in input
order, then one commit.

The report is `board/put-report/v2`, whose `data` holds `writes`, each with its Location, display
path, `previous` blob (null for a new file), `blob` and `degraded` (as `check`
reports it, or null); and `commit` (null when none was made).

`board type add <type>` reads a JSON Schema on stdin, compiles it, and writes
it in canonical form to the type's schema file, taking the lock and committing
as a put does. A type in the `board` namespace, or a schema that does not
compile, is `board::schema::invalid`, and a type with a part over 255 bytes is
`board::path::malformed`. An equal schema already there is left as it is, and
committed if HEAD lacks it, unless a symlink holds it
(architecture/paths/symlinks.md). A different one is `board::schema::differs`,
because a changed type is a new version, and one that is not JSON is refused
with its `json` code. The report is `board/put-report/v2`.

`board rm <path>` removes one file, or with `-r` every file beneath a
directory, and commits the removal, and a
`put --batch` line with `"remove": true` removes one in the batch's commit.
[architecture/writes/removing.md](architecture/writes/removing.md) holds the
rules.

[architecture/writes.md](architecture/writes.md) says how a put of one record
or a batch writes through a mount and moves the pin, how `board mount update`
moves a pin to the commit its `follow` ref names, and how `board sync`
replays this clone's commits onto the branch it follows.

## Commands

| Command | Does |
| --- | --- |
| `board init` | write a minimal `.board/board.json` at this git repository's root unless one exists; commit nothing |
| `board check [path]` | refresh, then report every entry; exit 1 if any is degraded |
| `board get <path>...` | resolve and print each file's stored bytes, up to 16 MiB each, all from one run; with `--fetch`, an attachment's bytes from the store; see below |
| `board ls <path>` | list a directory without following pointers, each entry with the status `check` gives it |
| `board refs <path>...` | references to and from each object, and values in each target, all from one check of the board; see docs/architecture/index.md |
| `board put <path>` | validate and write a record; with `--file`, write a file as given |
| `board rm <path>` | remove one file and commit the removal; see Writes |
| `board mount update <path>` | advance a mount to its `follow` |
| `board sync [--no-push]` | fetch the branch this board follows, replay its own commits on top, push, and move HEAD |
| `board query <sql>` | read-only SQL over the index |
| `board plugin list` / `find` | installed plugins, and every capability or those that match the filters given |
| `board plugin enable <name>` | write the plugin's schemas, then name it in `.board/board.json` |
| `board type add <type>` | compile the schema on stdin and write it to `.board/types/`; see Writes |
| `board call <plugin>.<capability>` | call a capability; see docs/plugins.md |
| `board decide --op <plugin>.<capability> --path <path> [--text\|--record <file>]` | ask the owner to admit a plugin's op at a path, bound to the content it will write; see docs/architecture/writes/effects.md |
| `board answer <id> <answer>` | the owner answers a plugin's pending effect; see docs/architecture/writes/effects.md |
| `board serve` | serve this board's pages to a browser on 127.0.0.1 |
| `board ui kinds` / `template` / `open` | the component kinds, a slot to fill, and a binding's page in a browser; see [architecture/ui/views.md](architecture/ui/views.md) |

[architecture/records/get.md](architecture/records/get.md) says how `board get`
prints several files, its report, and how it bounds its output.

Each command returns one typed report. `--json` prints it as crumbkit's
`board/<command>-report/v2`, or `board/failure/v1` on failure;
[architecture/errors.md](architecture/errors.md) gives both shapes. Exit codes: 0 done,
1 failed, 2 a refused command line (`board::cli::usage`).

`-C <dir>` before the command runs it as if Board started in `<dir>`. Several
combine as in git: a relative one is taken from the one before. A `<dir>` naming
no directory is `board::cli::usage`. `--personal` starts from the personal
board's root instead of the current directory.

Tab completion covers commands, flags and logical paths;
[architecture/paths/completion.md](architecture/paths/completion.md) says how.

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
| `board-ops` | every operation as a typed function returning its report; opening the board a directory is in, or the known board that mounts it; who asks for an effect; the host of built-in plugins | all of the above |
| `board-plugins` | the plugins built into the binary, each behind a feature | board-ops, each `board-plugin-<name>` |
| `board-plugin-plans` | the plans plugin, built in ([plugins/built-in.md](plugins/built-in.md)) | board-model, board-ops, board-plugin, schemars |
| `board-serve` | the local web server and the app it embeds ([architecture/ui/serve.md](architecture/ui/serve.md)) | board-index, board-model, board-ops, tiny_http |
| `board-cli` | the `board` binary: arguments (clap), configuration, the one printer | board-model, board-ops, board-plugin, board-plugins, board-serve, board-tree |

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

## Released interfaces

After a release, a changed type is a new version and the old version stays
readable, a `--json` report keeps every field it had, and a code is never
renamed or removed. `released.json` at the repository root lists what the
last release shipped, and a test refuses an in-place change to anything it
lists. [architecture/released.md](architecture/released.md) states the rule
and how a release writes the file.

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
