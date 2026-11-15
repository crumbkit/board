# Reading at a commit, and history

This file holds detail for [get.md](get.md). It says how `board get --at`
reads a board at one commit and how `board log` lists the commits that
changed a path. Neither writes anything, takes a lock or reads the index.

## `get --at`

`--at <COMMIT>` reads every path in the tree of one commit of the board's own
repository. `<COMMIT>` is `HEAD` or a full commit id in lowercase hex, 40 or
64 digits, read as `--expect` reads a blob's. Any other value is
`board::cli::usage`, exit 2, so no value is ambiguous.

Paths resolve as they do beneath a mount: links are followed, and a mount is
read at the commit its pointer pins in that tree. An uncommitted edit is not
seen. A path that ends at a mount that cannot be read returns the mount's own
record, as `get` does in the working tree, and a path beneath it gets the
mount's code. As in the working tree, a path under a renamed plugin's new
prefix that names nothing is read again under the prefix it moved from, so
an old commit answers with the record it held under the old prefix.

The report is `board/get-report/v2`, with each file's `location.source` the
commit's id. The limits are `get`'s. A path that names nothing in that commit
is refused with `board::path::not_found`, and the other paths are answered.
An id that names no commit in the repository, names another kind of object,
or has a length the repository's hash does not use is
`board::commit::not_found` for every path; so is `HEAD` before the board's
first commit. The commit need not be reachable from HEAD, as for
`git show <commit>:<path>`.

`--checkout` changes only a pointer read in the working tree, so `get --at`
and `log` refuse it with `board::cli::usage`.

## `log`

`board log [PATH] [--at <COMMIT>] [--limit <N> | --all]` lists the commits
that changed what a path names, newest first, as `board/log-report/v2`. The
path, `/` when left out, resolves in the start commit, `--at` or HEAD, with
every pointer followed. A path that names nothing there is
`board::path::not_found`, and the hint names `--at`. Before the first commit
it is the same code, saying so.

The walk compares the git tree entry at the path's physical path, a file, a
directory or the root, between a commit and its parents. An entry changed
when its object id or its kind differs, or when only one side has it. Merges
follow git's default rule for a path: when a merge's entry equals a parent's,
the walk follows the first such parent only and does not list the merge;
when it differs from every parent, the merge is listed and every parent is
walked. A commit with no parent is listed when the entry exists.

The walk starts with a queue that holds the start commit. It takes the commit
with the newest committer time, with ties taken by the smaller id, and queues
the parents it follows. A commit is listed after the child that queued it,
even when clocks are skewed.

`log` follows the physical path resolved at the start commit. A symlink at
the path that leads to a file stands for that file, as it does for `get`, so
the walk follows the file's path and a row's `blob` is the blob `get`
reports. A link
retargeted since then can make `get --at <commit>` resolve to another file,
so a row's `blob` is the value to compare. Renames are not followed. A path
through a mount lists the mounted repository's commits from the commit the
mount pins, and `summary.location.repository` names that repository;
`get --at` does not take those ids. Such a log does not list the board
commits that moved the pin, even where a move changed what `get` returns for
the path. Where the mount's pinned commit cannot be
read, `log` of the mount's path or a path beneath it is refused with the
mount's code; it does not list the history of the pointer file.

`--limit` is 1 to 10,000 and defaults to 100; `--all` lifts it. The walk stops
as soon as it holds `limit` rows, so `--limit 1` on a file changed at the
start commit reads one commit. It reads at most 100,000 commits. It reads the
repository's shallow commits once and walks each as a commit with no parent.
So the oldest row at a shallow commit may not be a change: that commit is
listed whenever it holds the path, and `summary.complete` is false.
A missing or corrupt commit or tree is `board::repo::unreadable`.

Each row has `commit`, the full id; `time`, the committer time in UTC as RFC
3339 with seconds; `subject`, the first line of the message cut at 256 bytes
on a character boundary, with bytes that are not UTF-8 replaced; and `blob`,
the file at that commit, or `null` when the commit removed it or the path is
a directory there. Text prints the id, time and subject of a commit on one
line.

The report has no `total`, because counting would walk the whole history.
`truncated` is true when the walk stopped before the end of the history, so
older rows may exist: at the limit with commits still queued, at the bound,
and at a shallow commit. `summary` holds `at`, the start commit; `walked`, the
commits read; `complete`, false at the bound or a shallow commit, where a
larger limit cannot reach older commits; and the path's `location`.
`board log --limit 1 --json` therefore also reads the board's head commit.
