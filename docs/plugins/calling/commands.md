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
