# Walking mounts

This file holds detail for docs/architecture/paths.md. It says how the index
walks the boards reachable through mounts, and how it stops at a mount that
repeats a board.

The index walks each mounted commit once, however many places mount it. One
command opens each mounted repository once, however many mounts name it.

## Mount cycles

One resolution cannot cycle through mounts alone, because no commit holds its
own id. A walk can still repeat a board. Two boards that mount each other at
successive commits would be indexed once per level, at `/x`, `/b/x`,
`/b/a/b/x` and so on, until the commits run out.

So the index does not descend into a mount whose repository is the repository
of a board above it on its route. The root board counts as above every mount.
Two repositories are one when they share a git common directory, as linked
worktrees do, or when both have an `origin` with the same stored form, as
clones do. Board reads both facts from git; no record or `.board/board.json`
names a board.

Such a mount is degraded with `board::mount::cycle`. `board check` names the
board it repeats, and nothing beneath the mount is indexed. A mounted commit
is walked on its shortest route, so its mounts are judged against the boards
above it on that route.

A mount of the same repository beside another is walked. One repository
mounted twice at different commits is an example. A clone with no `origin`,
or a different one, is not seen as the same repository, so its loop is
unrolled as before and still ends.

`board get` of a path through such a mount still resolves it, since one
resolution ends.
