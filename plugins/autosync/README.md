# autosync

A Board plugin that keeps each board the owner listed in step with its
remote on an interval. It pushes only commits that a gitleaks scan passed.

## Enabling

Board has the plugin built in. Enable it in the personal board:

    board --personal plugin enable autosync

Then, for each board, the owner runs `add` and signs the record with Touch
ID, and installs the launchd agent:

    board --personal autosync add --name team --path ~/Code/team --origin git@github.com:team/board.git
    board --personal autosync run --dry-run
    board --personal autosync install

## Records

`autosync/board/v1` is one board at `/autosync/boards/<name>` in the
personal board:

```json
{
  "$type": "autosync/board/v1",
  "path": "~/Code/team",
  "branch": "main",
  "origins": ["git@github.com:team/board.git"],
  "signature": "-----BEGIN SSH SIGNATURE-----\n..."
}
```

- `path` is absolute or starts with `~/`.
- `branch` is `main` when left out.
- `origins` lists every URL the board may push to.
- `signature` is the owner's signature of their put of the other fields at
  this path in the personal board, the `board/write/v1` statement
  (docs/architecture/writes/signing.md). Editing a field, or copying the
  record to another name, breaks it. Deleting a record needs no signature.

## Commands

- `add`: builds the record, asks the owner to sign it through Board's
  signer, and writes it. Without a signer, as in a debug build, it refuses
  with `autosync::sign::no_signer` and writes nothing.
- `run`: one round over every record, one board at a time, under an
  exclusive lock on `run.lock`. `--dry-run` checks each signature and
  origin, prints the range a push would scan, and changes nothing.
  `--scan-timeout <seconds>` gives the scan longer than 600 seconds.
- `status`: each board's last round, from the status file.
- `install`: writes `~/Library/LaunchAgents/board.autosync.plist` to a
  temporary name, renames it into place and bootstraps it. It runs
  `board --personal autosync run` every 300 seconds, or `--interval`, 60 to
  3,600, and at load, with the `PATH` `install` ran with. `--dry-run`
  prints the plist.
- `uninstall`: boots the agent out and deletes its plist.

## One board's round

1. The record's signature must verify against the allowed signers file, or
   the board is `autosync::board::unsigned`.
2. The path must be the top of its own checkout, not inside another
   (`autosync::board::nested`).
3. Every push URL of the remote its branch follows, as git gives it with
   `pushInsteadOf` applied, must equal an entry of `origins` once scheme,
   user, `.git` and trailing slash are dropped (`autosync::origin::not_allowed`).
4. The guarded sync (docs/architecture/writes/sync/guarded.md) refuses another
   branch, staged files and a git operation in progress before it fetches,
   and a merge with any conflict before HEAD moves. It never accepts a
   rewritten remote. Uncommitted edits are never staged, stashed or pushed.
5. Before each push, autosync looks for `.gitleaks.toml` or
   `.gitleaksignore` in the tree at the commit it would push and in the
   working tree. Either one stops the push with
   `autosync::scan::board_config`, since gitleaks would read it. Shared
   boards keep them out.
6. Then it runs `nice -n 10 gitleaks git` over exactly the commits the push
   sends: `<fetched>..<new>`, or every commit reachable from `<new>` when
   the remote has no branch yet. A finding, an error, a timeout or a missing
   gitleaks 8.19.0 holds the push; the board stays merged locally.
7. A rejected push starts another round, up to 3, each scanned again.

## False positives

The fingerprints file is `~/.config/board/autosync/gitleaksignore` on each
Mac. Each line is a gitleaks fingerprint, `<commit>:<file>:<rule>:<line>`,
which the owner adds by hand after reading `status`. A file over 1 MiB
stops every push with `autosync::scan::fingerprints_too_large`.

## Reports

- `~/Library/Logs/board-autosync/status.json` holds each board's last
  round: its outcome, code, message and, for findings, each one's rule,
  short commit, file and line, at most 50. `autosync.log` beside it gets one
  line per board per run, rotated above 1 MB.
- A notification shows when a board's outcome changes.
- A failing board has one open task in the personal board, written with
  `tasks.add`, rewritten when its outcome or number of findings changes, and
  closed with `tasks.done` after the next clean round.
- No report holds a secret's text.

## Limits

- gitleaks still honours a `gitleaks:allow` comment on a committed line.
- A secret no gitleaks rule matches is pushed.
- A run has 30 minutes; boards left after that are `out_of_time`.
