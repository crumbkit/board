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
`branch.<name>.merge`, and pushes what the clone committed. It works in
rounds of three phases; only the second takes the board's lock. `sync.lock` in the git directory
admits one sync per clone: a second exits at once with `board::sync::busy`.

1. Fetch the branch into its remote-tracking ref by running the `git`
   executable, so git's remotes, credentials and SSH settings apply, with
   `fetch.fsckObjects` on.
2. Integrate, under the board lock. Merge the fetched commit into HEAD
   from their common ancestor, as [merging.md](writes/merging.md) says: a
   merge commit has HEAD as its first parent and the fetched commit as its
   second, so every commit made here keeps its id, and each conflict is
   recorded in that commit. Then move HEAD, the index and the
   working tree to the result, writing each file as a put writes, the files
   first, then the index, then HEAD. A file whose
   working-tree bytes are neither its old blob nor its new one is an edit;
   the sync stops before it writes, with `board::sync::edited`. A
   symlink on disk at or above a file stops it the same way, with
   `board::put::symlink`. Each file is compared again just before its
   write; one edited since keeps its edit and is listed in `kept` with
   `board::sync::kept`. The index takes the new blob; a file the remote
   removed is left untracked. Committing the edit undoes the remote's
   change. An edit after the second compare is overwritten. If another
   program moves HEAD, the next round fetches again. A sync killed
   before HEAD moves is repaired by the next one ([merging.md](writes/merging.md)).
3. Push, unless `--no-push`, with `git push --atomic`, never forced. When
   the remote moved meanwhile, the push is rejected and the next round
   starts from the fetch. After three rounds the sync stops
   (`board::sync::gave_up`).

A branch that follows no remote branch is `board::sync::no_remote`, and a
fetch or push git refuses is `board::sync::failed`, with the end of git's
stderr. Neither git nor ssh may ask anything: git runs with
`GIT_TERMINAL_PROMPT=0`, and the owner's ssh command with `-o BatchMode=yes`
for OpenSSH, so a remote that needs a passphrase, host key or password fails
at once. A sync running after 60 seconds, counting every fetch, push, merge and
wait for the board lock, is `board::sync::timeout`. Board then sends git's group SIGTERM, on which git
removes its lock files, kills the group, ssh included, up to a second later,
and frees the board lock. On Ctrl-C, SIGTERM or SIGHUP, git's group gets
that signal, then SIGKILL once git exits or a second later, and Board frees
the board lock and stops. `sync.lock` is a kernel file lock, so SIGKILL frees it
too and a leftover file blocks nothing. When git exits, Board kills its group, so a helper
there, such as git-credential-cache's daemon, ends with it. The commits here
stay for the next sync. The report is
`board/sync-report/v2`: `remote`, `branch`, `fetched`, `old` (null before
the first commit), `new`, `merged`, `conflicts`, `pins`, `failing` (these four
are in [merging.md](writes/merging.md)), `pushed`, `attempts` and `kept` (each
kept file's `path`, `code` and `hint`).
