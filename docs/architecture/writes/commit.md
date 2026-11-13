# Committing a put

A put commits after its files are written. It takes git's `index.lock` when
it stages the commit, polling every 20 milliseconds for the wait that
`board.lock` had. A lock still held then refuses with `board::put::locked`
before any file is written. The two waits are separate, so a put that waits
the full time for both refuses after twice that time.

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
