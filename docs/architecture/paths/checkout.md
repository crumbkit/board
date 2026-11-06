# Reading a checkout

This file holds detail for docs/architecture.md. It says what `board
--checkout` reads and what it refuses.

`--checkout` reads the mount matched as the directory's own repository,
linked worktrees included, at the commit HEAD names in the directory's
working tree, instead of the commit its pointer pins. It lasts one run and
rewrites no pointer, so the next run reads the pin again. Board reads that
commit, never uncommitted edits. Run from the board's own tree, from a clone
matched only by origin or a repo record, from a checkout whose HEAD names no
commit, or with `--personal`, `init`, `answer`, `serve`, `ui` or a process
plugin's capability, it is refused with `board::cli::usage`, and the hint
says what to run instead. Only built-in plugins run under it, because a
process plugin's own `board` runs would read the pins. Two pointers that
mount the repository are refused with `board::config::ambiguous`. The run
writes nothing to the board it opened: every write, including one a
built-in plugin's call makes, is refused with `board::cli::usage` before it
takes the board's lock or uploads an attachment, and a nested call reads the
same checkout. A board a plugin opens elsewhere, such as the personal
board, is read at its pins.
