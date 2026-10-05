# Plugins

This is the v0 design. PLAN.md says which parts are built.

A plugin teaches Board a higher-level construct, such as tasks, a reading list
or a changelog. It is a separate program that Board starts, talking JSON over
stdin and stdout, with its own disposable SQLite database.

## Installing and enabling

A plugin is a directory on this machine, at
`$XDG_DATA_HOME/board/plugins/<name>/` (by default
`~/.local/share/board/plugins/<name>/`), holding `plugin.json` and its code. The
directory may be a symlink, so a plugin can be installed from a checkout. A
board enables plugins by name in `.board/board.json`; a name that is not a
plugin name, or one listed twice, makes it `board::config::invalid`. Board never
loads a plugin from a board's tree, so cloning or mounting a board never runs
code.

`board plugin enable <name>` writes each schema an installed plugin ships to
`.board/types/`, then names the plugin in `.board/board.json`, as one put that
`commit_on_put` commits. A schema already there must be equal, or the enable is
`board::plugin::schema_differs` and writes nothing. Enabling again writes
nothing.

## The manifest

`plugin.json` is `board/plugin-manifest/v1`:

```json
{
  "$type": "board/plugin-manifest/v1",
  "name": "tasks",
  "version": "0.1.0",
  "description": "Open tasks and their due dates.",
  "run": ["uv", "run", "--frozen", "python", "-m", "board_tasks"],
  "metadata": { "area": "planning" },
  "extract": ["lr/task/v1"],
  "types": ["tasks/note/v1"],
  "capabilities": [
    {
      "name": "due",
      "title": "Due tasks",
      "description": "Open tasks due before a date.",
      "inputSchema": { "type": "object" },
      "outputSchema": { "type": "object" },
      "annotations": { "readOnlyHint": true },
      "_meta": {}
    }
  ]
}
```

Plugin and capability names are lowercase ASCII letters, digits and hyphens.
The plugin's `metadata` is any JSON object, which `board plugin find` can
match. Each capability is an MCP Tool object (MCP 2025-11-25): `description`,
`inputSchema` and `outputSchema` are required, and `title`, `annotations` and
`_meta` are optional. Each schema's root is `"type": "object"`. `annotations`
holds only MCP's five hint keys, which Board ignores. `_meta`
is any JSON object. `icons` and `execution` are refused. `extract`
lists the record types the plugin reads (see Extraction). `types` lists the
types whose schemas the plugin ships, at `types/<namespace>/<kind>/v<n>.schema.json`
in its directory. Each type's namespace is the plugin's name, never
`board`. The manifest refuses
unknown fields. An installed plugin's `name` equals its directory's name;
`check` and `put` check only the content. `version` is
`MAJOR.MINOR.PATCH` in digits. The schemas compile under a mounted board's
rules. A manifest that breaks any of these is `board::plugin::bad_manifest`.

## Finding plugins

`board plugin list` prints every installed plugin and its capabilities, and
whether the current board enables it. A broken install is listed with
`board::plugin::bad_manifest`, and an enabled name that is not installed with
`board::plugin::not_installed`; either makes `list` exit 1.
`board plugin find [--capability <name>] [--meta <key>=<value>]…`, with at least
one filter, lists the capabilities of working installs that match every filter.
A `--meta` filter matches a string value in the capability's `_meta`, or in the
plugin's `metadata` when `_meta` lacks the key. `--json` prints each capability
as its Tool object.

Only enabled plugins can be called: naming one that is installed but not
enabled is `board::plugin::not_enabled`. `board call --capability <name>` calls
the one enabled plugin that offers a capability, so a plugin can use another by
ability. When more than one offers it, the call is
`board::plugin::ambiguous`, naming each. A broken install offers nothing. A
plugin that is not installed, or a capability no installed plugin offers, is
`board::plugin::not_installed`; the message lists what the named plugin
offers, or the nearest offered name.

## Calling a capability

`board call <plugin>.<capability>` (or `--capability <name>`) reads input JSON
on stdin and:

1. validates it against `inputSchema` (`board::plugin::bad_input`);
2. starts `run` in the plugin's directory (a program whose name holds `/` is
   found from there, any other through `PATH`), with an environment holding only
   `PATH`, `HOME`, `LANG`, `BOARD_CALL_DEPTH`, and `XDG_CACHE_HOME` and
   `XDG_DATA_HOME` when set;
3. writes one request to the plugin's stdin:

   ```json
   {
     "$type": "board/call/v1",
     "capability": "due",
     "input": { "before": "2026-11-01" },
     "board": { "root": "/Users/me/board", "database": "/Users/me/.cache/board/…/tasks@0.1.0.sqlite" }
   }
   ```

4. reads exactly one document from the plugin's stdout: `board/call-result/v1`
   holding `output`, or `board/error/v1` holding `code`, `message` and typed
   `fields`;
5. validates `output` against `outputSchema` and prints it: as JSON in text
   mode, and wrapped in `board/call-report/v1` `{plugin, capability, output}`
   with `--json`.

A plugin exits 0 with a result, or 1 with an error whose code is
`<plugin>::<area>::<reason>`, starting with its own name. Board reports that
code as its own, with the plugin's fields and `plugin` added. Any other answer
on stdout is `board::plugin::bad_output`; a non-zero exit without an error
document, or a signal, is `board::plugin::failed`. Stderr is for logs; Board
shows its last 64 KiB when the call fails. A call has 30 seconds and 16 MiB of
stdout; past either, Board stops the plugin with `board::plugin::timeout` or
`board::plugin::too_large`.

Past a limit, Board sends SIGTERM to the process it started, waits a second,
then kills it, and stops reading, so a grandchild still holding a pipe cannot
keep it waiting. Board signals only that process, so a `run` command must
`exec` its final program or forward SIGTERM, as `uv run` does.

A plugin calls another plugin only through `board call`, and reads Board's
`entries` only through `board query`, run as a child with its working directory
at `board.root`. It never opens Board's index. Board increments `BOARD_CALL_DEPTH` on each nested call (unset is
0) and refuses a call arriving at 8, or at a value that is not a whole number,
with `board::plugin::too_deep`.

## Extraction

A plugin that lists types in `extract` gets each record of those types once per
blob, if the blob validates against its schema through some route. Codes that
depend on the tree, such as `ref::dangling`, do not hold a record back. Board
sends batches, in display path order, of at most 1,000 records or 16 MiB:

```json
{
  "$type": "board/extract/v1",
  "records": [
    { "blob": "9c1e…", "path": "/tasks/write-plan", "type": "lr/task/v1", "record": {} }
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

Board remembers in its index's `extracted` table which blobs each plugin
version has handled, sends only new ones, and forgets them when the plugin's
database file is missing. A plugin replaces its rows for a blob it receives
again.

Extraction runs in the refresh before `query`, for every enabled plugin, and
before `call`, for the plugin called. It never runs before `check`, so `check`
runs no plugin code. When a plugin fails, `query` still answers and lists it
under `plugins` in its report, with its code; `call` refuses with that code.

Extraction must be a pure function of one record's bytes. Anything that spans
records, like "tasks blocked by an open task", is a query that joins the
plugin's tables with Board's `entries` view when it runs.

## Databases

Board gives each plugin version one SQLite file,
`plugins/<name>@<version>.sqlite` beside the index, and passes its path in every
request. The plugin owns everything inside it.
`board query` attaches it read-only as `p_<name>`, with hyphens turned into
underscores, from before the first extraction. A new version starts with an
empty file and extracts again. Deleting any plugin database is always safe. A
plugin installed through a symlink keeps its rows after its code changes, until
its version changes or its database is deleted.

## Codes

`board::plugin::` `not_installed`, `not_enabled`, `bad_manifest`,
`schema_differs`, `ambiguous`,
`bad_input`, `bad_output`, `failed`, `timeout`, `too_large`, `too_deep`.

## Writing a plugin in Python

Plugins are written in Python 3.12 or newer with uv, pydantic v2, ruff, pyright
in strict mode and pytest (STANDARDS.md P1 to P6). Each plugin talks the
protocol above directly and keeps that code in one module. A shared
`board-plugin` library is extracted once two plugins exist; until then there is
none.

Each plugin lives in `plugins/<name>/` in this repository, with its lockfile and
a committed `plugin.json` that a test compares with the one its pydantic models
generate. Its pytest suite drives the real `board` binary against fixture
boards. CI runs each plugin's gates in its own job.

The one plugin so far is `tasks`: it extracts each `lr/task/v1` record's title,
status and due date into a `tasks` table keyed by blob, and its `due`
capability finds open tasks due before a date by running `board query`, which
joins that table with `entries`.
