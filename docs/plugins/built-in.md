# Built-in plugins

This file holds detail for docs/plugins.md. It says how a plugin compiled into
Board is called. `artifacts`, `attention`, `boundary`, `controls`, `plans`, `evals`, `runs`,
`github`, `jira`, `notes`, `params`, `prompts`, `review`, [`share`](share.md), `tasks`, [`topics`](built-in/topics.md), `changes` and
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
    with `code`, `message`, `fields` and `hint`, as `board/error/v1` holds
    them. The hint becomes the refusal's `hint`, as a process plugin's does.
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
    `put_bulk(lines, bulk)` writes them as `board put --batch --bulk`
    does, ending the commit's message in the `Board-Bulk:` trailer.
  - `call`, `board`, `decide`, `sync` and `mount_update` act as the
    plugin's own `board call`, `board --personal`, `board -C`, `board
    decide`, `board sync` and `board mount update` do
    ([built-in/elsewhere.md](built-in/elsewhere.md)).
  - `warn(refusal)` says a walk over many records passed over a path. Board
    prints each distinct warning on stderr after the call, whether it
    answers or is refused, and records it nowhere.
  - `settings()` gives `now`, the time the call started, `day_start` and
    `caller`, from `BOARD_DAY_START` and `BOARD_CALLER_DIR`, and
    `utc_offset`, the local clock's seconds east of UTC, which board-cli
    reads from `TZ` or the system with jiff. It also gives
    `environment`, the variables a process plugin offering the capability
    would see, its `env` among them, and `cache_home`, `XDG_CACHE_HOME` when
    it is absolute, else `~/.cache`. A built-in plugin reads no environment
    of its own.
  - `caller_remote()` reads the `origin` of the git repository holding
    `caller`, as `git remote get-url origin` there gives it, from git's
    config through board-tree, so no process starts. It is `Outside`
    without a caller or a repository, `Url`, or `Unread` with why the
    repository gave none.
  - `sign_order(order)` shows the owner an attention order's statement
    through attention-sign and returns their signature, or why there is
    none; `verify_order(order, signature)` returns `owner` or why the
    signature does not verify. Only Board's own host offers them; any other
    refuses (docs/architecture/writes/signing.md).
  - `run(process)` starts a program, such as git, gh or an agent;
    `run_hiding`, which implementors override, starts one without some
    variables; `places` names a directory's mounts; and `cached` and
    `cache` keep a small value by content hash
    ([built-in/running.md](built-in/running.md)).
- `board_ops::agent` starts a clean-context model agent, the argv a person
  names in a variable, through `Host::run` in an empty directory, and
  refuses each way it fails in the plugin's namespace, such as
  `controls::agent::timeout`.

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
limit and never gives `not_started`, `failed`, `timeout` or `too_large`.
Its capability's `timeout` is not enforced; each process it runs has its
own deadline. It must not panic: a panic ends Board's run instead of
refusing the call. Its refusal carries an empty `stderr`. The `plugin.call`
phase of the timing log includes the queries it makes and the processes it
runs, and `started` counts none of those processes.
