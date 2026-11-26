# A sync that skips a fresh fetch

`Board::sync_unless_fetched` is the sync a plugin runs when reading a
clone a moment old is good enough. tasks.available runs it on each notes
clone it reads, with a window of 60 seconds, so a sweep of several calls in
a row fetches once.

- With `push`, it is a plain sync.
- Without `push`, it runs no fetch when all of these hold:
  - `FETCH_HEAD` in the clone's git directory is not empty. git rewrites
    it only after a fetch has reached the remote, so a fetch that cannot
    reach the remote leaves the previous fetch's file and date, and one
    that fails after that leaves it empty, which never counts.
  - `FETCH_HEAD` was written less than the window ago, by this machine's
    clock. A time in the future does not count.
  - The remote-tracking commit is HEAD or an ancestor of HEAD, so the last
    fetch brought nothing left to integrate.
  - No git operation (a merge, rebase, cherry-pick or revert) is in
    progress, and the clone is neither shallow nor partial. A plain sync
    refuses those as [merging and shallow](refusals.md), so the skip leaves
    them to it.
- It then takes no lock, runs no git process and changes nothing. The
  report has HEAD as `old` and `new`, the remote-tracking commit as
  `fetched`, and `attempts` 0.
- Otherwise it is a plain sync.

A commit pushed elsewhere inside the window is read by the first call after
the window. A sync refused as [rewound](refusals.md) leaves `FETCH_HEAD`
written and puts the tracking ref back, so for up to the window a skip
returns HEAD without the refusal; the next plain sync refuses again.
