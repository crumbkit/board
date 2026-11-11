# Which mount a directory runs as

This file holds detail for docs/architecture.md. It says which mount Board
runs as when a directory outside every board is in a repository that known
boards mount more than once.

A mount holds the directory when its repository has the same git common
directory as the directory's repository, so a linked worktree is held too.
A mount also holds it when its repository has the same origin, so a
separate clone is held.

A mount beneath a record whose `phase` is `done` or `dropped` is left out.
A change's own mount, such as `/changes/fix-it/source`, sits beneath the
change record at `/changes/fix-it`, so the mount of a change that has ended
no longer holds any directory. A record that is missing, cannot be read or
has no `phase` leaves the mount in.

Board sorts the mounts that remain by one key and runs as the one that
sorts first. The key has two parts, compared in this order.

1. How the mount relates to the checkout:
   - first, a mount whose `follow` is the branch the checkout is on.
     `follow` may be the branch's short name or `refs/heads/<name>`. A
     checkout whose HEAD is detached or cannot be read follows no mount.
   - then, a standing mount, which sits beneath no record, such as
     `/repos/widgets`;
   - then, any other mount, such as a live change's own mount when the
     checkout is on another branch.
2. How the mount holds the directory:
   - first, a mount of the directory's own repository;
   - then, a mount of the same origin whose commit the clone holds;
   - then, any other mount of the same origin.

So a worktree on a change's branch runs as that change's mount, and another
checkout of the repository runs as a standing mount when there is one. The
first part is compared first, so a mount of a separate clone that follows
the checkout's branch sorts before a standing mount of the checkout's own
repository.

The same key picks within one board and across every known board. Two
mounts with the same smallest key are a real tie, such as two standing
mounts of one repository, and the run is refused with
`board::config::ambiguous`; `-C` then picks a board.

A repo record matches only when no mount holds the directory
(architecture/paths/repos.md).

The write guard of the changes plugin chooses no mount. It takes the mount
the brief's change names (plugins/changes/README.md, "The write guard").
