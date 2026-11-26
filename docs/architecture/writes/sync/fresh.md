# A sync that skips a fresh fetch

`Board::sync_unless_fetched` is the sync a plugin runs when reading a
clone a moment old is good enough. tasks.available runs it on each notes
clone it reads, with a window of 60 seconds, so a sweep of several calls in
a row fetches once.

- With `push`, it is a plain sync.
- Without `push`, it runs no fetch when all of these hold:
  - `FETCH_HEAD` in the clone's git directory is not empty. git empties it
    when a fetch fails or is stopped, so a failed fetch never counts.
  - `FETCH_HEAD` was written less than the window ago, by this machine's
    clock. A time in the future does not count.
  - The remote-tracking commit is HEAD or an ancestor of HEAD, so the last
    fetch brought nothing left to integrate.
- It then takes no lock, runs no git process and changes nothing. The
  report has HEAD as `old` and `new`, the remote-tracking commit as
  `fetched`, and `attempts` 0.
- Otherwise it is a plain sync.

A commit pushed elsewhere inside the window is read by the first call after
the window. A sync refused as [rewound](refusals.md) leaves `FETCH_HEAD`
written and puts the tracking ref back, so for up to the window a skip
returns HEAD without the refusal; the next plain sync refuses again.
