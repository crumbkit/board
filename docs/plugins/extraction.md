# Extraction and databases

This file holds detail for docs/plugins.md. It says how Board sends records to
a plugin that extracts them, and how it keeps each plugin's database.

## The protocol

A plugin that lists types in `extract` gets each record of those types once per
blob, if the blob validates against its schema through some route. Codes that
depend on the tree, such as `ref::dangling`, do not hold a record back. Board
sends batches, in display path order, of at most 1,000 records or 16 MiB:

```json
{
  "$type": "board/extract/v1",
  "records": [
    { "blob": "9c1e…", "path": "/tasks/write-plan.md", "type": "tasks/task/v2", "record": {} }
  ],
  "board": { "root": "…", "database": "…" }
}
```

Each batch is one run of the plugin. `path` is the entry's display path, so a
blob reached by several routes is sent once, and records in mounted boards are
included. The plugin writes what it needs into its database, keyed by blob, and
replies `{"$type": "board/extract-result/v1", "blobs": […]}` naming the blobs it
handled; exit and stdout are read as for a call. Naming a blob it was not sent
is `board::plugin::bad_output`; a blob it does not name is sent again next time.

Board records in `extracted` which blobs each plugin version has handled, sends
only new ones, and forgets them when the plugin's database is new. If Board
cannot read or make the database's id, it sends nothing that run. A plugin
replaces its rows for a blob it receives again. Extraction does not check that the board
holds the plugin's schemas; it sends only records the board's own copy admits.

Extraction runs in the refresh before `query`, for every enabled plugin, and at
no other time. `check` runs no plugin code, and `call` starts the plugin without
extracting first. A plugin that reads its rows through `board query` reads them
extracted from the tree that query refreshed. A plugin that opens its database
directly sees what the last `query` extracted, which can be older. When the
index is in memory, or opening it fell back, each run's database is a new
temporary file, so such a plugin sees no rows at all. When a
plugin fails, `query` still answers and lists it under `plugins` in its report's `summary`,
with its code.

Extraction must be a pure function of one record's bytes. Anything that spans
records, like "tasks blocked by an open task", is a query that joins the
plugin's tables with Board's `entries` view when it runs.

## Databases

Board gives each plugin version one SQLite file,
`plugins/<name>@<version>.sqlite` beside the index, and passes its path in
every request. Board puts the file in a temporary directory instead only when
it cannot make that `plugins` directory or the index is in memory. The plugin owns everything inside it but `board_database`, which
holds Board's id for the file. `board query` attaches it read-only as
`p_<name>`, with hyphens turned into underscores, from before the first
extraction. A new version starts with an empty file, extracts again, and
deletes the files of older versions. When Board cannot make the file, or
`board query` cannot read it, the plugin fails with
`board::plugin::no_database`. Deleting any plugin database is safe. A
plugin installed through a symlink keeps its rows when its code changes.
