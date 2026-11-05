# Updating a plugin

This file holds detail for docs/plugins.md, Installing and enabling. It says
what happens when an installed plugin stops shipping a type, how the
plugin moves its records to the types it ships now, and how a board moves
to the shared shapes.

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
- For a Python plugin, the `board_plugin.migrate` module in
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

## The shared shapes migration

Before 0.1.0, Board moved actors, hashes and subjects onto shared shapes in place,
keeping each type's version. `cargo xtask migrate-shapes <board>` rewrites
one board's own tree to them. A board's records and its stored schemas move
together, so update a board in this order:

1. Install the new Board.
2. Run `cargo xtask migrate-shapes <board>`, read what it could not map,
   and write a `--map <file>` that maps each such old string to an actor
   object or a subject. It refuses a map value outside those shapes. An
   actor name it cannot parse becomes the actor's `agent`, so it drops no
   attribution, and a suite's own name leaves its subjects.
3. Run it again with `--write` and the map. A second run changes nothing.
4. Run `board plugin enable` for each enabled plugin, so the stored schemas
   are the new ones, then `board check`.

It reads only the board's own tree, so each mounted board and each mounted
notes repository is migrated on its own, with
`cargo xtask migrate-shapes <its checkout>`. Until it is, a task there whose
`owner` is still a string is unreadable and drops out of `tasks.available`,
and a question there whose `session` is still a string drops out of
`plans.open`. A crossing with a bare hash is degraded until its board is
migrated.

The migration leaves seen records and Board.app alone. A seen record keeps
naming its effect by `/effects/answered/<id>`, which Board.app reads, and a
reply may still name its effect by the bare id, which Board.app's relay
writes. The migration gives a bare reply effect its `sha256:` prefix, and
both forms stay valid, so Board.app and Board can be updated in either
order.
