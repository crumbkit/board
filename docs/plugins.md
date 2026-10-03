# Plugins

This is the v0 design. PLAN.md says which parts are built.

A plugin teaches Board a higher-level construct, such as tasks, a reading list
or a changelog. It is a separate program that Board starts, talking JSON over
stdin and stdout, with its own disposable SQLite database. Board stays small
because the constructs live here.

## Installing and enabling

A plugin is a directory on this machine, at
`$XDG_DATA_HOME/board/plugins/<name>/` (by default
`~/.local/share/board/plugins/<name>/`), holding `plugin.json` and its code. The
directory may be a symlink, so a plugin can be installed from a checkout. A
board enables plugins by name in `.board/board.json`; a name that is not a plugin
name, or one listed twice, makes it `board::config::invalid`. Board never loads a plugin
from a board's tree, so cloning or mounting a board never runs code.

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
  "capabilities": [
    {
      "name": "due",
      "description": "Open tasks due before a date.",
      "input_schema": { "type": "object" },
      "output_schema": { "type": "object" },
      "metadata": {}
    }
  ]
}
```

Plugin and capability names are lowercase ASCII letters, digits and hyphens.
`metadata` is any JSON object, which `board plugin find` can match. `extract`
lists the record types the plugin reads from the index (see Extraction). The
manifest refuses unknown fields. Its `name` equals its directory's name, so two
installed plugins cannot share one. `version` is `MAJOR.MINOR.PATCH` in digits.
The schemas are compiled under the same rules as a mounted board's. A manifest
that breaks any of these is `board::plugin::bad_manifest`.

## Finding plugins

`board plugin list` prints every installed plugin and its capabilities, and
whether the current board enables it. A broken install is listed with
`board::plugin::bad_manifest`, and a name the board enables that is not
installed with `board::plugin::not_installed`; either makes `list` exit 1. A
plugins directory that cannot be listed refuses `list` and `find` with
`board::plugin::not_installed`, naming the directory.
`board plugin find [--capability <name>] [--meta <key>=<value>]…`, with at
least one filter, lists the capabilities of working installs that match every
filter. A `--meta` filter
matches a string value in the capability's metadata, or in the plugin's when
the capability's lacks the key. Finding is a filter, never a ranking.

Only enabled plugins can be called: naming one that is installed but not
enabled is `board::plugin::not_enabled`. `board call --capability <name>` calls
the one enabled plugin that offers a
capability, so a plugin can use another by ability without knowing its name.
When more than one offers it, the call is refused with
`board::plugin::ambiguous`, naming each; the caller then names one. A broken
install offers nothing. A capability no installed plugin offers is
`board::plugin::not_installed`.

## Calling a capability

`board call <plugin>.<capability>` (or `--capability <name>`) reads input JSON
on stdin and:

1. validates it against `input_schema`;
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
5. validates `output` against `output_schema` and prints it: as JSON in text
   mode, and wrapped in `board/call-report/v1` `{plugin, capability, output}`
   with `--json`.

A plugin exits 0 with a result and 1 with an error. Its stderr is for logs:
Board keeps the last 64 KiB and shows it when the call fails. A call has 30
seconds and 16 MiB of stdout; past either, Board stops the plugin and refuses
with `board::plugin::timeout` or `board::plugin::too_large`. A plugin's error
codes are `<plugin>::<area>::<reason>`, so its failures read like Board's. Board
reports the plugin's code as its own, with the plugin's fields and `plugin`
added. How Board reads a plugin's exit and stdout:

- exit 0 with `board/call-result/v1`: the output, checked against
  `output_schema`; an output that fails it is `board::plugin::bad_output`;
- exit 1 with `board/error/v1` whose code starts with the plugin's own name:
  that code;
- exit 0 with anything else, or exit 1 with anything else on stdout (a result,
  another plugin's code, a malformed document, an unknown `$type`):
  `board::plugin::bad_output`;
- exit 1 with empty stdout, any other exit, or a signal:
  `board::plugin::failed`.

Past a limit, Board sends SIGTERM to the process it started, waits a second,
then kills it, and stops reading, so a grandchild still holding a pipe cannot
keep it waiting. Board signals only that process, so a `run` command must
`exec` its final program or forward SIGTERM. `uv run` forwards it. A shell
wrapper must use `exec`: `sh` without it leaves its child running.

A plugin calls another plugin only through `board call`, and reads Board's
`entries` only through `board query`, run as a child with its working directory
at `board.root`. It never opens Board's index. So the same checks, limits and
logs apply. Board increments `BOARD_CALL_DEPTH` on each nested call (unset
is 0) and refuses a call arriving at 8, or at a value that is not a whole
number, with `board::plugin::too_deep`.

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
replies `{"$type": "board/extract-result/v1", "blobs": […]}` naming the blobs
it handled; exit and stdout are read as for a call. Naming a blob it was not
sent is `board::plugin::bad_output`; a blob it does not name is sent again next
time. Blobs handled by batches before one that fails stay handled.

Board remembers in its index's `extracted` table which blobs each plugin
version has handled, sends only new ones, and forgets them when the plugin's
database file is missing. A
plugin may therefore receive a blob again, and replaces its rows for that blob.

Extraction runs in the refresh before `query`, for every enabled plugin, and
before `call`, for the plugin called. It never runs before `check`, so `check`
runs no plugin code. When a plugin fails, `query` still answers and lists it
under `plugins` in its report, with its code; `call` refuses with that code. A
plugin database SQLite cannot read is not attached, and `query` lists its
plugin with `board::plugin::failed` and runs.

Extraction must be a pure function of one record's bytes. Anything that spans
records, like "tasks blocked by an open task", is a query that joins the
plugin's tables with Board's `entries` view when it runs. That is what keeps a
plugin's database correct on every branch and every commit.

## Databases

Board gives each plugin version one SQLite file,
`plugins/<name>@<version>.sqlite` beside the index, and passes its path in every
request. The plugin owns everything inside it and
names its tables as it likes. `board query` attaches it read-only as `p_<name>`,
with hyphens in the name turned into underscores. A new version starts with an
empty file and extracts again. Deleting any plugin database is always safe. When
the index is in memory, each plugin database is a temporary file for that run;
when the index is kept but the database's directory cannot be made, the
database is a temporary file too, and `query` and `call` say so on stderr.
Board creates a missing database file empty, so `p_<name>` attaches for every
enabled plugin before its first extraction.

## Codes

`board::plugin::` `not_installed`, `not_enabled`, `bad_manifest`, `ambiguous`,
`bad_input`, `bad_output`, `failed`, `timeout`, `too_large`, `too_deep`.

## Writing a plugin in Python

Plugins are written in Python 3.12 or newer with uv, pydantic v2, ruff, pyright
in strict mode and pytest (STANDARDS.md P1 to P6). Each plugin talks the
protocol above directly and keeps that code in one module. Its manifest's
schemas are generated from its pydantic models. A shared `board-plugin` library
is extracted once two plugins exist and their common code is visible; until
then there is none.

Each plugin lives in `plugins/<name>/` in this repository, with its lockfile and
a committed `plugin.json` that a test compares with the generated one. Its
pytest suite drives the real `board` binary against fixture boards. Board's own
Rust tests use small shell-script plugins for the `board::plugin::` codes, so
the Rust gate does not need uv. CI runs each plugin's gates in its own job.

The one plugin so far is `tasks`: it extracts each `lr/task/v1` record's title,
status and due date into a `tasks` table keyed by blob (a record its board's
schema admits but the plugin cannot read gets no row, is still handled and is
logged), and its `due`
capability finds open tasks due before a date by running `board query`, which
joins that table with `entries`.
