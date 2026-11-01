# Plugins

This is the v0 design. docs/architecture.md says what is built, and PLAN.md
says what comes next.

A plugin teaches Board a higher-level construct, such as tasks, a reading list
or a changelog. It is a separate program that Board starts, talking JSON over
stdin and stdout. It reads the board through `board query`. A plugin can also
be compiled into Board; [plugins/built-in.md](plugins/built-in.md) says how
it is called.

## Installing and enabling

A plugin is a directory on this machine, at
`$XDG_DATA_HOME/board/plugins/<name>/` (by default
`~/.local/share/board/plugins/<name>/`), holding `plugin.json` and its code. The
directory may be a symlink, so a plugin can be installed from a checkout. A
release ships each plugin here as `<name>-<version>.tar.gz`, holding that
directory with `board-plugin` vendored under `vendor/` and its lockfile; extract
it into `$XDG_DATA_HOME/board/plugins/` and run `uv sync --frozen --no-dev`
in `<name>/`. `bin/package-plugin` builds the archive and `bin/install-check`
installs it with a released `board` and runs init, enable, put and call. A
board enables plugins by name in `.board/board.json`; a name that is not a
plugin name, or one listed twice, makes it `board::config::invalid`. Board never
runs a plugin from a board's tree.

`board plugin enable <name>` makes one put, which `commit_on_put` commits:

- Each shipped schema goes to `.board/types/`. A different one there is
  replaced and listed in `replaced`; `board check` lists records it degrades.
- Each other schema under `.board/types/<name>/` is removed and listed in
  `removed`. While a record in the board's own tree has its type, it is kept
  and listed in `kept` instead.
- `.board/board.json` names the plugin.

An equal schema is left, so enabling again writes nothing.
[plugins/lifecycle/updating.md](plugins/lifecycle/updating.md) says how a
plugin update proceeds and how a plugin migrates its records.

## The manifest

`plugin.json` is `board/plugin-manifest/v1`:

```json
{
  "$type": "board/plugin-manifest/v1",
  "name": "tasks",
  "version": "0.1.0",
  "description": "Open tasks to sweep, and the calls that change them.",
  "run": ["uv", "run", "--frozen", "python", "-m", "board_tasks"],
  "metadata": { "area": "planning" },
  "types": ["tasks/task/v2"],
  "capabilities": [
    {
      "name": "available",
      "title": "Available tasks",
      "description": "Open tasks available now, due soon or stale.",
      "inputSchema": { "type": "object" },
      "outputSchema": { "type": "object" },
      "annotations": { "readOnlyHint": true },
      "_meta": {}
    }
  ]
}
```

A plugin that ships a `SKILL.md` gives its frontmatter `description` the same
text as the manifest's `description`, the first thing an agent reads about the
plugin. That text says what the plugin does, when to use it, the first command
to run with its input, and where to learn more.

Plugin and capability names are lowercase ASCII letters, digits and hyphens.
The plugin's `metadata` is any JSON object, which `board plugin find` can
match. Each capability is an MCP Tool object (MCP 2025-11-25): `description`,
`inputSchema` and `outputSchema` are required, and `title`, `annotations`,
`_meta`, `timeout` and `env` are optional. Each schema's root is `"type": "object"`. `annotations`
holds only MCP's five hint keys, which Board ignores. `_meta`
is any JSON object. `icons` and `execution` are refused. `types` lists the
types whose schemas the plugin ships, at `types/<namespace>/<kind>/v<n>.schema.json`
in its directory. Each type's namespace is the plugin's name, never
`board`. The manifest refuses
unknown fields. An installed plugin's `name` equals its directory's name, and
ships each type's schema; `check` and `put` check only the content.
`version` is `MAJOR.MINOR.PATCH` in digits. The schemas compile under a mounted
board's rules. Breaking any of these is `board::plugin::bad_manifest`.

## Finding plugins

`board plugin list` prints every installed plugin, its capabilities, and
whether the board enables it. It exits 1 when an install is broken, an
enabled plugin is not installed, or an enabled plugin's schema is missing or
differs. `board plugin find` lists the capabilities of
working installs, filtered by capability name or by a `_meta` value.
[plugins/finding.md](plugins/finding.md) gives their columns, filters and
exit codes.

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

`board call <plugin>.<capability>` takes each top-level string, integer,
number or boolean property of the capability's `inputSchema` as a flag of the
same name, such as `--path /docs`, `--open true` or `--offset -1`, and
`board call <plugin>.<capability> -h` lists them. Board reads input JSON on
stdin only when no flag is given and the schema admits a property that is not
a flag, such as an array. A schema also admits properties it does not name,
which no flag can give, unless it sets `additionalProperties` to `false` and
has no `patternProperties`. A capability whose schema admits only flag
properties never reads stdin, so a pipe nobody closes does not hold it up.
Neither does a target nobody offers, which Board refuses whatever the input.
`board call --capability <name>` takes no flags and always reads stdin.
A flag whose property has `"format": "board-path"` may be given relative to
the run's prefix, such as `--path docs` from inside a mounted repository;
Board sends the absolute logical path (architecture.md, Typed references).
No flags and nothing on stdin, a terminal on stdin, or stdin left unread, is
the input `{}`. An
input property named like one of `board call`'s own flags (`--json`, `--fields`,
`--long`, `--full`, `--capability` or `--help`), or not made of lowercase
letters, digits, `-` and `_`, is not a flag and goes on stdin. Whitespace alone
on stdin is no input.
Then Board:

1. checks that the board holds each schema the plugin ships, as `plugin list`
   does (`schema_missing`, `schema_differs`), so a record Board admits is one
   the plugin's own schema admits;
2. validates it against `inputSchema` (`board::plugin::bad_input`);
3. starts `run` in the plugin's directory (a program whose name holds `/` is
   found from there, any other through `PATH`), with an environment holding only
   `BOARD_CALL_DEPTH` and `BOARD_PLUGIN` (the plugin's name, or the outer
   plugin's when Board already runs inside one); `PATH`, `HOME`, `LANG`, `TZ`,
   `XDG_CACHE_HOME`, `XDG_DATA_HOME`, `XDG_CONFIG_HOME`, `BOARD_TIMINGS`,
   `BOARD_CALLER_DIR` and `BOARD_DAY_START` when set; and those its
   capability's `env` names;
4. writes one request to the plugin's stdin:

   ```json
   {
     "$type": "board/call/v1",
     "capability": "available",
     "input": { "repo": "/" },
     "board": { "root": "/Users/me/board" }
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
shows its last 64 KiB when the call fails. A call has 30 seconds, or its capability's `timeout`, and 16 MiB of
stdout; past either, Board stops the plugin with `board::plugin::timeout` or
`board::plugin::too_large`. A `run` command must `exec` its final program or
forward SIGTERM. [plugins/processes.md](plugins/processes.md) has the detail.

A plugin calls another plugin only through `board call`, and reads Board's
`entries` only through `board query`, run as a child with its working directory
at `board.root`. It never opens Board's index. Board increments `BOARD_CALL_DEPTH` on each nested call (unset is
0) and refuses a call arriving at 8, or at a value that is not a whole number,
with `board::plugin::too_deep`.

## Codes

`board::plugin::` `not_installed`, `not_enabled`, `bad_manifest`,
`schema_missing`, `schema_differs`, `ambiguous`, `duplicate`,
`bad_input`, `bad_output`, `failed`, `not_started`, `timeout`,
`too_large`, `too_deep`.

## Writing a plugin in Python

The plugins here are written in Python through the shared library in
`python/board-plugin/`. There are seven: `tasks`, `runs`, `share`, `notes`,
`changes`, `review` and `prompts`. `controls`, `plans`, `requests` and
`evals` are built into Board ([plugins/built-in.md](plugins/built-in.md)). The README in each `plugins/<name>/` describes its records and
capabilities. [plugins/share.md](plugins/share.md) explains the design of
`share`, [plugins/controls-claude-code.md](plugins/controls-claude-code.md)
the controls mod, and [plugins/plans-claude-code.md](plugins/plans-claude-code.md)
the plans mod.
[plugins/python.md](plugins/python.md) says what Board validates for a plugin,
what the plugin still checks, and how plugins are built and tested.
[plugins/claude-code.md](plugins/claude-code.md) covers Claude Code mods.
