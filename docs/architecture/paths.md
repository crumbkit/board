# Paths

This file holds detail for docs/architecture.md. It says how a logical path
names an entry, how resolution follows links and mounts, and which paths a
write refuses.

## Logical paths

A logical path is `/` alone, the board's root, or `/` followed by segments
separated by `/`. A segment is non-empty UTF-8 of at most 255 bytes, without
`/`, NUL or control characters, and is not `.` or `..`. A path is at most 4,096
bytes. A record's file name adds `.json`, so its segment is at most 250 bytes.
Paths are absolute and are read against the root of the board that holds them.
The working tree must also fit the OS's `PATH_MAX` (1,024 bytes on macOS, 4,096
on Linux) with its checkout prefix; Board keeps its own limit because paths
read through mounts never touch the file system.

In a directory, a segment `x` names one of:

- a record or pointer stored as `x.json`;
- a directory `x/`;
- a non-JSON file whose whole name is `x`, such as `notes.md`.

A `.json` file is addressed only without its extension, so every entry has one
address. A record `x.json` and a directory `x/` may sit side by side, so a
record can have children: `/x` names the record, `/x/y` continues into the
directory, and `ls /x` lists the directory. Any other match of one segment to
more than one entry, including a readable record of pointer type beside a
directory whether or not it is a valid pointer, degrades each with
`board::path::shadowed`, and resolution refuses with the same code. Two names in
one directory that differ only by ASCII case are degraded with
`board::path::case_collision`, because a macOS working tree cannot hold both;
resolution still matches names exactly. A name that is not a valid segment is
`board::path::malformed` and cannot be reached. A path that names nothing,
including one that continues through a file, is `board::path::not_found`. A
repository that cannot be opened, or a missing or corrupt object, is
`board::repo::unreadable`.

## Pointers: links and mounts

A pointer is a record of type `board/mount/v1` holding exactly one target:

```json
{ "$type": "board/mount/v1", "path": "/work/company-name/projects/x" }
```

```json
{
  "$type": "board/mount/v1",
  "repo": "~/src/company-board",
  "follow": "main",
  "commit": "3f9a…"
}
```

The first is a link to a path in the same board. The second is a mount of
another repository at an exact commit. `follow` names what `board mount update`
advances `commit` to; `commit` is what Board reads. v0 mounts local
repositories only; a URL is `board::mount::remote_unsupported`. A mount's
`repo` is a local path, read from `~/` or relative to the board, so on a
machine with no clone at that path the mount degrades to
`board::mount::repo_missing` until M8 (PLAN.md) adds remotes.

A pointer at `dir/name.json` makes `/dir/name` its point: resolution continues
through it. The semantics are POSIX symlinks':

- Reaching a link restarts at the link's `path` in the board that holds it,
  then continues with the remaining segments. Reaching a mount continues at the
  root of the mounted tree at `commit`.
- A resolution follows at most 40 pointers (`board::resolve::too_many_hops`).
  Returning to a board and path already visited is `board::resolve::cycle`.
- An absolute path in a mounted board means that board's path.
- A pointer whose target is missing is degraded with `board::ref::dangling`;
  nothing else breaks.
- The root board is read from its working tree, uncommitted edits included and
  marked. A
  mounted board is read from committed objects at `commit`, so a board at a
  given commit fully determines everything under its mounts.
- Mounting brings data and schemas, never code. A mounted board's plugin list
  is ignored.

Resolution returns a `Location`: the board (repository and source, either the
working tree or a commit), the physical path, the object (a blob hash, or a
directory), and every pointer followed. A Location's identity is its board,
source and physical path; two logical paths that reach one identity are one
object, reported once. The display path replaces every link with its target and
keeps mount points.

Resolving follows pointers. Walking (`ls`, the index) does not: a walk reports
a pointer as an entry, as `find` does without `-L`. The index includes every
board reachable through mounts, each commit indexed once however many places
mount it. Only `board-tree` reads pointers.

## Writes

`board put` resolves the path, following pointers, so a write through a link
lands at its target; put does not create a missing target, and refuses with its
resolution code. A path through a mount is `board::put::through_mount`. A
segment git keeps, `.git` under any spelling HFS+ or NTFS opens as it, is
`board::path::malformed`, in `mount update` too. Missing directories are
created. A write that would leave its own path degraded is refused with that
path's code, such as `path::shadowed`.
