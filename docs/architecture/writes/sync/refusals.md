# Refusals before a sync writes

Integrate checks these three things, in this order, after the fetch and
before it takes the board lock or writes any file, object, index entry or
HEAD. Each refusal leaves the working tree, index and HEAD as they were and
puts the remote-tracking ref back to the commit it held before the fetch, when it held one.

| Code | When | Way out |
| --- | --- | --- |
| `board::sync::merging` | `MERGE_HEAD`, `rebase-merge` or `rebase-apply` is in git's directory | finish or abort the git operation |
| `board::sync::shallow` | the clone has a `shallow` file, or git's `extensions.partialclone` or `remote.<name>.promisor` is set | `git fetch --unshallow`, or clone again in full |
| `board::sync::rewound` | the fetched commit does not contain the remote-tracking commit read just before the fetch, as after a force push or a branch reset | inspect the remote, then `board sync --accept-rewrite` |

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

A writer such as `notes.add` or a tasks writer treats all three as a sync it
can write beside, like `board::sync::edited`: it writes the note or task in the
clone and leaves the sync for a person.
