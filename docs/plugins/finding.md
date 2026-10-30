# Finding plugins

This file holds detail for docs/plugins.md. It says what `board plugin list`
and `board plugin find` print.

`board plugin list` prints every installed plugin, its capabilities, and
whether the board enables it. Its columns are `status`, `name`, `version`,
`enabled` and `offers`, the capability names. In text, `status` reads `ok` or
`degraded` and the problem's code; JSON holds that code in `code`. `--long` adds
`description`, the plugin's description from its manifest, and the problem
text: `message` for a refused manifest and `schemas` for schema problems. The table leaves out a column that is empty in every shown row. Text prints a line per plugin unless `--long`
or `--fields` asks for the table. It exits 1 on a broken install
(`board::plugin::bad_manifest`), an enabled name not installed
(`not_installed`), or an enabled plugin's schema that the board lacks
(`schema_missing`) or holds differently (`schema_differs`), naming the file,
or a built-in plugin also installed (`duplicate`), naming it in `shadows`.
`board plugin find [--capability <name>] [--meta <key>=<value>]…` lists the
capabilities of working plugins, a line each with its description. Each filter
narrows the list to the capabilities that match it; with none, it lists them all.
A `--meta` filter matches a string value in the capability's `_meta`, or in the
plugin's `metadata` when `_meta` lacks the key. Its columns are `plugin`,
`capability`, `enabled` and `description`, what the line shows. `--long` adds
`title`, `version`, `annotations`, `timeout` and `env`; the table leaves out a column that is
empty in every shown row. The schemas are too wide for a table, so
only `--fields` names `inputSchema`, `outputSchema` and `_meta`. `--json` prints
each capability as its Tool object, its name as `capability`, with `plugin`,
`version` and `enabled`.
