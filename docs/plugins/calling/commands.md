# Plugin commands

`board <plugin> <capability> [flags]` runs what
`board call <plugin>.<capability> [flags]` runs. This page says how Board
reads the first word, and how it reads list and path flags.

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

## List and path flags

An input property that is a list of strings is a flag given once for each
item, in order: `board call tasks.add --title t --after /tasks/a.md --after
/tasks/b.md` sends `"after": ["/tasks/a.md", "/tasks/b.md"]`, and `--after
none` alone sends an empty list. A list of objects, such as `considered` on
`changes.plan`, has no flag. A call with a flag reads nothing on stdin, so
input that needs a list of objects goes on stdin whole, lists of strings
included.

A flag whose property has `"format": "board-path"` may be given relative to
the run's prefix, such as `--path docs` from inside a mounted repository;
Board sends the absolute logical path (architecture.md, Typed references).
So may each item of a list flag whose `items` have that format, and Board
refuses such an item that holds a comma, with a hint to repeat the flag, so
`--after /tasks/a.md,/tasks/b.md` is refused instead of sent as one path.

## Renamed plugins

A manifest's `former_names` lists the names a plugin had before it was
renamed, such as `["controls"]`. A former name reaches the plugin in
`board call <former>.<capability>`, as the first word, and in a board's
`plugins`. The call runs and prints the warning `board::plugin::former_name`,
which names the new name. A manifest that lists its own name is
`board::plugin::bad_manifest`. A name that one plugin has and another lists
as a former name, or that two plugins list, is refused with
`board::plugin::ambiguous`, naming the plugins that list it first.
`board plugin enable <former>` enables the plugin under the former name.
A built-in plugin may still ship types in a former name's namespace; an
installed plugin's manifest that does is `board::plugin::bad_manifest`. Its
codes start with its own name. Its `renamed_types` method pairs each old type with the type that replaces it;
a board that holds the schema of one type of a pair lacks no schema for the
other, so a call and `plugin list` pass before and after the board migrates.

A built-in plugin's `moved` method lists prefixes its records moved between,
such as `[("/controls", "/rules")]`. Neither may be `/`, and neither may lie
beneath the other; building in a plugin that breaks this panics. An installed
plugin's manifest that names `moved` is `board::plugin::bad_manifest`. Board
reads the pairs when the plugins are built into it. A record read by a path
that names nothing is resolved again under the other prefix, in either
direction, at the root of the board and at the root of each mounted board,
so `/repos/x/controls/a` reaches `/repos/x/rules/a`. The first miss stands
when the other names nothing too. A path under neither prefix is resolved
once, as before. Writes, removals, `resolve_pointer` (which stops at a
pointer at the end) and the resolution inside one mount's tree use the path
exactly as written, so a write lands where its path says. Segments a link's
target gives are not swapped.
