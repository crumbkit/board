# Refusals before a sync changes the working tree

Integrate checks these three things, in this order, after the fetch and
before it takes the board lock or writes any working-tree file, index entry
or HEAD. The fetch has already written objects and `FETCH_HEAD` and moved
the remote-tracking ref. Each refusal leaves the working tree, index and
HEAD as they were and puts the remote-tracking ref back to the commit it
held before the fetch, when it held one. The fetched objects stay in the
object store.

| Code | When | Way out |
| --- | --- | --- |
| `board::sync::merging` | git reports a merge, rebase, cherry-pick or revert in progress (the same state a put refuses on; git's state misses a `sequencer/` directory left alone and shows a bisect ahead of a revert) | finish or abort the git operation (`git merge`, `git rebase`, `git cherry-pick` or `git revert`, with `--continue` or `--abort`) |
| `board::sync::shallow` | the clone has a `shallow` file, or git's `extensions.partialclone` or `remote.<name>.promisor` is set | `git fetch --unshallow`, or clone again in full |
| `board::sync::rewound` | the fetched commit does not contain the remote-tracking commit read just before the fetch, as after a force push or a branch reset | inspect the remote, then `board sync --accept-rewrite` only if this clone's commits should be pushed again |

Because the ref goes back, every later run refuses a rewritten remote too
until a person passes `--accept-rewrite`, which skips the rewound check and no
other. A clone with no tracking ref has nothing to compare and is not
refused; a tracking ref that exists but cannot be read is an error. A
previous commit the object store no longer holds counts as rewound. When the
ref cannot be put back (that commit is missing, or the ref changed during the
sync), the refusal text says so, and a plain rerun then compares the remote
with the fetched commit and will not refuse. The previous commit is read
again before each round of a sync, so a round that follows a rejected push
compares with the commit the earlier round fetched.

`--accept-rewrite` merges the rewritten branch with this clone's history,
so the clone's commits, including any the rewrite removed, are pushed again.
A rewrite made on purpose, such as removing a leaked secret, needs a fresh
clone instead, and `--accept-rewrite` must not be used on the old one. Keep
the old clone until `git log <remote>/<branch>..HEAD` in it shows no commit
you still need, since the fresh clone does not hold its unpushed commits.

A writer such as `notes.add` or a tasks writer treats `rewound` and `shallow`
as a sync it can write beside, like `board::sync::edited`: it writes the note or
task in the clone and leaves the sync for a person. It does not do so for
`merging`, because a put refuses during a git operation too, and nothing is
written. A note write fails with `notes::board::refused`, whose `code` field is
`board::sync::merging`. A task write goes on to the put, which refuses, so it
fails with `tasks::board::refused`, whose `code` field is `board::put::locked`.
Finish or abort the operation, then write again.

A call that only reads, such as `notes.daily`, goes on after `merging` as it
does after the other three: it returns the note as the clone holds it, with
`unsynced` set to `board::sync::merging`.
