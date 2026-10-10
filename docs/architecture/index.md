# The index

This file holds detail for docs/architecture.md. It says where the index
lives, what it stores, how a refresh decides what to redo, and the commands
that read it.

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
| `trees` | every entry in a tree: name, mode, object, logical segment and path code | tree |
| `worktree` | each working-tree entry's logical path, path code, blob and stat (size, times, inode, mode) | path |
| `blobs` | parse result in the role the path gives (`json`, `schema`, `reserved_schema`): kind, type, code, and the parser's or compiler's reason; a schema's code says whether it compiles | blob, role |
| `bodies` | the canonical form of each record that parses | blob |
| `validations` | schema result: code, failing JSON pointer, reason, failure count | record blob, schema blob |
| `refs` | references a valid record makes, in order: JSON pointer, target path, accepted types | record blob, schema blob, ordinal |
| `extracted` | the blobs each plugin version has handled; a query sees those that are valid records now | plugin, version, blob |
| `mounts` | each mount point and the commit it reaches, or its code | source, path |
| `state` | index format, and when the last refresh that hashed or recorded a file began | key |

The `entries` view joins these across the root and its mounts, one row per
entry: `source`, `path`, `logical` (the display path), `blob`, `size`, `kind`,
`type`, `status`, `code`, `change`, `role` and `schema` (the schema blob a
record was checked against). The `records` view is its rows of kind `record`,
with `source`, `path`, `logical`, `blob`, `type`, `status`, `code` and `body`;
`body` is NULL when the record does not parse. Tables, views and columns are an
interface agents write SQL against; changing one changes the index format. A
commit mounted in several places appears once, with `logical` through the
shortest display path, then the first lexically.

## Refresh

Each command refreshes the index first, doing work in proportion to what
changed: it hashes only working-tree files whose stat changed, as git does, and
parses and validates only blobs it has not seen. A file it could not read is
tried again by every refresh, because whether it can be read may change while
its stat does not. A file is hashed again if its mtime falls in or after the
second in which the last refresh that hashed or recorded a file began. A
refresh that finds nothing changed, including a file it still cannot read,
writes nothing, so it neither waits for nor blocks another process. If another
process holds the lock or wrote first, the refresh runs again, waiting up to 5
seconds for the lock. If the wait runs out, it runs in an index in memory. A
JSON file of up to 16 MiB is parsed from the bytes hashed, so a cold refresh
opens each file once.
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
With `--missing`, a path that names nothing reports its deepest resolving
ancestor and, under `missing`, the physical path a file there would have, by
put's resolution; put may still refuse it, for instance through a mount, and
names a new record's file with `.json`. Beneath a non-JSON file, or naming an
existing entry by its `.json` file, the path stays `path::not_found`.
