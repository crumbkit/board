# Plugin commands

`board <plugin> <capability> [flags]` runs what
`board call <plugin>.<capability> [flags]` runs. This page says how Board
reads the first word.

A first word is a built-in command first, then an enabled plugin's name, then
an alias, then a built-in plugin's name, which runs as `board call` so the
call reports why it cannot run. A plugin's name wins over another plugin's
alias. A plugin named like a built-in command runs only through `board call`,
so Board's generic hints, which may name any plugin, print that form. A
built-in command never reads a manifest: Board reads them only after clap
refuses the first word, or for help. `board plugin enable` refuses an alias
that is a built-in command, or another enabled plugin's name or alias, with
`board::plugin::alias_conflict`. It does not refuse a plugin whose name is an
enabled plugin's alias; that plugin's name then wins. Running an alias two
enabled plugins share is refused with the same code; the hint names the
`board call` line. The capability comes right after the plugin's word; a flag
in its place is a usage error.

## Renamed plugins

A manifest's `former_names` lists the names a plugin had before it was
renamed, such as `["controls"]`. A former name reaches the plugin in
`board call <former>.<capability>`, as the first word, and in a board's
`plugins`. The call runs and prints the warning `board::plugin::former_name`,
which names the new name. A manifest that lists its own name is
`board::plugin::bad_manifest`. A name that one plugin has and another lists
as a former name, or that two plugins list, is refused with
`board::plugin::ambiguous`, naming the plugins that list it first.

A manifest's `moved` lists prefixes the plugin's records moved between, such
as `[{"from": "/controls", "to": "/rules"}]`. Neither may be `/`, and neither
may lie beneath the other. A reference that names nothing is resolved again
under the other prefix, in either direction, at the root of the board and at
the root of each mounted board, so `/repos/x/controls/a` reaches
`/repos/x/rules/a`. The first miss stands when the other names nothing too.
A write never swaps a prefix: it lands where its path says. Segments a link's
target gives are not swapped. A plugin's own query matches globs, so
`Board::moved_glob` gives a glob under the other prefix, which the plugin
matches only when the first matches nothing. Board follows the moved prefixes
of the plugins built into it, and reads them the first time a resolution
names nothing.
