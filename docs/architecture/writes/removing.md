# Removing

`board rm <path>` removes one file, as `git rm` does.

- It takes the lock and waits as a put does, with `--wait`.
- The path names one file: a record, a file such as `/notes/plan.md`, or a
  link or mount record itself. A path through a link removes the file the
  link leads to.
- A directory is refused with `board::rm::directory`. A missing path is
  `board::path::not_found`.
- A path through a mount is refused with `board::put::through_mount`, whose
  `path` field names the mount. The mount record itself can be removed.
- A symlink is refused with `board::put::symlink`, as a put over one is.
- `--expect <blob>` refuses with `board::put::stale` unless the file holds
  that blob. `--expect none` is always stale, since the file is there.
- It removes a file that is untracked or edited on disk. It first writes
  the file into git's object store, so `git cat-file -p <blob>` restores
  the blob it reports.
- References to the removed file are not checked. `check` reports them as
  dangling.
- It passes the effects check as op `rm`; see
  [effects.md](effects.md).
- It commits as a put does, only the removed path, with the message
  `board rm <path>`.
- The report is `board/put-report/v2`. Its `data` gains `removed`, each with
  the logical `path` and the `blob` removed. Text prints
  `removed <path> <blob>`.

## In a batch

- A `put --batch` line `{"path": ..., "expect": ..., "remove": true}`
  removes its file, checked as `rm` checks it.
- A line that removes and also holds `record` or `text` is
  `board::put::invalid`. So is a removal of a file another line writes.
- A removal line through a mount is refused. A batch through a mount holds
  no removal.
- Writes and removals share one commit. Its message counts each kind, such
  as `board put --batch (1 records, 1 removals)`.
