# Refusals before a sync writes

Integrate checks these three things, in this order, after the fetch and
before it takes the board lock or writes any file, object, index entry or
HEAD. Each refusal leaves the clone as it was.

| Code | When | Way out |
| --- | --- | --- |
| `board::sync::merging` | `MERGE_HEAD`, `rebase-merge` or `rebase-apply` is in git's directory | finish or abort the git operation |
| `board::sync::shallow` | the clone has a `shallow` file, or git's `extensions.partialclone` or `remote.<name>.promisor` is set | `git fetch --unshallow`, or clone again in full |
| `board::sync::rewound` | the fetched commit does not contain the remote-tracking commit read just before the fetch, as after a force push or a branch reset | inspect the remote, then `board sync --accept-rewrite` |

A rewound refusal puts the remote-tracking ref back to the commit it held
before the fetch, so every later run refuses too until a person passes
`--accept-rewrite`, which skips this one check and no other. A clone with no
tracking ref has nothing to compare and is not refused. A tracking commit the
object store no longer holds counts as rewound. The previous commit is read
again before each round of a sync, so a round that follows a rejected push
compares with the commit the earlier round fetched.

A writer such as `notes.add` or a tasks writer treats all three as a sync it
can write beside, like `board::sync::edited`: it writes the note or task in the
clone and leaves the sync for a person.
