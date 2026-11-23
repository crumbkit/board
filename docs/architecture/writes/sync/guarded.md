# Guarded sync

`Board::sync_guarded` is the sync a program runs for the owner, with nobody
watching. The autosync plugin runs it. It refuses more than `board sync`
and asks before each push.

- Before the fetch it refuses:
  - HEAD on another branch than the one named, with `board::sync::other_branch`.
  - A staged change, with `board::sync::staged`. Board compares the index
    with HEAD's tree.
  - A git operation in progress, with `board::sync::merging`.
- After the fetch, the three [refusals](refusals.md) apply. A rewritten
  remote is always refused, because a guarded sync never accepts a rewrite.
- Before HEAD moves it refuses a merge that would record anything, with
  `board::sync::conflicted` and the paths. That covers:
  - A file conflict.
  - A pin whose two sides are not in one line.
  - A record that fails validation.
- That refusal writes no conflict record and moves nothing. It puts the
  tracking ref back, as the other refusals do, and sets `unrestored` when it
  cannot.
- Before every push, including the push that starts a remote branch, the
  caller's `before_push` is asked with the fetched commit, or none, and the
  commit to push. `Hold` stops the sync with `board::sync::held`; the merge
  stays committed locally. The time `before_push` takes moves the sync's
  deadline later, so a slow check does not leave the push without time.
- After a rejected push the next round fetches again and asks again, about
  the new range.
- Uncommitted edits are left as a plain sync leaves them. Nothing is staged,
  stashed or pushed from the working tree.
