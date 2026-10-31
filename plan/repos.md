# Repo records

A repository gets one record at `/repos/<name>.json`, whether or not the
board mounts it. Every other record names the repository by that path, so
`board check` reports a reference to a repository the board does not know,
and no record holds a repository's name or URL as a free string.

This milestone builds the declaration that the deferred "Repositories across
boards" sketch in PLAN.md named. That sketch put it in
`/.board/repos/<name>.json`; it moves to `/repos/<name>.json`, where today's
mounts already sit, so existing references keep working. References into
another board's repository (`name:/path`) stay deferred.

## The record

```json
{
  "$type": "board/repo/v1",
  "remote": "github.com/crumbkit/board",
  "workflow": "/workflows/personal",
  "checkout": {
    "repo": "~/Code/board",
    "follow": "origin/main",
    "commit": "8c3179b3…"
  }
}
```

- `remote` is required. It is the host and path of the repository's
  canonical remote, with no scheme, user or `.git` suffix, so two clones of
  one repository give the same value.
- `workflow` is optional. It refers to a `/workflows/<name>` record
  (plan/work-records.md). A repository without one uses the board's default
  workflow.
- `checkout` is optional. With it, the record is a mount: `repo`, `follow`
  and `commit` mean exactly what they mean in `board/mount/v1` today, and the
  repository's tree appears beneath `/repos/<name>/`. Without it, the record
  is only an identity. Nothing appears beneath it, and a path below it is
  `board::path::not_found`.

`board/mount/v1` stays for links (`path`) and for mounts of boards that are
not repositories of work, such as a shared team board. Only `board-tree`
reads either type as a pointer, as today.

## Creating the record

Nobody writes a repo record by hand. The first command that needs one
creates it:

- `changes.start` and `evals` read the current checkout's `origin` remote.
  If no repo record has that `remote`, they write one at `/repos/<name>`,
  where `<name>` is the remote's last path segment. A clash with an existing
  record of another remote is `board::repo::name_taken`, which suggests
  `--repo-name`.
- A mount is added as today, with `board put`: the record gains `checkout`.
- `board mount update` moves `checkout.commit`, as it moves a mount's
  `commit` today.

A repository without a checkout therefore has a record as soon as anyone
starts a change or records an eval about it. Mounting it later adds
`checkout` to the same record, and nothing that refers to it changes.

## Finding the record from a checkout

Board already maps the current directory to the mount whose `repo` holds it.
It now also maps a directory to a repo record by `remote`: when no checkout
matches, Board reads the directory's `origin` remote and looks for a record
with that `remote`. This is how a command run inside an unmounted clone finds
its repository.

## Migration

A one-off script, run once and not kept, rewrites each `board/mount/v1` at
`/repos/<name>.json` whose `repo` is a git repository as a `board/repo/v1`,
reading `remote` from that clone's `origin`. It prints each rewrite and
writes nothing with `--dry-run`. A mount whose clone is missing is left as it
is and listed.

## Build

One pull request in `board-model` and `board-tree`. The tests:

- A repo record with a checkout resolves exactly as the equivalent mount, for
  every case in the existing mount tests.
- A repo record without a checkout is a valid reference target and has no
  children.
- `changes.start` in an unmounted clone creates the record once, and a second
  start reuses it.
- Each refusal has an admitted twin.

Done when the owner's board has migrated and `board check` reports nothing
new.
