# Removing

`board rm <path>` removes one file, as `git rm` does. `board rm -r <path>`
removes every file beneath a directory.

- It takes the lock and waits as a put does, with `--wait`.
- The path names one file: a record, a file such as `/notes/plan.md`, or a
  link or mount record itself. A path through a link removes the file the
  link leads to.
- A directory is refused with `board::rm::directory`. A missing path is
  `board::path::not_found`.
- A directory left empty is removed, as `git rm` removes one.
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
## Recursive

- `-r` on a directory removes each file beneath it in one commit, with the
  message `board rm -r <path>`. On a file it removes that file.
- A link or mount record beneath is removed itself, never followed.
- A symlink beneath refuses the whole removal with `board::put::symlink`. A
  name beneath with no logical path refuses it with
  `board::path::malformed`.
- Each file passes the effects check as op `rm`.
- `-r` takes no `--expect`.

## Unmatched paths

- `--ignore-unmatch` turns `board::path::not_found` into success. Nothing is
  removed or committed, and the report lists the path in `absent`.
- Every other refusal stands: a stale `--expect`, a directory without `-r`,
  a path through a mount.

## Report

- The report is `board/put-report/v2`. Its `data` gains `removed`, each with
  the logical `path` and the `blob` removed. Text prints
  `removed <path> <blob>`. `absent`, when not empty, lists the paths
  `--ignore-unmatch` found absent; text prints `absent <path>`.

## In a batch

- A `put --batch` line `{"path": ..., "expect": ..., "remove": true}`
  removes its file, checked as `rm` checks it. `"ignore_unmatch": true`
  makes a missing path absent, as `--ignore-unmatch` does.
- A line that removes and also holds `record` or `text` is
  `board::put::invalid`. So is `ignore_unmatch` on a line that does not
  remove. So is a removal of a file another line writes.
- A removal line through a mount is refused. A batch through a mount holds
  no removal.
- Writes and removals share one commit. Its message counts each kind, such
  as `board put --batch (1 records, 1 removals)`.
