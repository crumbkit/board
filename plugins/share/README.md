# share

A Board plugin that copies a record from a board into a board it mounts, such
as a team's, with everything a reader there needs to open it. The source keeps
its record. [docs/plugins/share.md](../../docs/plugins/share.md) holds the
design and the reasons for it; this file says how to call it.

## The call

    board call share.to --path /evals/x --to /team [--dry_run true] [--local_attachments true]

- `path` names a record or file in this board's own tree, not one read through
  a mount.
- `to` names a mount record in this board, such as `/team`. A path inside a
  mount, a mount inside a mounted board and a link to a mount are refused. The
  share writes through the mount, so Board's checks for a write through a
  mount apply: the mounted checkout is on the branch the mount follows, its
  board commits on put, and no path in the set changed since the pin.
- `dry_run` lists what would be copied, decides the plugin's own refusals,
  and writes nothing. Board's checks for a write through a mount run only on
  the real call.
- `local_attachments` copies an attachment whose bytes are on this machine.
  Pass it only when every reader of the destination can read that location.

## What is copied

The set is the entry at `path`, every entry under `path/`, and every entry a
record in the set references, followed until nothing new is reached. Sharing
`/evals/x` copies the eval, the evidence under `/evals/x/evidence/`, the
suites and runs it references, and a cases file a suite references, such as
`/notes/cases.md`. A reference to a directory reaches every entry under it.
Only records' references are followed. A copied Markdown file's links are
not, so a note it links to stays behind, and `board check` reports the link
as dangling in the destination.
Each entry lands at the same path in the destination, so no
reference is rewritten. The schema of every record type in the set goes too,
except Board's own `board/*` types.

A reference that resolves through a mount or a link is not copied. It must
resolve, in the destination, into the same repository. A reference through a
link inside this board can therefore be shared only if the record names the
link's target path directly, or the destination mounts this board. A link inside the set
is refused. A record with a broken reference is copied as it is, because
`board put` admits it.

## What it writes

One `board put --batch` through the mount writes each schema the destination
lacks and every path it does not already hold, the paths with `expect` set to
`none`. Board makes one commit in the mounted board and then moves this
board's pin to it, or writes nothing.

It does not push. Run `board -C <repository> sync`.

The output is `repository`, the repository the mount reaches; `commit`, the
commit in it that holds the copies, which the pin now names, null when nothing
was written; `types`, the types
whose schemas were added; `copied`, at most 100 paths, with `total` and
`truncated`; and `unchanged`, the number of paths the destination already
held equally. A dry run returns the same with `commit` null.

## Refusals

The plugin decides each of these before anything is written, on a dry run
too. Every refusal except
`share::path::not_found` and `share::mount::not_found` names each path it
refuses in its `paths` field.

| Code | When |
| --- | --- |
| `share::path::not_found` | `path` names nothing in this board's own tree |
| `share::mount::not_found` | `to` is not the mount record of another repository: it reaches no board, mounts this board's own repository, or is a path inside a mount, a mount inside a mounted board or a link to a mount |
| `share::link::unsupported` | a link, such as a `board/mount/v1` record, is in the set; a batch would write a copy of its target in its place |
| `share::record::invalid` | a record in the set carries a code other than a broken reference's, so `board put` would refuse it |
| `share::closure::unresolved` | a reference through a mount or link does not resolve into the same repository in the destination |
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

## Installing

    uv sync --frozen --no-dev
    ln -s "$PWD" "${XDG_DATA_HOME:-$HOME/.local/share}/board/plugins/share"
    board plugin enable share

The plugin ships no types. `SKILL.md` holds the steps an agent follows.

## Developing

The gates, which CI runs: `uv run ruff format --check .`, `uv run ruff check .`,
`uv run pyright` and `uv run pytest`. The tests drive `$BOARD_BIN`, by default
the workspace's `target/debug/board`, so build it first with
`cargo build -p board-cli`. They share records of the evals plugin's types
from a personal board into a mounted team board, both real git repositories.
`plugin.json` is generated from `src/board_share/models.py` by
`board_share.manifest`, and a test checks that the committed file matches.
