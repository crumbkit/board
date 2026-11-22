# The personal board

This file holds detail for docs/architecture.md. It says how the known
boards file names the personal board and how `board init --personal` sets it.

The known boards file, `$XDG_CONFIG_HOME/board/config.json`, may name the
user's personal board, as in
`{"boards": [...], "personal": "/Users/me/Code/board"}`, read against the
file's directory when relative. `--personal` runs as that board from its
root, from any directory: inside another board, in a repository no board
mounts, or outside git. Any `-C` is taken from that root. A file that names
no personal board, or names a directory that is not a board's root, refuses
`--personal` with `board::config::not_found`. The hint names the file and
`board init --personal`.

`board init --personal`, run in a git repository, first reads the file. It
refuses with `board::config::invalid` when the file is not a list of known
boards, and with `board::config::personal_elsewhere` when `personal` names
another directory; the hint says to edit `personal` by hand to move it.
Neither refusal writes anything. It then makes the repository a board as
`board init` does, and enables tasks, runs, evals, rules, plans and share in
that order, as `board plugin enable` does, one commit each. A plugin already
enabled with the same schemas writes nothing. Last, it sets `personal` to the
board's root and adds the root to `boards` unless `boards` lists it, keeping
every other key. It writes a temporary file beside the file and renames it
into place, making the directory when missing.

A failed enable leaves the earlier ones committed and the file unchanged, so
running the command again finishes the work. A second run on a named board
with the six plugins changes nothing. The report adds `enabled`, the plugins
this run enabled, and `personal`, the file and whether this run wrote it.
