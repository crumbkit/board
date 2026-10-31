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
  "remote": "crumbkit/board",
  "checkout": {
    "repo": "~/src/board",
    "follow": "origin/main",
    "commit": "8c3179b3…"
  }
}
```

- `remote` is required. It is the origin as `board_plugin.repos.shown_remote`
  already stores it: `owner/name` for GitHub, and otherwise the URL without
  its user and password. Two clones of one repository give the same value.
- `remote` is unique: `board check` reports a second record with the same
  `remote` as `board::repo::duplicate_remote`.
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
clone's `origin` remote and, when a known board's repo record has that
`remote`, runs as that board with `/repos/<name>` as the prefix. Two known
boards with a record for the same `remote` is `board::config::ambiguous`.
When no known board has it, Board runs as the `personal` board that
`config.json` names, with no prefix. With no `personal` board, the clone
stays `board::config::not_found`, as today. This mapping and its fallback
live in `board-ops/src/known.rs`, beside today's.

A checkout of a repository that is itself a board, such as crumbkit/board,
runs as that repository's board, because Board finds `.board/board.json`
before it reads the known boards. Every work command this plan adds is
therefore run as `board --personal call …`, and `changes.brief`, the pull
request gate's refusal and the changes skill print it in that form.
`BOARD_CALLER_DIR` still names the directory Board was started in, so the
plugin sees the caller's clone.

A plugin learns the caller's repository as the tasks and notes plugins do
today: `board_plugin.repos.caller_remote()` reads the origin of
`BOARD_CALLER_DIR`, and the plugin looks up the repo record whose `remote`
matches.

## Creating the record

Nobody writes a repo record by hand. `changes.start` creates it from
`caller_remote()` when no record has that remote (built with
plan/work-records.md), at `/repos/<name>`, where `<name>`
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

One pull request in `board-model`, `board-tree` and `board-ops`. The
tests:

- A repo record with a checkout resolves exactly as the equivalent mount, for
  every case in the existing mount tests.
- A repo record without a checkout is a valid reference target and has no
  children.
- A directory in an unmounted clone runs with its repo record's prefix, and
  one with no record runs as the personal board.
- Each refusal has an admitted twin.

Done when the owner's board has migrated and `board check` reports nothing
new.
