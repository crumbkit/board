# Repo records

A repository gets one record at `/repos/<name>.json`, whether or not the
board mounts it. Every other record names the repository by that path, so
`board check` reports a reference to a repository the board does not know,
and no record holds a repository's name or URL as a free string.

This milestone builds the declaration that the deferred "Repositories across
boards" sketch in PLAN.md named. That sketch put it in
`/.board/repos/<name>.json`; it moves to `/repos/<name>.json`, where today's
mounts already sit, so existing references keep working.

## The record

```json
{
  "$type": "board/repo/v1",
  "remote": "github.com/crumbkit/board",
  "checkout": {
    "repo": "~/src/board",
    "follow": "origin/main",
    "commit": "8c3179b3…"
  }
}
```

- `remote` is required. It is the host and path of the repository's
  canonical remote, with no scheme, user or `.git` suffix, so two clones of
  one repository give the same value.
- `checkout` is optional, and `remote` is unique: `board check` reports a
  second record with the same `remote` as `board::repo::duplicate_remote`.
- `checkout` is optional. With it, the record is a mount: `repo`, `follow`
  and `commit` mean exactly what they mean in `board/mount/v1` today, and the
  repository's tree appears beneath `/repos/<name>/`. Without it, the record
  is only an identity. Nothing appears beneath it, and a path below it is
  `board::path::not_found`.

`board/mount/v1` stays for links (`path`) and for mounts of boards that are
not repositories of work, such as a shared team board. Only `board-tree`
reads either type as a pointer, as today.

Extending `board/mount/v1` with `remote` and an optional `repo` would avoid
a second pointer type. A mount without a target, though, is a pointer to
nothing, which `board check` rightly reports as dangling. So an identity with
no checkout gets its own type.

## Finding the record from a directory

Board already maps the caller's directory to the mount whose `repo` holds
it, and runs the call with that mount's path as its prefix. `board-tree` now
also maps a directory in a clone the board does not mount: it reads the
clone's `origin` remote and, when a repo record has that `remote`, runs with
`/repos/<name>` as the prefix. When no record has it, Board runs with no
prefix and passes the normalised remote to the plugin as
`BOARD_CALLER_REMOTE`, beside `BOARD_CALLER_DIR`. No plugin reads git for
this.

## Creating the record

Nobody writes a repo record by hand. `changes.start` creates it from
`BOARD_CALLER_REMOTE` when the caller's repository has none, at `/repos/<name>`, where `<name>`
is the remote's last path segment. A clash with an existing record of
another remote is `changes::repo::name_taken`, which suggests `--repo-name`.
Evals and other records refer to the record once it exists. A mount is added
as today, with `board put`: the record gains `checkout`. `board mount update`
moves `checkout.commit`, as it moves a mount's `commit` today.

## Migration

A one-off script, run once and not kept, takes a list of mount paths and
rewrites each `board/mount/v1` among them as a `board/repo/v1`, reading
`remote` from that clone's `origin`. The owner names the mounts, because a
team board is a git repository too and stays a mount. The script prints each
rewrite and writes nothing with `--dry-run`.

## Build

One pull request in `board-model` and `board-tree`, with `changes.start`'s
use of it. The tests:

- A repo record with a checkout resolves exactly as the equivalent mount, for
  every case in the existing mount tests.
- A repo record without a checkout is a valid reference target and has no
  children.
- A directory in an unmounted clone runs with its repo record's prefix, and
  one with no record passes `BOARD_CALLER_REMOTE`.
- `changes.start` in an unmounted clone creates the record once, and a second
  start reuses it.
- Each refusal has an admitted twin.

Done when the owner's board has migrated and `board check` reports nothing
new.
