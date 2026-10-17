# Plugins

This is the v0 design. docs/architecture.md says what is built, and PLAN.md
says what comes next.

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
runs a plugin from a board's tree.

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
  "extract": ["tasks/task/v1"],
  "types": ["tasks/task/v1"],
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
lists the record types the plugin reads (see Extraction and databases). `types` lists the
types whose schemas the plugin ships, at `types/<namespace>/<kind>/v<n>.schema.json`
in its directory. Each type's namespace is the plugin's name, never
`board`. The manifest refuses
unknown fields. An installed plugin's `name` equals its directory's name, and
ships each type's schema; `check` and `put` check only the content.
`version` is `MAJOR.MINOR.PATCH` in digits. The schemas compile under a mounted
board's rules. Breaking any of these is `board::plugin::bad_manifest`.

## Finding plugins

`board plugin list` prints every installed plugin, its capabilities, and
whether the board enables it. It exits 1 on a broken install
(`board::plugin::bad_manifest`), an enabled name not installed
(`not_installed`), or an enabled plugin's schema that the board lacks
(`schema_missing`) or holds differently (`schema_differs`), naming the file.
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
offers, or the nearest offered name, and so do its `fields`
([architecture/errors.md](architecture/errors.md)).

## Calling a capability

`board call <plugin>.<capability>` (or `--capability <name>`) reads input JSON
on stdin and:

1. checks that the board holds each schema the plugin ships, as `plugin list`
   does (`schema_missing`, `schema_differs`), so a record Board admits is one
   the plugin's own schema admits;
2. validates it against `inputSchema` (`board::plugin::bad_input`);
3. starts `run` in the plugin's directory (a program whose name holds `/` is
   found from there, any other through `PATH`), with an environment holding only
   `PATH`, `HOME`, `LANG`, `BOARD_CALL_DEPTH`, and `XDG_CACHE_HOME`,
   `XDG_DATA_HOME` and `BOARD_TIMINGS` when set;
4. writes one request to the plugin's stdin:

   ```json
   {
     "$type": "board/call/v1",
     "capability": "due",
     "input": { "before": "2026-11-01" },
     "board": { "root": "/Users/me/board", "database": "/Users/me/.cache/board/…/tasks@0.1.0.sqlite" }
   }
   ```

5. reads exactly one document from the plugin's stdout: `board/call-result/v1`
   holding `output`, or `board/error/v1` holding `code`, `message` and typed
   `fields`;
6. validates `output` against `outputSchema` and prints it: as JSON in text
   mode, and with `--json` as `data` `{plugin, capability, output}` of a
   `board/call-report/v2`.

A plugin exits 0 with a result, or 1 with an error whose code is
`<plugin>::<area>::<reason>`, starting with its own name. Board reports that
code as its own, with the plugin's fields plus `plugin` and `stderr`, the end
of the plugin's stderr; Board adds no other key. Any other answer
on stdout is `board::plugin::bad_output`; a non-zero exit without an error
document, or a signal, is `board::plugin::failed`. A `run` program that
cannot be started is `board::plugin::not_started`. Stderr is for logs; Board
shows its last 64 KiB when the call fails. A call has 30 seconds and 16 MiB of
stdout; past either, Board stops the plugin with `board::plugin::timeout` or
`board::plugin::too_large`.

On Unix the plugin leads a new process group, which Board signals past a limit
or when Board itself is interrupted; a `run` command must `exec` its final
program or forward SIGTERM. [plugins/processes.md](plugins/processes.md) has
the detail.

A plugin calls another plugin only through `board call`, and reads Board's
`entries` only through `board query`, run as a child with its working directory
at `board.root`. It never opens Board's index. Board increments `BOARD_CALL_DEPTH` on each nested call (unset is
0) and refuses a call arriving at 8, or at a value that is not a whole number,
with `board::plugin::too_deep`.

## Extraction and databases

A plugin that lists types in `extract` gets each record of those types once per
blob, in batches, in the refresh before `query`, and writes what it needs into
its own SQLite database, which `board query` attaches as `p_<name>`. Extraction
must be a pure function of one record's bytes.
[plugins/extraction.md](plugins/extraction.md) has the protocol, when it runs,
and how Board keeps each database.

## Codes

`board::plugin::` `not_installed`, `not_enabled`, `bad_manifest`,
`schema_missing`, `schema_differs`, `ambiguous`,
`bad_input`, `bad_output`, `failed`, `no_database`, `not_started`, `timeout`,
`too_large`, `too_deep`.

## Writing a plugin in Python

The plugins here are written in Python through the shared library in
`python/board-plugin/`. There are three: `controls`, `tasks` and `evals`. The
README in each `plugins/<name>/` describes its records and capabilities.
[plugins/python.md](plugins/python.md) says what Board validates for a plugin,
what the plugin still checks, and how plugins are built and tested.
