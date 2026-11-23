---
name: share
description: 'Copy a record from this board into a mounted board, such as a team''s, with its children, the records and files it references or links to, the records that reference it and the schemas of their types, so a reader there can open it. Use it when someone else needs to read a record you wrote. Start with `board -C <board> share to --path /<record> --to /<mount>`, which lists what would be copied and prints the command that writes it. SKILL.md and README.md in the plugin''s directory say more.'
---

# share

Copy a record you wrote into a board other people read.

## Steps

1. Find the mount of the board to share into. `board -C <board> ls /` lists
   the board's entries; a mount is a `board/mount/v1` record such as
   `/team`. The mounted repository needs a checkout on the branch the mount
   follows, as any write through a mount does. Run
   `board -C <board> mount update /<mount>` first if the mount's repository
   has moved on since its pin.
2. Run `board -C <board> share to --path /<record> --to /<mount>`. This is
   the preview, and it writes nothing. A flag is named after the input
   property; `--dry-run` and `--allow_uncopied` are refused with
   `board::cli::usage`. The preview prints `copied`, the schemas the
   destination lacks in `types`, and:
   - `extras`: each record or file outside `/<record>` that goes with it,
     with `via` and `from`. `ref` is a typed reference, `link` a Markdown
     link, and `back_ref` a record whose typed reference names the shared
     record, an entry under it or a whole, such as a decision whose `refs`
     names the shared project.
   - `wholes`: each record the shared one is part of, such as a task's
     project named by its `parent`, with `back_refs`, how many decisions
     and questions it brought. A whole brings only those, and the share
     follows none of their references.
   - `not_copied`: each target the share does not copy, with its `reason`:
     `directory`, `mount`, `link`, `missing`, or `beyond` for an entry a
     whole's decision or question names that the set does not hold, or
     `withheld` for a back-ref whose type stays out of shares, such as
     the owner's priorities, which never needs `--allow-uncopied`.
   - `next`: the command that writes.

   Read `extras`, `wholes` and `not_copied` and agree to them before you go on, or
   change the records so the set holds what you mean to share. Board's checks
   for a write through a mount run only in step 4.
3. Fix what it refuses:
   - `share::path::not_found`: `--path` names nothing in this board's own
     tree. Check the path with `board -C <board> ls`; a record read through
     a mount cannot be shared from here.
   - `share::target::differs`: the destination holds a different file at a
     listed path. Share is a copy, never a merge. Rename your record, or ask
     the destination's owner which copy is right.
   - `share::closure::unresolved`: a record other than a back-ref references
     a path through a mount or link that the destination does not resolve
     into the same repository.
     For a mount, remove the reference or ask the destination's owner to
     mount the same repository at the same path. For a link inside this
     board, such as `/latest` linking to `/notes`, the reference can be
     shared only if the record names the target path, `/notes`, directly,
     or the destination mounts this board.
   - `share::mount::not_found`: `--to` is not the mount record of another
     repository. Name the mount itself, such as `/team`, not a path inside
     it, a mount inside the mounted board, or a link to it.
   - `share::attachment::local`: an attachment's bytes are only on this
     machine. Put them in a store every reader can reach, or pass
     `--local_attachments true` when every reader shares the location.
   - `share::file::unsupported`: a binary file or a JSON file without
     `$type` sits in the set. Put it in a store, or move it out.
   - `share::link::unsupported`: a link, such as a mount, sits in the set.
     Move it out of the set.
   - `share::record::invalid`: `board check` reports a listed record, and
     `board put` would refuse it. Fix the record first.
   - `share::closure::uncopied`: with `--yes`, `not_copied` lists what the
     share cannot copy. Change the record, or run the command
     `next` printed, which adds `--allow-uncopied` and shares without them.
4. Run the command `next` printed, which adds `--yes`. It writes the copies
   through the mount as one commit in the mounted repository and moves the mount's pin
   to it. It does not push. A refusal with `share::board::refused` carries
   Board's own code, and for a refused batch `refusals` names each path and
   its code: fix what it names and run the call again.
5. Run `board -C <repository> sync` to push, where `<repository>` is the
   `repository` the call printed. A reviewer opens the record at its path in
   that repository at the printed `commit`.
6. Give the reader the record's URI from `uris`, such as
   `board://acme/team-board/evals/x`. A reader whose known boards list a
   clone of the destination runs `board get <uri>` from any directory. When
   `uris` is empty, the destination has no origin; give its path instead.

A second call writes only what the destination still lacks, and leaves a path
it already holds equally as it is. A second call finishes a share killed
before the mounted repository committed, or after it committed and before
the pin moved. If someone changed a shared file in the mounted checkout in
between, Board refuses with `board::mount::stale` naming it; remove or
restore that file and run the call again. A kill can leave a `board.lock`
and git's `index.lock` in the git directories of both repositories, this
board's and the mounted one. The next put or sync in each clears the lock
files Board left, so run the call again. A kill after the pin was written and before this
board committed it leaves the mount's record, such as `team.json`, modified,
and a second call writes nothing. Commit it with `git -C <board> commit -m
"Move the pin" -- team.json`.
