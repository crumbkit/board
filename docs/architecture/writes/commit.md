# Committing a put

A put commits after its files are written. It takes git's `index.lock` when
it stages the commit, polling every 20 milliseconds. `board.lock` and every
`index.lock` wait, including the waits for staging again, share one deadline
that starts when the put begins, so a put refuses by that wait and never later.
A lock still held then refuses with `board::put::locked`; the first staging
does so before any file is written.

Before it checks `--expect`, the put notes the HEAD it sees. If HEAD moved by
the time it moves HEAD itself, the put stages the same writes again on the new
HEAD and tries again, up to three more times. Before each try it compares,
for every path it wrote, the blob in the commit it is about to finish with the
blob in the HEAD it noted. A different blob means another program changed a
path the put overwrote, so the put refuses with `board::put::head_moved`.

The put also refuses with `board::put::head_moved` when HEAD still moves
after the third retry, and when the commit cannot be staged again, for example
because a merge began. In each case the files are already written and stay
uncommitted for the next sync to adopt.

To move HEAD, the put locks the branch and checks it still holds the commit
the put staged on. It then replaces git's index and only then moves the
branch, because git reads HEAD and then the index: a commit that sees the new
HEAD must also see the new index. A failure to write the index leaves HEAD
where it was, with `board::put::failed`.
