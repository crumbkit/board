# Writes through a mount

`board put <path>` and `board put --batch` can write through one mount. Such a put is made in the mounted board as an ordinary put, which
commits there, and then the mount's pointer is rewritten to pin that
commit. `put --file` refuses a path through a mount with
`board::put::through_mount`. A batch line writing in another board than its
first line's is `board::put::through_mount`.

The put takes the root's `board.lock`, resolves the path, and finds the mount
it passes and the file in the mounted tree. It works through links in
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

The mounted put takes the mounted board's own lock, waiting as long as it
waited for the root's (`board::put::locked`), and checks each record against the mounted board's
schemas, never the root's. Each file it writes must hold the same blob, or be
absent, at the pinned commit and at HEAD, and the working tree must hold that
blob or what is put. Otherwise the put is refused with
`board::mount::stale`, because committing would carry a change the root has
not seen. An expect is compared with the blob at the
pinned commit, which is what `board get` shows through the mount. A batch
lists each such line.

The mounted put commits only what it writes, with the message
`board put <path in the mounted board>`, or a batch's message. Then, under the root lock, the
pointer is rewritten with `--expect` set to its own blob, and committed when
the root sets `commit_on_put`, with the message `board put <path>`, or the
batch's.

The report is `board/put-report/v2`. The write's Location is in the new pinned
commit, `previous` is the blob at the old pin, and `degraded` resolves the
record's references in the mounted board at the new pin, as the root now sees
it. A reference to a file only in the mounted working tree is dangling.

If the mounted put fails, the pin does not move. Running a killed put again
finishes it: after the mounted files were written, it commits them; after the
mounted commit, it moves the pin with no new mounted commit; after the pin
moved, it commits the pin. A put that changes nothing makes no commit.

## Moving a pin

`board mount update <path>` takes the lock, then reads the mount pointer at
`path`, which it does not follow. Anything other than a mount pointer there is
`board::mount::invalid`. It reads the pointer's `follow` ref in the mounted
repository (missing: `board::mount::follow_missing`) and, if the commit
changed, rewrites `commit` through `put` with `--expect` set to the pointer's
blob. The report, `board/mount-update-report/v2`, gives the path, the old and
new commit and whether it wrote.

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
