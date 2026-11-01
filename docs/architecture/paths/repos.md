# Repo records

This file holds detail for docs/architecture/paths.md. It says how a board
names a repository, with a clone mounted or without one.

A repository gets one record, conventionally at `/repos/<name>.json`, whether
or not the board mounts it. Other records name the repository by that path,
so `board check` reports a reference to a repository the board does not know.

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
  its user and password. Two clones of one repository give the same value.
  A `remote` that is not its own stored form, such as
  `git@github.com:crumbkit/board.git`, is `board::mount::invalid`.
  `board_model::shown_remote` and `board_plugin.repos.shown_remote` compute
  the stored form, and both read the table in
  `python/board-plugin/tests/remotes.json` in their tests.
- `remote` is unique within a board's tree. `check` degrades every repo
  record whose `remote` another repo record in the same tree also holds with
  `board::repo::duplicate_remote`, naming the others, even when its scope
  holds only one of them.
- `checkout` is optional. With it, the record is a mount: `repo`, `follow`
  and `commit` mean exactly what they mean in a `board/mount/v1` mount, the
  repository's tree appears beneath the record's path, and the same codes
  refuse a bad one. `board mount update` moves `checkout.commit`. Without
  it, the record is an identity only: nothing appears beneath it, and a path
  below it is `board::path::not_found`.
- Any other field, a missing `remote` or a bad `checkout` is
  `board::mount::invalid`.

`board/mount/v1` stays for links and for mounts of boards that are not
repositories of work, such as a shared team board. A repo record is written
with `board put`.

## Finding the record from a directory

When no board holds the directory Board started in, Board reads the known
boards (docs/architecture.md, Boards and entries). In each known board it
first looks for a mount whose clone is the directory's repository, with a
`board/mount/v1` mount or a repo record's checkout alike. When none is, it
reads that repository's `origin`, as `git remote get-url origin` gives it,
and looks for a repo record in the known board's working tree whose `remote`
is the origin's stored form. Such a record gives the prefix `/repos/<name>`,
its own path, from any directory in the clone. Matches count across all known
boards, so two boards with a record for one `remote` are
`board::config::ambiguous`. When nothing matches, the run is
`board::config::not_found`, so a check there still means the repository has
no controls.

A checkout of a repository that is itself a board runs as that board,
because Board finds `.board/board.json` first. A plugin that needs the
caller's repository reads `board_plugin.repos.caller_remote()`, the raw origin
of `BOARD_CALLER_DIR`, and looks up the repo record whose `remote` equals
`shown_remote(caller_remote())`.
