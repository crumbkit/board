# Repository records

This file holds detail for docs/architecture/paths.md. It says how a board
names a repository.

## board/repo/v2

A repository has one record, at `/repositories/<remote>`, in each board
whose records name it. A record that names a repository holds a typed
reference to that path, `"x-board-type": "board/repo/v2"`, so `board refs`,
`board check` and `share.to` see it.

```json
{"$type": "board/repo/v2", "remote": "crumbkit/board"}
```

- `remote` is the only field. It is the repository's identity form, which
  `board_model::repo_identity` computes from an origin URL. Its tests read
  the table in `crates/board-model/src/repo/remotes.json`.
  - A GitHub remote, over https, ssh, git or scp-like `git@github.com:`, with
    or without a host alias such as `github.com-work`, is `owner/name` in
    lower case, because GitHub ignores case.
  - Any other host is `<host>/<path>`, with the host in lower case and no
    scheme, user, password, port, trailing `.git` or trailing `/`, such as
    `gitlab.com/example-org/app`. A GitHub owner holds no dot and a host
    does, so the two forms never collide.
  - A local path, a `file://` URL or a relative path names a directory on
    one machine. It has no identity form and is refused with
    `board::repo::local_remote`.
- The path is the identity, so one repository has the same path and the
  same bytes in every board, and two records of one repository cannot
  exist. The record holds no clone path, branch or commit.
- `check` degrades a record whose path is not `/repositories/<remote>` with
  `board::repo::path_mismatch`, naming the path it belongs at. It degrades
  one whose `remote` is not in identity form, or that holds any other field,
  with `board::repo::invalid`, naming the identity form. `put` refuses an
  invalid one with that code.
- A segment of the path holds at most 250 bytes, so the file's name fits
  with `.json`. A longer identity is refused with `board::repo::too_long`.
  GitHub names never reach it.
- The schema, generated from the Rust struct, is
  `crates/board-model/types/board/repo/v2.schema.json`. Board reads the type
  by its Rust type and needs no schema in `.board/types/`. `remote` has the
  shared shape `Remote`.

A reference is read in the referring record's board, so a record in a
mounted board names that board's own copy, never the parent's. Each writer
that names a repository adds the line `board_ops::repo::ensure` gives to
the batch that writes its own record: a put with `"expect": "none"` when the
record is missing, and nothing when something is at the path. Two writers
that race leave one record, and the second batch is `board::put::stale`.
`share.to` copies a repository record through the reference, as it copies
any referenced record; the bytes are the same in every board, so the
destination's copy counts as present.

`board repo add <remote>` writes the record from any remote form, and
`board repo add --from <dir>` reads the origin of the clone that holds the
directory. Each prints the path. When the record exists it writes nothing
and says `unchanged`. A clone with no origin is `board::mount::no_remote`.

## board/repo/v1

`board/repo/v1` is read until `board mount migrate`, from
/changes/mounts-follow-the-clone, replaces each one with a `board/repo/v2`
and a mount.

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

- `remote` is required. It is the origin URL in its stored form: `owner/name`
  for GitHub, over https, ssh, git or scp-like `user@github.com:`, with or
  without a host alias such as `github.com-work`; otherwise the URL without
  its user and password. A `remote` that is not its own stored form is
  `board::mount::invalid`. `board_model::shown_remote` computes the stored
  form.
- `remote` is unique within a board's tree. `check` degrades every repo
  record whose `remote` another repo record in the same tree also holds with
  `board::repo::duplicate_remote`, naming the others, even when its scope
  holds only one of them.
- `checkout` is optional. With it, the record is a mount: `repo`, `follow`
  and `commit` mean exactly what they mean in a `board/mount/v1` mount, the
  repository's tree appears beneath the record's path, and the same codes
  refuse a bad one. `board mount update` moves `checkout.commit`. Without
  it, nothing appears beneath it, and a path below it is
  `board::path::not_found`.
- Any other field, a missing `remote` or a bad `checkout` is
  `board::mount::invalid`.

## Finding the record from a directory

When no board holds the directory Board started in, Board reads the known
boards (docs/architecture.md, Boards and entries). In each known board it
first looks for a mount whose clone is the directory's repository, with a
`board/mount/v1` mount or a repo record's checkout alike. When no known board
has such a mount, it reads that repository's `origin`, as `git remote get-url origin` gives it,
and looks for a `board/repo/v1` record in the known board's working tree whose `remote`
is the origin's stored form. Such a record gives the prefix `/repos/<name>`,
its own path, from any directory in the clone. A mount in any known board outranks a repo record matched by remote in
another. Remote matches count across all known boards, so two boards with a record for one `remote` are
`board::config::ambiguous`. When nothing matches, the run is
`board::config::not_found`, so a check there still means the repository has
no rules.

A checkout of a repository that is itself a board runs as that board,
because Board finds `.board/board.json` first. A plugin that needs the
caller's repository reads the raw origin of `BOARD_CALLER_DIR`, which a
built-in gets from `Host::caller_remote`, and looks up the repo record
whose `remote` equals that origin's `board_model::shown_remote`.
