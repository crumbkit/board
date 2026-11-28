# Reading a checkout

This file holds detail for docs/architecture.md. It says what `board
--checkout` reads and what it refuses.

`--checkout` reads the mount matched as the directory's own repository,
linked worktrees included, at the commit HEAD names in the directory's
working tree, instead of the commit its pointer pins. It lasts one run and
rewrites no pointer, so the next run reads the pin again. Board reads that
commit, never uncommitted edits. Run from a board's own tree, such as a
clone of a board, it reads that tree through the known board that mounts
its repository, as from any mounted clone. Run from a board's own tree no
known board mounts, from a clone matched only by origin or a repo record,
from a checkout whose HEAD names no commit, or with `--personal`, `init`,
`answer`, `serve`, `ui` or a process plugin's capability, it is refused with
`board::cli::usage`, and the hint says what to run instead. Only built-in
plugins run under it, because a process plugin's own `board` runs would read
the pins. Two pointers that mount the repository are refused with
`board::config::ambiguous`.

When two known boards mount one clone, Board does not choose between them,
because the two boards hold different rules. `--board <DIR>` names the
board to read through: Board then reads only that board, and the run keeps
the directory it started in. The directory must be a board's root, or the
run is refused with `board::config::not_found`. `--board` works with and
without `--checkout`, and conflicts with `--personal`. The run
writes nothing to the board it opened: every write, including one a
built-in plugin's call makes, is refused with `board::cli::usage` before it
takes the board's lock or uploads an attachment, and a nested call reads the
same checkout. A board a plugin opens elsewhere, such as the personal
board, is read at its pins.

A run without `--checkout` that finds its board through a mount of the
directory's own repository records that mount and the directory. Board
reads nothing more from the checkout until a built-in plugin asks.
`rules.check` asks when it reads that mount, or checks a path under
it. It compares the checkout's HEAD with the commit the pointer pins.
When they match, it also asks whether a tracked file differs from HEAD.
When either differs, its output carries `checkout`, which names both
commits and the `board --checkout` command that measures HEAD.
