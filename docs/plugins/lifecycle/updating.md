# Updating a plugin

This file holds detail for docs/plugins.md, Installing and enabling. It says
what happens when an installed plugin stops shipping a type, and how the
plugin moves its records to the types it ships now.

## The update

- The owner installs the new version in the plugin's directory.
- Calls refuse with `schema_missing` until `board plugin enable <name>` runs
  again.
- Enable writes each schema the plugin ships. For each schema in its
  namespace that it no longer ships:
  - with no record of that type in the board's own tree, enable removes it
    and lists it in `removed`;
  - while such a record exists, enable keeps it and lists it in `kept`, with
    its type and the number of records. A record never loses its schema, so
    `put` and `check` still validate it.
- `call` checks only the schemas the plugin ships, so a kept schema never
  refuses a call, and the plugin can run to migrate.
- The plugin's migrate capability, or the owner by hand, moves the records
  to the new types.
- The next enable removes each kept schema that no record uses.

Each step leaves a valid board in its own commit.

## A migrate capability

- It is an ordinary capability, conventionally `migrate`, whose input has
  `apply`, false by default.
- For a plugin still in Python, the `board_plugin.migrate` module in
  `python/board-plugin/` does the work:
  - `olds(board, types)` reads the records of `types` in the board's own
    tree that parse, each as an `Old` with its logical `path`, `blob` and
    `record`.
  - `put(old, record)`, `move(old, to, text)` and `remove(old)` build
    `board put --batch` lines. Each line expects the blob read, so a file
    changed since the read refuses the batch with `board::put::stale`. A
    move writes `text` to a new file and removes the old one.
  - `migrate(board, lines, owed, apply=...)` returns `applied`, `commit`,
    the first 100 `lines` with `total` and `truncated`, and `owed`: what the
    owner must do afterwards, such as re-signing. Without `apply` it writes
    nothing. With it, one `board put --batch` writes every line in one
    commit.
- A built-in plugin cannot migrate until `Host` can write
  ([built-in/writing.md](../built-in/writing.md)).
- `board call` sets `BOARD_PLUGIN`, so the batch is checked as the plugin's
  own put ([architecture/writes/effects.md](../../architecture/writes/effects.md)).
- Board's wire format has no migration concept. What is owed is the plugin's
  own output.
