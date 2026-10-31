# Writes to a path

This file holds detail for docs/architecture/paths.md. It says which paths a
write refuses.

`board put` resolves the path, following pointers, so a write through a link
lands at its target; put does not create a missing target, and refuses with its
resolution code. A path through a mount writes as architecture/writes.md says. A
segment git keeps, `.git` under any spelling HFS+ or NTFS opens as it, is
`board::path::malformed`, in `mount update` too. Missing directories are
created. A write that would leave its own path degraded is refused with the
code the walk would give it, such as `path::shadowed`. When that code comes
from a directory above, such as one whose name collides in case, the error
names the directory, says whether the write would create it, and names the
existing entry it collides with.

A put never writes through or over a symlink (architecture/paths/symlinks.md).
An ignored path is written like any other: put writes into an ignored
directory and commits the file. An ignored file where a directory must go
cannot be read as one, so the put is refused with `board::repo::unreadable`
before anything is written.

## New names

`board put --new <folder>/` names the record itself. The name is
`YYYYMMDDTHHMMZ-xxxx`: the put's UTC minute and four random lowercase hex
digits, such as `/projects/board/questions/20261030T1405Z-c2e1`. A record's
meaning goes in its `title` or its type's main text field, never in its name.
The put is otherwise `board put <folder>/<name> --expect none`, through links
and mounts too. When a file already holds the name, the put draws a new suffix,
up to three times, then refuses with `board::put::name_taken`. The suffix comes
from Rust's per-process random hash keys. It only has to differ between puts
in one minute; it is not meant to resist guessing.

The folder must end in `/`, so a mistyped full path is refused with
`board::put::not_a_folder` instead of becoming a folder. A folder relative to
the current directory works as other paths do, so `--new ./` names a record
here. `--new` takes no path, `--expect`, `--batch` or `--file`.

`board put --new` with no folder writes in the folder the record's type
declares in its schema's `x-board-folder` (architecture/records/shapes.md).
Each `--var <name>=<value>` fills the placeholder `{name}` with one path
segment, so `board put --new --var project=board` writes a
`plans/question/v1` under `/projects/board/questions/`. A later `--var` of
the same name wins. The put is refused with `board::put::no_folder` when the
type declares no folder, `board::put::folder_unfilled` when no `--var` fills a
placeholder, and `board::put::var_unused` when a `--var` names no placeholder,
or a folder is given. The folder is read from this board's schema, and a
record that no put admits keeps its own code. The plans types and
`runs/run/v1` declare their folders.

A `put --batch` line `{"new": "<folder>/", "record": {...}}` does the same,
with no `expect`. A batch line names its folder, since lines may write for
different projects. Two new lines in one batch draw different names. When the
only refusals are names already taken, the batch draws every new line's name
again, up to three times; then each such line is refused with
`board::put::name_taken` and the batch with `board::put::invalid`. The report's
`writes[].display`, and the text line `wrote <path> <blob>`, carry each name
chosen.
