# Writes through a mount

`board put <path>`, for one record, can write through one mount. The record is
put in the mounted board as an ordinary put, which commits it there, and then
the mount's pointer is rewritten to pin that commit. `put --file` and
`put --batch` refuse a path through a mount with `board::put::through_mount`.

The put takes the root's `board.lock`, resolves the path, and finds the mount
it passes and the record file in the mounted tree. It works through links in
the root, and through links inside the mounted board. Then it refuses, with
`board::put::through_mount` and the mount's path unless another code is named,
when:

- the path passes a second mount;
- the mount names the root's own repository;
- the mounted repository has no working tree with a `.board/board.json`;
- the mounted board sets `commit_on_put` to false;
- `follow` names no ref (`board::mount::follow_missing`), or a ref other than a
  local branch, such as `origin/main`;
- HEAD is detached, or on a branch other than the one `follow` names;
- HEAD does not descend from the pinned commit
  (`board::mount::not_ancestor`). The check walks back at most 100,000
  commits; `board mount update <mount>` then pins HEAD.

The mounted put takes the mounted board's own lock, waiting up to 5 seconds
(`board::put::locked`), and checks the record against the mounted board's
schemas, never the root's. The record file must hold the same blob, or be
absent, at the pinned commit and at HEAD, and the working tree must hold that
blob or the record being put. Otherwise the put is refused with
`board::mount::stale`, because committing would carry a change the root has
not seen. `--expect` is compared with the blob at the
pinned commit, which is what `board get` shows through the mount.

The mounted put commits only the record, with the message
`board put <path in the mounted board>`. Then, under the root lock, the
pointer is rewritten with `--expect` set to its own blob, and committed when
the root sets `commit_on_put`, with the message `board put <path>`.

The report is `board/put-report/v2`. The write's Location is in the new pinned
commit, `previous` is the blob at the old pin, and `degraded` resolves the
record's references in the mounted board at the new pin, as the root now sees
it. A reference to a file only in the mounted working tree is dangling.

If the mounted put fails, the pin does not move. If a put is killed after it
writes the mounted file and before it commits, running the same put again
commits it. If it is killed after the mounted commit and before the pin moves,
running the same put again makes no mounted commit and moves the pin. If it
is killed after the pin moves and before the root commits it, running the same
put again commits the pin. A put that changes nothing makes no commit.

# Sync

`board sync` brings a clone of a board up to date with the remote branch
HEAD's branch follows, as git configures it in `branch.<name>.remote` and
`branch.<name>.merge`, and pushes what the clone committed. It takes the
board's lock, so no put runs meanwhile, and then:

1. fetches that branch into its remote-tracking ref by running the `git`
   executable, so git's remotes, credentials and SSH settings apply, with
   `fetch.fsckObjects` on;
2. replays each commit only this clone has, oldest first, onto the fetched
   commit, as a new commit with the same author and message, with the
   committer git is configured with. A commit whose every change the remote
   already holds makes no commit. A file that a replayed commit changed from
   one blob to another, when the remote holds a third, is refused with
   `board::sync::conflict`; Board never merges contents;
3. pushes the result with `git push --atomic`, and when the remote moved
   meanwhile fetches and replays again, up to three times
   (`board::sync::gave_up`). `--no-push` leaves the push for later;
4. moves HEAD, git's index and the working tree to the result, writing each
   file the remote changed, as a put writes. A file whose working-tree bytes
   are neither its old blob nor its new one is an edit, and the sync stops
   before it pushes or writes, with `board::sync::edited`.

A branch that follows no remote branch is `board::sync::no_remote`, and a
fetch or push git itself refuses is `board::sync::failed`, which carries the
end of git's stderr. The report is `board/sync-report/v2`: `remote`,
`branch`, `fetched`, `old` (null before the first commit), `new`,
`replayed` (each commit's `from` and its `to`, null when it made none),
`pushed` and `attempts`.
