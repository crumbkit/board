# The index

This file holds the index detail for docs/architecture.md.

## Where it lives and what it holds

The index is SQLite under `$XDG_CACHE_HOME/board/<root-hash>/` (by default
`~/.cache/board/<root-hash>/`), where `<root-hash>` is the SHA-256 of the board
root's canonical path. The file is `index-<format>.sqlite`, so builds of
different index formats keep separate files. A file whose contents report
another format than its name, or that is corrupt, is deleted and rebuilt. A
build that creates an index file records the board root beside it in `root`. It
deletes the index files there more than one format older, and the cache
directory of each other board whose recorded root it can see is gone.

| Table | Holds | Keyed by |
| --- | --- | --- |
| `sources` | the root working tree and each mounted commit: its root tree and the repository it was first read from | source: `worktree` or a commit id |
| `trees` | every entry in a tree: name, mode, object, logical segment and path code; a name that is not UTF-8 is a blob of its bytes | tree, name |
| `worktree` | each working-tree entry's logical path, path code, blob and stat (size, times, inode, mode) | path |
| `blobs` | parse result in the role the path gives (`json`, `schema`, `reserved_schema`, `markdown`): kind, type, code, and the parser's or compiler's reason; a schema's code says whether it compiles | blob, role |
| `bodies` | the canonical form of each record that parses; a Markdown record's is its frontmatter | blob |
| `validations` | schema result: code, failing JSON pointer, reason, failure count | record blob, schema blob |
| `refs` | references a valid record makes, in order: JSON pointer, target path, accepted types | record blob, schema blob, ordinal |
| `links` | a Markdown file's links to board paths outside its frontmatter, in order, as written | blob, ordinal |
| `extracted` | the blobs each plugin version has handled; a query sees those that are valid records now | plugin, version, blob |
| `mounts` | each mount point and the commit it reaches, or its code | source, path |
| `state` | index format, and when the last refresh that hashed or recorded a file began | key |

The `entries` view joins these across the root and its mounts, one row per
entry: `source`, `path`, `logical` (the display path), `blob`, `size`, `kind`,
`type`, `status`, `code`, `change`, `role` and `schema` (the schema blob a
record was checked against). The `records` view is its rows of kind `record`,
with `source`, `path`, `logical`, `blob`, `type`, `status`, `code` and `body`;
`body` is NULL when the record does not parse. A `code` of `board::ref::dangling` or
`board::ref::wrong_type` marks a broken reference in a record its schema
admits; only the first marks a Markdown file that is not a record. Tables, views and columns are an
interface for agents' SQL; changing one changes the index format. A
commit mounted in several places appears once, with `logical` through the
shortest display path, then the first lexically.

## Refresh

Each command refreshes the index first, doing work in proportion to what
changed: it hashes only working-tree files whose stat changed, as git does,
parses each JSON or Markdown file on its hashing thread, and stores and
validates only blobs it has not seen. A file it could not read is
tried again by every refresh, since it may become readable while its stat
stays the same. A file is hashed again if its mtime falls in or after the
second in which the last refresh that hashed or recorded a file began. A
refresh that finds nothing changed, including a file it still cannot read,
writes nothing, so it neither waits for nor blocks another process. If another
process holds the lock or wrote first, the refresh runs again, waiting up to 5
seconds for the lock. If the wait runs out, it runs in an index in memory, so
the index lock never refuses a `query` or a `call`. A
JSON or Markdown file of up to 16 MiB is parsed from the bytes hashed, so a cold refresh
opens each file once. The walk lists directories and reads each entry's stat
on up to four threads, as many as there are cores, never listing an ignored
directory, then hashes the files whose stat the index does not hold on as
many new threads; the entries and the index are the same whatever the count.
A cold refresh compiles each schema once: the compilation that judges the
schema also validates the records of its type.
A file that is read again, such as a larger one, and that changed after hashing
is `board::repo::unreadable` until the next run, and so are the records of a
schema that changed. `check`, `refs`, `query` and `ls` use the index; `get`
resolves directly.

Once the index has doubled in pages since its last prune or first refresh, a
refresh that writes also prunes: it deletes the fact rows that neither the
working tree, `HEAD`'s tree nor a mounted commit reaches, and gives the freed
pages back. A query that another process prunes under runs again, at most
three times. Under a concurrent prune, one `board query` can skip extracting a
record whose file changed after its refresh; the next query sends it.

## Commands that read it

`board query '<sql>'` runs one read-only statement against the index. Each
enabled plugin's database is attached read-only as `p_<name>`. Rows return as
JSON, at most 100 unless `--limit` (at most 10,000) or `--all` says
otherwise, with `truncated` set when rows were left out. Rows also stop
before one that would take them past 64 MiB as printed, even with `--all`.
That cut adds the code `board::output::over_budget` to the report's `summary`
and still exits 0. A first row or a single value past 64 MiB is refused with the same
code and exits 1. Each value is counted as the row is read, so a wide row
stops at the value that passes the budget. The report is `rows`, each
row an array in column order, and a `summary` holding `columns` and
`plugins`, which lists any plugin whose extraction failed. SQL that does not parse, would write, or holds more than one statement
is `board::query::invalid`; ATTACH, DETACH and PRAGMA statements count as
writes, while table-valued pragma functions such as `pragma_table_info` are
admitted. A query still running after 1 s is `board::query::timeout`. A query
sees each fact table filtered to what the current tree reaches, so it answers
the same warm as cold; `main.<table>` is the whole cache, which keeps facts for
other branches.

`board refs <path>` reports each reference the entry makes, resolved now with
its status, and each entry whose references reach its identity, whatever path
they name.
With `--missing`, a path that names nothing reports its deepest resolving
ancestor and, under `missing`, the physical path a file there would have, by
put's resolution; put may still refuse it, for instance through a mount, and
names a new record's file with `.json`. Beneath a non-JSON file, or naming an
existing entry by its `.json` file, the path stays `path::not_found`.
Several paths are answered in the order given. All resolve in one snapshot
of the working tree, and the board is checked once, only when a path
resolves, so a plugin needs one run for many records.
Each answer is what the path alone gives, with the limit applied to each, so
a run prints at most the number of paths times the limit reference lines,
plus a note or error for each path, and the caller chooses how many paths to
ask. A refused path does not stop the others and makes the run exit 1. Text
prints each path's lines under a `==> <path> <==` header, as `get` does, and
an error names its path on a `key:` line. JSON is always one `board/refs-report/v2`,
for one path too. `data` holds each path's references in the order asked, and
`failures` each refusal with the `index` of its path. One run takes at most 10,000
paths; more is `board::cli::usage`. A path that does not parse as a logical
path, such as `tasks/a`, refuses the whole run as `board::cli::usage`. As `get`
does, `refs` answers each path only while its output, counted in the format
printed, is under 64 MiB, always the first, and names every later path in the
`left_out` of one `board::output::over_budget` failure; the library's `Board.refs` asks
for those again.

`--field <pointer>`, repeated, reads the target of each reference shown,
within the limit, where refs resolved it, through links and mounts. A
reference that resolves to nothing gets nothing. Each other `from` gains
`values`: in the order asked, each JSON pointer with its `value`, or with
`absent: true` when the record has nothing there. `target_code` is the code
check gives the target, or get's code when it is not a record; a target
that is not a record has no `values`. As text, the line gains
` target=<code>`, or ` target=-` for a healthy record, before the values;
each value is ` <pointer>=<value>`, the pointer a JSON string and the value
compact JSON or `absent`, so a pointer or value holding ` target=` cannot be
mistaken for the code. The check snapshot decides where a target is and its
code, and `refs` reads exactly the blob the walk listed and that code was
decided for. A target that resolved to another blob, or whose file changed
after the check, has `refs::target_changed` and no `values`, and the run counts it as `target_changed` in the timing log; run
`refs` again. `get` instead reads a file that changed during its run as it
is now, with the blob of the bytes it read. A pointer that is not RFC
6901 refuses the run as `board::cli::usage`. `tasks.next` asks for `/status`
and `/$type` this way to tell which waits are over.
