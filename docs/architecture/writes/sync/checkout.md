# A checkout that a kill or a moved HEAD interrupted

A sync writes the files of its changes, then the index, then moves HEAD's
branch, so a kill in between leaves files that hold the new commit's blobs
under the old HEAD. When the remote changes one of those files before the next
sync, that merge wants a third blob, which would look like a person's edit.

Two records in the git directory prevent that. Neither is ever committed.

## The checkout record

`board-checkout` lists the checkouts that were moving the working tree, one
line each: the commit a checkout started from (`-` when it started with none)
and the commit it was moving to. A record written before this form holds the
target commit alone per line; it is read with an unknown start commit. Integrate writes it, by temporary file, fsync and rename, after it takes
`index.lock` and before it writes the first file, and removes it after the
branch moves. A refused branch move (`head_moved`) leaves it for the next
round.

The next sync reads it under the board lock, and:

- drops each commit the object store cannot read, and removes the record when
  none is left;
- removes the record when HEAD is at its last commit, which means that
  checkout moved the branch and was killed before it removed the record;
- takes each remaining target commit's files as a third input, at the paths
  where its start and target commits differ, which are the paths that
  checkout wrote. A file whose bytes are the target's blob at such a path
  counts as written by Board and is written or removed to match the new
  merge. At every other path the record counts for nothing, so a person's
  edit that puts a file back to the target's bytes, after a put or a `git
  commit` moved HEAD, still refuses with `board::sync::edited`. A record with
  an unknown start commit, or whose start commit the object store cannot
  read, accepts no blob, and the sync behaves as it did before records
  existed. Any other bytes that are neither the old blob nor the new one
  refuse with `board::sync::edited`;
- visits every path where a recorded start and target commit differ, or where
  a recorded target differs from HEAD, even when HEAD and the new merge agree
  there, so a file the killed checkout wrote is put back, and so is its index
  entry.

A second killed checkout adds its entry to the record, so the files the first
wrote are still known.

## The lock owner record

`board-locks` names the device, inode and birth time (with nanoseconds) of each lock file Board holds:
`board.lock`, `index.lock`, and the lock files of HEAD and its branch. The
entry is written right after Board takes the lock and removed when it is
released or committed.

A kill leaves the lock files, so Board must tell them from a lock that git or
another program holds. Every put and sync first takes a kernel lock on
`board.flock`, which the kernel drops when the process ends. Holding it shows
that no live Board process holds any of the lock files, so under it Board
removes each file the record names whose device, inode and birth time still
match, then the record. A file system can give a new file the inode number of
one just deleted, as ext4 and XFS do, but the new file has a new birth time.
Board never removes a lock file the record does not name, or names with
another inode or birth time, such as the `index.lock` of a running
`git commit` after a person removed the killed Board's lock. Where the
platform or file system gives no birth time, when the lock is recorded or when
it is cleared, Board keeps the file and drops the entry, and the put or sync
waits for the file and refuses with `board::put::locked` naming it. An entry
written before birth times were recorded holds none, so it is treated the same
way.

A process killed between taking a lock and writing its entry leaves a lock
with no entry, which still refuses with `board::put::locked`. The window is one
read of the record, and the create, write and rename of its temporary file,
with no fsync, since a killed process leaves the page cache. The window for
`board.lock` blocks every put and sync until a person removes the file.

Board removes an entry as soon as its lock file is renamed into place or
committed, before the next write point, so a kill after the index rename or
the branch move leaves no entry naming a live file. Between the rename and the
removal, a few system calls, a kill still leaves one. The remaining assumption
is a file system that gives a birth time with nanosecond resolution that
differs between a file and the file created at its path afterwards.
