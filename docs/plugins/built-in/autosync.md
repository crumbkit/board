# autosync

The autosync plugin keeps each board the owner listed in step with its
remote, from a launchd agent, and pushes only commits a gitleaks scan
passed. plugins/autosync/README.md has the commands and codes.

- Each board is an `autosync/board/v1` record at `/autosync/boards/<name>`
  in the personal board, with its path, branch and allowed push URLs.
- `add` signs the record with Touch ID through `Host::sign_owner_write`, the
  `board/write/v1` statement of a put by the owner
  ([signing](../../architecture/writes/signing.md)). `run` checks it with
  `Host::verify_owner_write` and skips a record that does not verify.
- A run takes `run.lock` in `~/Library/Logs/board-autosync`, then for each
  board:
  - Checks that the path is its own checkout and that every push URL is
    allowed.
  - Runs the [guarded sync](../../architecture/writes/sync/guarded.md).
  - Scans the commits each push would send with `gitleaks git`, from the
    `before_push` the guarded sync asks, and holds the push on a finding.
- A board that fails gets a task in the personal board, rewritten when the
  reason changes and closed when the board is clean. A notification is
  shown when a board's outcome changes.
- Known boards with no record are listed as `not_synced`.
- The status file, the log and the scratch directory are written through
  board-index's state functions, outside every board.
- No finding's secret is kept. Reports hold the rule, commit, file and line.
