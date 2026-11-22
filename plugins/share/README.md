# share

A Board plugin that copies a record from a board into a board it mounts, such
as a team's, with everything a reader there needs to open it. The source keeps
its record. [docs/plugins/share.md](../../docs/plugins/share.md) holds the
design and the reasons for it; this file says how to call it.

## The call

    board share to --path /evals/x --to /team [--yes] [--allow-uncopied] [--local_attachments]

- `path` names a record or file in this board's own tree, not one read through
  a mount.
- `to` names a mount record in this board, such as `/team`. A path inside a
  mount, a mount inside a mounted board and a link to a mount are refused. The
  share writes through the mount, so Board's checks for a write through a
  mount apply: the mounted checkout is on the branch the mount follows, its
  board commits on put, and no path in the set changed since the pin.
  [Setting up a shared board](#setting-up-a-shared-board) says how to make one.
- Without `yes`, the call is a preview. It lists what would be copied,
  decides the plugin's own refusals except `share::closure::uncopied`, and
  writes nothing. Board's checks for a write through a mount run only when
  the call writes. `next` gives the command that writes.
- `yes` writes the set.
- `allow-uncopied`, with `yes`, writes the set even when `not_copied` lists
  something: a link or back-ref the share cannot copy, or a back-ref's
  reference through a mount the destination does not resolve. Without it,
  such a target makes the call refuse.
- `local_attachments` copies an attachment whose bytes are on this machine.
  Pass it only when every reader of the destination can read that location.

Each boolean flag may be given bare, so `--yes` means `--yes true`.

## What is copied

The set is the entry at `path`, every entry under `path/`, and these extras,
followed until nothing new is reached:

- each entry a typed reference of a record in the set names, as the
  record's schema declares it, including a Markdown record's frontmatter
  references;
- each record or file a Markdown link in a file in the set names, relative
  or absolute;
- each record in this board's own tree whose typed reference names an entry
  that collects back-refs. This is a back-ref, such as a decision whose
  `refs` names the shared project.

The entry at `path`, the entries under it and wholes collect back-refs. A
whole is a record that an entry collecting back-refs names through a
part-of field: a field whose schema in this board's `.board/types/` carries
`"x-board-part-of": true`, on `items` for a list. The `parent` of a
`tasks/task/v2` task is one. Sharing a task therefore brings its project,
the project's decisions and questions that name it, and the same for the
project's own parent, up the chain. A whole's back-ref is skipped when each
typed reference it makes to the whole is a part-of field, so a shared task
does not bring the other tasks of its project. Sharing the project itself
still brings them.

Back-refs, and records reached by a reference or link that are not wholes,
collect none, so sharing an eval does not bring every record that names the
eval's suite. A back-ref's own references and links are followed. A path
written in prose is not followed, because only a typed reference or a link
is one. A board whose copy of a type's schema lacks the keyword finds no
wholes through that type until `board plugin enable` refreshes the copy.

Sharing `/evals/x` copies the eval, the evidence under `/evals/x/evidence/`,
the suites and runs it references, and a cases file a suite references, such
as `/notes/cases.md`. A typed reference to a directory reaches every entry
under it.

A link or back-ref target the share cannot copy is listed in `not_copied`
with a reason:

- `directory`: a link to a directory, whose entries are not copied. A
  directory under `path`, or one a typed reference reaches, is not listed,
  because its entries are copied anyway.
- `mount`: the target is reached through a mount.
- `link`: the target is reached through a link in this board.
- `missing`: the link names nothing.

A typed reference through a mount that only back-refs make, and that the
destination does not resolve into the same repository, is listed as
`mount`, with `from` the back-ref.

Each entry lands at the same path in the destination, so no
reference is rewritten. The schema of every record type in the set goes too,
except Board's own `board/*` types.

A typed reference that resolves through a mount or a link is not copied. It
must resolve, in the destination, into the same repository, unless only
back-refs make it. A reference
through a link inside this board can therefore be shared only if the record
names the link's target path directly, or the destination mounts this board.
A link inside the set is refused. A record with a broken reference is copied
as it is, because `board put` admits it.

## What it writes

With `yes`, one `board put --batch` through the mount writes each schema the
destination lacks and every path it does not already hold, the paths with
`expect` set to `none`. Board makes one commit in the mounted board and then
moves this board's pin to it, or writes nothing.

It does not push. Run `board -C <repository> sync`.

The output is:

- `shared`, the path shared;
- `extras`, at most 100 paths in the set outside `path`, each with `via`,
  how it was reached (`ref`, `link` or `back_ref`), and `from`, the entry
  that reached it first, with `extras_total` counting them all;
- `not_copied`, at most 100 link and back-ref targets, and back-refs'
  references through a mount, that the share leaves behind, each with its
  `path`, `reason` and `from`, with `not_copied_total` counting them all;
- `wholes`, at most 100 wholes outside `path`, each with `path`, `from`,
  the part that names it, and `back_refs`, how many records it added to the
  set, with `wholes_total` counting them all. It names each whole even when
  `extras` is cut at 100;
- `next`, the command that writes: the same call with `--yes`, and with
  `--allow-uncopied` when `not_copied` is not empty. It starts
  `board -C <root>`, where `<root>` is this board's repository, so it writes
  the same board from any directory. It is null after a write;
- `repository`, the repository the mount reaches;
- `commit`, the commit in it that holds the copies, which the pin now names,
  null when nothing was written;
- `types`, the types whose schemas were, or would be, added;
- `copied`, at most 100 paths, with `total` and `truncated`;
- `uris`, the `board://` URI of each path in `copied`, empty when the
  destination's repository has no origin;
- `unchanged`, the number of paths the destination already held equally.

## Setting up a shared board

A team's shared board is a git repository that each colleague clones.
Make the board in it once, from an empty repository:

    git clone git@github.com:<owner>/<team-board>.git ~/team-board
    cd ~/team-board
    git checkout -b main
    board init
    board plugin enable evals
    board plugin enable runs
    git add .board
    git commit -m 'Start the team board'
    git push -u origin main

- Run `git checkout -b main` only when the clone has no branch, as an empty
  repository's clone has none.
- Enable the plugins whose records you will share, here evals and runs.
- A colleague whose repository already holds a board only clones it.

Then mount the clone in your own board as `/team`:

    board mount add /team ~/team-board

- `board mount add <path> <repo>` writes a `board/mount/v1` record at the
  path. Its `repo` is the clone's directory, its `follow` the branch checked
  out in the clone, and its `commit` that branch's commit now.
- `follow` must name a local branch, such as `main`, because the share
  writes through the mount. Board refuses a write through a mount whose
  `follow` is a remote branch, such as `origin/main`. Pass `--follow main`
  when the clone has another branch checked out.

## Refusals

The plugin decides each of these before anything is written, in the preview
too, except `share::closure::uncopied`, which only a call with `yes` meets.
Every refusal except
`share::path::not_found` and `share::mount::not_found` names each path it
refuses in its `paths` field.

| Code | When |
| --- | --- |
| `share::path::not_found` | `path` names nothing in this board's own tree |
| `share::mount::not_found` | `to` is not the mount record of another repository: it reaches no board, mounts this board's own repository, or is a path inside a mount, a mount inside a mounted board or a link to a mount |
| `share::link::unsupported` | a link, such as a `board/mount/v1` record, is in the set; a batch would write a copy of its target in its place |
| `share::record::invalid` | a record in the set carries a code other than a broken reference's, so `board put` would refuse it, or a schema it needs stopped parsing after the set was read |
| `share::closure::unresolved` | a typed reference through a mount or link, made by an entry that is not a back-ref, does not resolve into the same repository in the destination |
| `share::closure::uncopied` | with `yes` and without `allow-uncopied`, `not_copied` lists a path; the message gives each path's reason |
| `share::file::unsupported` | a file in the set is not UTF-8, or is a `.json` file without `$type`, which a batch line cannot write |
| `share::attachment::local` | an attachment's `location` is a path on this machine, without `local_attachments` |
| `share::target::differs` | the destination holds a different record, file or schema at a path in the set |

A record is the same when its JSON value is equal, so key order and spacing do
not matter; a file must have equal bytes. A refusal from `board` itself is
`share::board::refused` with Board's code and Board's other fields. Board
refuses the batch, for example, when the mounted checkout is on another
branch, when its board does not commit on put, or when a path changed in the
mounted board since the pin. Then `refusals` lists each refused batch line
with its code and a message naming the path.

## Enabling

Board has the plugin built in, so nothing is installed:

    board plugin enable share

The plugin ships no types. An older install at
`${XDG_DATA_HOME:-$HOME/.local/share}/board/plugins/share` makes `call` and
`plugin enable` fail with `board::plugin::duplicate`. Remove it; it is
usually a symlink to a checkout, so remove the link, not the checkout.
`SKILL.md` holds the steps an agent follows.

## Developing

The plugin is the crate `crates/board-plugin-share`. It generates the
manifest's input and output schemas from Rust structs with schemars. The
tests of the plugin through the binary are in
`crates/board-cli/src/tests/built_in/share.rs` and `share/refusals.rs`. They
share records of the evals and runs plugins' types from a board into a
mounted team board, both real git repositories.
