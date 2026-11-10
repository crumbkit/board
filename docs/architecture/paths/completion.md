# Tab completion

This file holds detail for docs/architecture.md. It says how Board completes
a command line.

Tab completion covers commands, flags and logical paths, through
`clap_complete`'s dynamic engine. `BOARD_COMPLETE=<shell> board`, with no other
argument, prints a script for bash, zsh, fish, elvish or powershell; on each Tab
the script runs `BOARD_COMPLETE=<shell> board -- <words>`. Any other command
line runs as usual whatever `BOARD_COMPLETE` holds. `board completion install`
writes the script where the shell loads completions (fish's `completions`
directory, bash-completion's user directory, a directory on zsh's `fpath`) for
the shell `SHELL` or `--shell` names. `completion check` exits 1 when that file
is missing or differs, and `completion print` prints the script. They are
crumbkit's `completion` commands, and the script sets `BOARD_COMPLETE`.

`Board::complete` resolves the directory the typed path ends in, through links,
mounts, `--personal` and any `-C` (a leading `~/` is the home directory), then
`Repository::names` lists that one level without hashing, validating or
indexing anything: about 20 ms in a directory of 5,000 records. A directory
ends in `/`. A link or mount is offered without one, since a listing does not
follow pointers; type the `/` to continue through it. A typed word's quotes
and `\` escapes are removed first. fish completes a name with a space typed
with `\ ` or inside quotes. bash is offered names with `\` escapes, so a space
must be typed as `\ `: bash does not complete inside quotes, and it splits a
word at `:`. zsh is offered names with `:` and `\` escaped. A `-C` after the
cursor is ignored. Completion writes
nothing and prints no error into the shell.
