# Built-in plugins

This file holds detail for docs/plugins.md. It says how a plugin compiled into
Board is called. `controls`, `plans`, `evals` and
[`requests`](built-in/requests.md) are built in.
[built-in/writing.md](built-in/writing.md) says how to write one.

- A built-in plugin is a crate, `board-plugin-<name>`, behind a Cargo feature
  of `board-plugins` that board-cli enables by default. It generates its
  capability and record schemas from Rust structs with schemars
  (docs/architecture/records/shapes.md).

## The traits

`board-ops` defines two public traits.

- `Plugin` (`Sync`):
  - `manifest()` returns the same `Manifest` that `plugin.json` parses into,
    with `run` empty or left out.
  - `schemas()` returns the schema shipped for each declared type. A declared
    type without a compiling schema is `board::plugin::bad_manifest`.
  - `call(host, capability, input)` returns the output, or a `PluginError`
    with `code`, `message` and `fields`, as `board/error/v1` holds them. A
    built-in plugin's refusal has no `hint`.
- `Host`, what Board offers during a call:
  - `query(sql, limit)` runs SQL as `board query` does, with the caller's
    index cache. `Limit::rows(n)` takes 1 to 10,000 rows; `Limit::ALL` every
    row. There is no byte budget.
  - `refs(paths, absent)` answers each path as `board refs` does, with no
    fields and no limit, and `get_each(paths)` reads each as `board get`
    does. Each resolves every path in one snapshot of the working tree and
    returns one answer per path, in order, so one path's refusal does not
    stop the others. Neither has a byte budget.
  - `put(lines)` writes the lines of one `board put --batch`, as JSON
    values, as that command does under `BOARD_PLUGIN`: the same checks,
    `expect`, refusals and report, one commit or nothing. It is checked as
    the plugin `BOARD_PLUGIN` names, else the called one, so it is refused
    what only the owner writes (docs/architecture/writes/effects.md).
  - `settings()` gives `now`, the time the call started, and `day_start` and
    `caller`, from `BOARD_DAY_START` and `BOARD_CALLER_DIR`. A built-in plugin
    reads no environment.

## Registration

`Board::build_in` takes a static list of plugins. A built-in plugin is
offered before installed ones and is listed, found and enabled as an
installed one is; a board still enables it by name. An installed plugin with
the same name is `board::plugin::duplicate` for `call` and `plugin enable`;
`plugin list` names its directory and `plugin find` leaves the plugin out.

## Calling

`board call` chooses a plugin, checks the board's copies of its schemas,
and validates input and output the same way for both kinds, with the same
codes. A built-in plugin runs in Board's process, so it has no time or size
limit and never gives `not_started`, `failed`, `timeout` or `too_large`. It
must not panic: a panic ends Board's run instead of refusing the call. Its
refusal carries an empty `stderr`. The `plugin.call` phase of the timing
log includes the queries it makes, and `started` counts no process.
