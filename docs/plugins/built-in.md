# Built-in plugins

This file holds detail for docs/plugins.md. It says how a plugin compiled into
Board is called. No plugin is built in yet; a fixture in board-ops tests the
path.

## The traits

`board-ops` defines two public traits.

- `Plugin` (`Sync`):
  - `manifest()` returns the same `Manifest` that `plugin.json` parses into.
    `Manifest::built_in` reads one whose `run` is empty or left out;
    `plugin.json` still needs a `run`.
  - `schemas()` returns the schema shipped for each declared type. A declared
    type without a compiling schema is `board::plugin::bad_manifest`.
  - `call(host, capability, input)` returns the output, or a `PluginError`
    with `code`, `message` and `fields`, as `board/error/v1` holds them.
- `Host`, what Board offers during a call:
  - `query(sql, limit)` runs SQL as `board query` does. `Limit::rows(n)`
    takes 1 to 10,000 rows; `Limit::ALL` takes every row. There is no byte
    budget.
  - `settings()` gives `now`, the time the call started, and `day_start` and
    `caller`, from `BOARD_DAY_START` and `BOARD_CALLER_DIR`. A built-in plugin
    reads no environment.

## Registration

`Board::build_in` takes a static list of plugins. A built-in plugin is
offered before installed ones and is listed, found and enabled as an
installed one is; a board still enables it by name. An installed plugin with
the same name is `board::plugin::duplicate` for `call` and `plugin enable`,
and `plugin list` reports that code.

## Calling

`board call` chooses a plugin, checks the board's copies of its schemas,
and validates input and output the same way for both kinds. These refusals
have the same code for both: `too_deep`, `not_installed`, `not_enabled`,
`ambiguous`, `schema_missing`, `schema_differs`, `bad_manifest`, `bad_input`,
`bad_output` (an output its schema refuses, or an error code without the
plugin's name), and the plugin's own code.

A built-in plugin runs in Board's process, so it has no time or size limit
and never gives `not_started`, `failed`, `timeout` or `too_large`. Its
refusal carries an empty `stderr`. The `plugin.call` phase of the timing
log includes the queries it makes, and `started` counts no process.
