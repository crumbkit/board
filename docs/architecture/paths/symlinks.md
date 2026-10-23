# Symlinks

This file holds detail for docs/architecture/paths.md. It says how Board reads
a git symlink, in a board's own tree or in a mount's commit, and how a put
treats one. architecture/paths/writes.md says which other paths a put refuses.

## Reading through a symlink

A git symlink is a tree entry of mode `120000` whose blob holds the path it
names, its text. A symlink is addressed as a file at its own path, so
`schemas/s.json` is `/schemas/s`, and its role comes from that path. Beside a
directory of the same name it is a record with children, as a file is. When it
leads to a file, its entry carries that file's blob and size, and `get`,
`check`, `refs` and the index read that file's bytes. Resolving the symlink's
path gives that file's blob, and the location keeps the symlink's path.

Board follows the text from the symlink's own directory, one name at a time,
as POSIX does: `..` leaves the directory reached so far, and a symlink met on
the way, a directory symlink included, is replaced by its own text. Board reads
only the tree. The working tree and a commit therefore give the same answer,
and a mount's commit reads the same on every machine. A walk remembers where
each symlink it followed leads, and so does a resolver of many paths in one
commit or one working-tree snapshot, which `get`, `refs` and the check of
references and Markdown links use. Many symlinks into one chain therefore
follow the chain once.

Reading a symlink's file in the working tree follows the symlink through a
snapshot of the working tree. A command makes a fixed number of snapshots,
however many symlinks it reads. Each snapshot lists a directory at most once
and follows a chain once. This holds for `get`, `refs`, `ls`, `check` (the
index refresh, the mount pointers, references and Markdown links), finding
the mounts that hold a directory, the check `put` makes after it writes, and
the schemas that `put`, `type add` and `plugin enable` read. Tests bound the
directories listed by `get` of many symlinks, and by a cold `check`, a warm
`check` and the mount lookup with many symlinked mount pointers or many
symlinks the index must parse again. Another test bounds the steps that
reading many symlinks through one chain takes. A symlink retargeted during a
command reads as the snapshot first found it if the snapshot has already
followed it. A read that meets its changed text is refused with
`board::repo::unreadable`.

Each relative Markdown link in a file read through a symlink resolves from
the symlink's directory, because the entry has the symlink's path. The same
link in the file it leads to resolves from that file's directory.

A symlink that leads to a directory is not followed. Its entry is a `file`
that holds its text, and resolution does not continue through it. When its
name ends in `.json`, its text is read as JSON. A path beneath it names
nothing, and Board never opens a working-tree file through a symlinked
directory.

Board checks each directory above a working-tree file, and the file itself,
with `lstat` and then opens the file, and the open follows symlinks. A
process that replaces a directory on the way, or the file itself, with a
symlink between the check and the open can make Board read a file outside
the tree. Board does not guard against this,
because a process that can rewrite the working tree while a command runs can
read those files itself.

A symlink that leads to no file is degraded, and resolving it is refused, with
one of these codes. The hint names the symlink's text, which an error carries
as `target`. A text longer than 100 characters is shown as its first 100
characters and its length, and one longer than 4,096 bytes as that limit
alone, without being loaded, so a long text cannot make a report large. A
control character, a Unicode format character such as U+202E, a line or
paragraph separator (U+2028, U+2029) or a backslash in the text is shown
escaped, as `\n` or `\u{202e}`, so a text cannot forge a line, a terminal
escape or a reordering of Board's output.

| Code | When |
| --- | --- |
| `board::symlink::outside` | The text starts with `/`, or a `..` leaves the repository's root. An absolute text names a file on the machine that reads the board, so the same commit could read differently on another machine. Board refuses it even when it names a file inside the repository. |
| `board::symlink::loop` | The chain passes through more than `MAX_SYMLINKS` symlinks, 32, the fewest any supported system follows (macOS), so a working-tree read through the file system reaches every file Board admits. A cycle ends here. |
| `board::symlink::dangling` | Nothing the board holds is at the text, or the text continues through a file. A file git ignores, one under `.git` and a submodule are not held, so a symlink to or into one dangles. A text that is empty, longer than 4,096 bytes or not UTF-8 dangles too, which bounds the work one symlink can ask for. |
| `board::repo::unreadable` | The file the text leads to is in the tree but cannot be read. |

A symlink is uncommitted when its own text differs from the one at `HEAD`. An
edit to the file it leads to shows on that file.

## Writes

Put checks the disk as well as the entries, because the listing hides ignored
files. A put never writes through or over a symlink: when the file it writes,
or a directory on the way to it, is a symlink on disk, ignored or not, it is
refused with `board::put::symlink`, which names the symlink. A put through a
symlink that leads to no file is refused the same way. A put reads a record's
schema through a symlink as `check` does, so the two agree on it, and a put
through a mount reads the mounted board's schemas the same way.

`type add`, a batch's schema line and `plugin enable` each compare their
schema with the one the board holds through a symlink. They write nothing for
an equal one, so a symlinked schema that is uncommitted stays uncommitted and
the rest of a batch is written as usual. A different one is
`board::schema::differs` for `type add` and a batch, and `plugin enable`
refuses to replace it with `board::put::symlink`. A symlink that leads to no
schema is refused with its symlink code.
