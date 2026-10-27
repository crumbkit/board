# The share plugin

This file holds detail for docs/plugins.md. It says why the `share` plugin
copies records between boards the way it does. `plugins/share/README.md` says
how to call it, what it copies and its codes.

A person writes records in a personal board and wants a reviewer to read one
in a team board. `board call share.to --path /evals/x --to /team` copies the
record, the records and files a reader needs with it, and their schemas into
the board mounted at `/team`. Board's core has no copy or share command; the
plugin composes `query`, `refs`, `get` and `put --batch`.

## It copies

The source keeps its record and the destination gets an equal one at the same
path. Moving the record, and leaving a link at its old path, was rejected:
a plugin may not touch the working tree, and a link beside
a record's child directory is `board::path::shadowed`, so the children would
have to go as well.

Because each entry keeps its path, no reference is rewritten. The set is found
from the index, which holds every typed reference, so no relation is declared
by hand: an eval's evidence is its children, and its suites and runs are
what it references. A reference to a directory reaches every entry under it.
Only the references a record's schema declares are followed, including
those in a Markdown record's frontmatter, which is copied as text with its
type's schema and checked again by the destination's `put`. A Markdown
file's links, which `refs` marks `link`, are not followed, so a link cannot reach a
directory either. A copied note may link to a note the destination lacks; the
link dangles there, and `board check` reports it. The owner chose this on
2026-10-21, so a share copies what records name and no more. A
reference through a mount is not copied, because the
mounted repository is not this board's to copy; it must resolve to the same
repository in the destination, or the reader would meet a dangling reference.
There is no flag to copy one dangling until a need shows. A link inside the
set is refused, because `board get` reads through it and a batch would write a
copy of its target in its place. A reference through a link is treated as one
through a mount: it is not copied, and must resolve into the same repository
in the destination.

## One commit, decided first

The plugin's own refusals are decided before anything is written, on a dry
run too: a path the destination holds differently, a schema it holds
differently, a reference it cannot resolve, a file a batch cannot hold, a
link, a record the index already reports with a code that `put` would refuse,
and an attachment its readers cannot fetch. A broken reference is not refused, because `put` admits it.
There is no override for a difference; the person renames the record or
settles which copy is right.

The share is one `board put --batch` through the mount. It holds a schema
line for each schema the destination lacks and a line, with `expect` set to
`none`, for each path it does not already hold. Board checks the schema lines
first, writes every line in the mounted board as one commit there, and then
moves the pin to it, so the share is all or nothing. Because the batch goes
through the mount, Board's checks for a write through a mount apply: the
mounted checkout is on the branch the mount follows, its board commits on
put, and no target changed since the pin. The plugin reads the destination
through the mount as well, so it compares against the pin.

Board's checks for a write through a mount run only on the real call,
because Board has no dry-run put. For the same reason the plugin compares
schemas itself, although Board would refuse a different one with
`board::schema::differs`: only the plugin can report a differing schema on a
dry run, and it reads the destination's schemas anyway to know which are
missing. It reads the repository and the pin from `board refs <mount>`,
because `board get` on a mount point reads through it and finds a directory.
`--to` must name the mount record itself, with one pointer that reaches
another repository. A path inside a mount, a mount inside a mounted board and
a link to a mount are refused with `share::mount::not_found`, because Board
refuses the real call through them and a dry run would otherwise pass.

A path the destination already holds equally is skipped, so running the share
again finishes one killed after the mounted commit and before the pin moved:
Board finds the paths at HEAD and moves the pin. A share killed before the
mounted commit leaves files uncommitted in the mounted checkout. Board wrote
them as canonical bytes, and a write through a mount admits a path whose
working file equals the blob it writes, so running the share again finishes
it too. If a shared file was changed in between, Board refuses with
`board::mount::stale` naming it. A SIGKILL can leave `board.lock` and git's
`index.lock` in the git directories of both repositories;
`board::put::locked` names a `board.lock` and says to remove it when no put
runs, and git names an `index.lock`. A kill after Board rewrote the pin and before it committed the mount
record leaves the record modified, and a second share finds nothing to copy.
The plugin cannot finish that case: it reads git state only through Board,
which does not report an uncommitted pin, and `board mount update` writes
nothing when the pin is current. The person commits the mount record, as
`SKILL.md` says.

## What a reader can reach

A record names an attachment by its `location`. The `directory` store keeps
the bytes on the sender's machine, so a reader of the team board on another
machine cannot fetch them. A location with no URL scheme, or a `file:` one, is
refused unless the call passes `local_attachments`, for readers who share the
location. A location in a store every reader reaches is copied unchanged.

A binary file kept in the tree cannot be copied, because a batch `text` line
holds UTF-8 only. Putting it in a store turns it into an attachment record,
which can be.

The share does not push. `board sync` pushes the destination when the person
chooses, and a reviewer then opens the record at its path in the repository at
the commit the share printed. No provenance record is written; the
destination's history shows the set arriving in one commit.
