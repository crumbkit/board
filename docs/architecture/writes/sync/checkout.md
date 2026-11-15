# A checkout that a kill or a moved HEAD interrupted

A sync writes the files of its changes, then the index, then moves HEAD's
branch, so a kill in between leaves files that hold the new commit's blobs
under the old HEAD. When the remote changes one of those files before the next
sync, that merge wants a third blob, which would look like a person's edit.

Two records in the git directory prevent that. Neither is ever committed.

## The checkout record

`board-checkout` lists the commits that checkouts were moving to, one id per
line. Integrate writes it, by temporary file, fsync and rename, after it takes
`index.lock` and before it writes the first file, and removes it after the
branch moves. A refused branch move (`head_moved`) leaves it for the next
round.

The next sync reads it under the board lock, and:

- drops each commit the object store cannot read, and removes the record when
  none is left;
- removes the record when HEAD is at its last commit, which means that
  checkout moved the branch and was killed before it removed the record;
- takes each remaining commit's files as a third input. A file whose bytes are
  the blob of any of those commits at that path counts as written by Board and
  is written or removed to match the new merge. Any other bytes that are
  neither the old blob nor the new one still refuse with `board::sync::edited`;
- visits every path where a recorded commit differs from HEAD, even when HEAD
  and the new merge agree there, so a file the killed checkout wrote is put
  back, and so is its index entry.

A second killed checkout adds its commit to the record, so the files the first
wrote are still known.

## The lock owner record

`board-locks` names the device and inode of each lock file Board holds:
`board.lock`, `index.lock`, and the lock files of HEAD and its branch. The
entry is written right after Board takes the lock and removed when it is
released or committed.

A kill leaves the lock files, so Board must tell them from a lock that git or
another program holds. Every put and sync first takes a kernel lock on
`board.flock`, which the kernel drops when the process ends. Holding it shows
that no live Board process holds any of the lock files, so under it Board
removes each file the record names whose device and inode still match, then
the record. Board never removes a lock file the record does not name, or names
with another inode, such as the `index.lock` of a running `git commit`.

A process killed between taking a lock and writing its entry leaves a lock
with no entry, which still refuses with `board::put::locked`. The window is a
few system calls long. A lock file of git's that is replaced by another file
with the recorded inode, after a kill, would be removed too, which needs the
inode number to be reused before the next Board run.
