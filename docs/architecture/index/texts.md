# The text index

This file holds the detail of the `texts` and `texts_fts` tables for
docs/architecture/index.md. `board search` reads them
(docs/commands/search.md).

`texts` keeps each Markdown file's and each untyped JSON file's first
heading and its prose after any frontmatter, cut at 64 KiB, once for each
blob. Each row has an integer `id`, its `INTEGER PRIMARY KEY`, so a full
`VACUUM` keeps it.

`texts_fts` is an FTS5 table with the trigram tokenizer over the prose of
`texts`, by `id`. It is an external-content table, so it keeps the trigrams
and no second copy of the prose. One trigger adds a text's trigrams when
the text is stored, and another deletes them when a prune deletes the text,
in the same transaction. A refresh therefore indexes only the texts it
stores.

A query sees `texts` filtered to the current tree, as every fact table.
`texts_fts` is not filtered, because `MATCH` needs the table itself. It
holds every text the cache keeps, so a query joins its `rowid` to the `id`
of the `texts` view to keep the current ones.

The index keeps the board's prose, so Board creates the board's cache
directory, `<root-hash>/`, with mode 0700, and the index file with mode
0600. SQLite gives the `-wal` and `-shm` journals the mode of the file. A
directory that is already there keeps its mode.

`board search` scans every entry but looks a text up only for a Markdown
file or an untyped JSON file, and a body only for a record. A word of three
or more characters matches a text through its set in `texts_fts`, so the
search reads no prose. Each connection maps up to 1 GiB of the index file
into memory, so a warm search reads pages with no system call each.
