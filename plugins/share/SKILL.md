---
name: share
description: 'Copy a record from this board into a mounted board, such as a team''s, with its children, every record and file it references and the schemas of their types, so a reader there can open it. Use it when someone else needs to read a record you wrote. Start with `board -C <board> call share.to --path /<record> --to /<mount> --dry_run true`, which lists what would be copied. SKILL.md and README.md in the plugin''s directory say more.'
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
2. Run `board -C <board> call share.to --path /<record> --to /<mount>
   --dry_run true`. A flag is named after the input property, with an
   underscore; `--dry-run` is refused with `board::cli::usage`. It lists what would be copied and the schemas the
   destination lacks, or refuses with the reason. Nothing is written. Board's
   checks for a write through a mount run only in step 4.
3. Fix what it refuses:
   - `share::path::not_found`: `--path` names nothing in this board's own
     tree. Check the path with `board -C <board> ls`; a record read through
     a mount cannot be shared from here.
   - `share::target::differs`: the destination holds a different file at a
     listed path. Share is a copy, never a merge. Rename your record, or ask
     the destination's owner which copy is right.
   - `share::closure::unresolved`: a record references a path through a mount
     or link that the destination does not resolve into the same repository.
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

   The share follows only what records reference. A copied note's links are
   not followed, so a note it links to is not copied. Share that note with
   its own call, or reference it from a record in the set.
4. Run the same call without `--dry_run`. It writes the copies through the
   mount as one commit in the mounted repository and moves the mount's pin
   to it. It does not push. A refusal with `share::board::refused` carries
   Board's own code, and for a refused batch `refusals` names each path and
   its code: fix what it names and run the call again.
5. Run `board -C <repository> sync` to push, where `<repository>` is the
   `repository` the call printed. A reviewer opens the record at its path in
   that repository at the printed `commit`.

A second call writes only what the destination still lacks, and leaves a path
it already holds equally as it is. A second call finishes a share killed
before the mounted repository committed, or after it committed and before
the pin moved. If someone changed a shared file in the mounted checkout in
between, Board refuses with `board::mount::stale` naming it; remove or
restore that file and run the call again. A kill can leave a `board.lock`
and git's `index.lock` in the git directories of both repositories, this
board's and the mounted one. `board::put::locked` names a `board.lock`, and
git names an `index.lock`; remove each when no put or git command is
running. A kill after the pin was written and before this
board committed it leaves the mount's record, such as `team.json`, modified,
and a second call writes nothing. Commit it with `git -C <board> commit -m
"Move the pin" -- team.json`.
