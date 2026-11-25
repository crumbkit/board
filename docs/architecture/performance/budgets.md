# Performance budgets

This file holds detail for docs/architecture.md. It says how fast each
operation must be and how the timing job measures it.

These budgets are macos-15 medians on the generated 10,000-record board,
measured at M4 and rounded up. The exported old corpus (about 8,000 records) is
the other reference board.

| Operation | Budget |
| --- | --- |
| `board get`, warm | 5 ms |
| `board ls` of a record's directory, warm | 500 ms |
| `board check`, warm, nothing changed | 500 ms |
| simple `board query`, warm | 500 ms |
| simple `board query`, warm, 20 mounts | 200 ms |
| full index, cold | 1 s |
| `board call tasks.available --repo /`, warm | 500 ms |
| `board put` of a small record, warm | 300 ms |

`cargo xtask bench` is the timing job. It runs on a generated board in CI's
slow lane, on Linux nightly and on macOS on Monday nights; a release does not run it. It reports every run and fails above three times a budget. A pull request that takes an operation past its budget is caught by the next nightly run on Linux, or by the next Monday run when only macOS is slow. In nextest, `xtask/tests/bench.rs` runs the real bench on a 20-record board and checks its eight rows against three times each budget; a board that small does not show a regression. The owner times the
private export locally with `--board`. The work-count tests are the hard gate.

The plugin call row calls the built-in tasks plugin in process, as the other
rows run. The generated board enables tasks and holds 100 open tasks; one in
five waits on another. The row calls `tasks.available` with `--repo /`, so it
syncs no mount and reads no git remote. The call that syncs notes repos waits
on git and their remotes, and has no budget. The row times the snapshot and
the query and refs the plugin makes. The first call is discarded. Bench skips
the row, and says why, when the board does not enable tasks.

A built-in plugin call takes at most one snapshot of each kind. Its queries
share one View snapshot, and its `refs` share one Report snapshot, which
decides every mount and so never answers a query. A capability the call
calls in the same board whose `readOnlyHint` is true keeps both and reads
through them, and the snapshots it takes count in the call's trace. A host
the call opens on the same board, through `--personal` or `-C`, shares
both; a call into another board keeps its own. A call keeps them across
another session's commits until a refresh of the shared index or its own
effects move them, and the two always show one board. They are not
isolated from the index: any process that refreshes it at a newer tree
rewrites the slot the View reads, so the View is retaken at the newest
`HEAD` and the held Report is dropped with it. Keeping them is safe because
every record a plugin call writes after reading it carries the blob it
read as its `expect`, so Board refuses a write over a newer record as
`board::put::stale`. A schema line takes no
`expect`, because a batch writes a schema only where the board lacks it.
The call drops both before it puts, syncs, updates a mount, decides, calls
a capability without `readOnlyHint: true`, or runs a process in the
board's own working tree. A process that runs elsewhere, such as `git
diff` in a mounted clone or one in a new temporary directory, keeps both,
because a snapshot reads each mount at the commit its mount record names.
Before such a process, or a read-only installed plugin's capability, the
call reads `HEAD`; the next read compares `HEAD` with it once and drops
both if it moved, which catches a `board put` either made. Both snapshots
are taken lazily, so before one is taken while the other is held, the call
compares `HEAD` once and drops the held one if it moved, and both show one
board. A host the call opens on another board that writes back through a
mount into this board does not drop them. In a board whose
`commit_on_put` is false, a put leaves `HEAD` where it was, so there every
process, and every installed plugin's capability, drops both, and every
read compares `HEAD`. A kept View that another
process prunes under (index.md) is dropped, and the query runs on a fresh
one within the same three tries. A call may not see another session's
commit or uncommitted edit made after its first read; the next call does.
With one commit every 2 s on a copy of a real board, a plan-phase `changes
next` took 92 to 127 s and 47 to 54 snapshots when every commit dropped
them, against 6.0 s and 2 snapshots alone.

The generated board commits its records in one commit, so git's index and
HEAD hold them as a real board's do; a put pays for both, and a put into a
board whose records are untracked takes a fifteenth of the time. The put row
writes a new small record for each run, committed as the board's own
identity. On a board given with `--board`, bench skips the row and says why,
because bench writes only a board it generated.

The mounted query row runs on a second generated board. It holds a fifth as
many tasks as the first, refers each to a project in one of up to 20
mounted repositories, one for each 500 records, and mounts each at
`/repos/r<n>`. Each repository is a generated board of a fortieth as many
tasks, with 100 directories of source files. At 10,000 records the board
decides about 13,300 entries, 11,100 of them mounted. Its budget is the
measured median on an M-series laptop, about 103 ms, doubled and rounded
up. The generated repositories hold their objects loose, from one
commit, where a real repository's are mostly packed. On a board given with
`--board`, bench skips the row and says why.

### Comparing two builds

`cargo xtask bench --against <board> [--runs <n>]` compares an older
`board` binary (base) with a release build of this workspace (new) on the
generated boards, or on the board `--board` gives. Bench builds new itself,
with `cargo build --release --package board-cli`, before it generates the
boards, and fails with `xtask::bench::no_board` if that leaves no `board`.
It is the regression check for a change that may be slower; it runs locally
and in no lane. Build the base from `main` in a worktree first:

    git worktree add /tmp/base origin/main
    cargo build --release --package board-cli --manifest-path /tmp/base/Cargo.toml

Each row runs its `board` command as a child process: `get`, `ls`,
`check`, `query`, a check with an empty cache for the cold row, and the
mounted query; there is no put or plugin call. Each binary gets its own
cache, an empty config and data directory, and only `HOME` and `PATH` from
this machine. After one discarded run of each, every round runs both,
alternating which goes first, from base, so 15 rounds, the default, run base
first 8 times and new first 7. Each
round gives the change `(new - base) / base`. A row is slower when the
median change is over 5% and new was slower in at least 12 rounds. Over 15
rounds that is a sign test that calls a row slower by luck under 2% of the
time. The 2% holds only at 15 rounds: with more it is looser, and with
fewer than 12 no row is slower. Faster is the same rule
the other way, and any other row is the same. A row whose base exits with
an error is reported and not judged. The command exits 1 when a row is
slower or new fails where base passed. A missing base fails with
`xtask::bench::no_against`, and `--runs` without `--against` with
`xtask::bench::runs_without_against`.

Targets are the speed an agent loop or a web view needs. Nothing enforces
them:

| Operation | Target |
| --- | --- |
| `board check`, warm, nothing changed | 100 ms |
| simple `board query`, warm | 100 ms |
| `board call plans.open --limit 20`, warm, on the owner's board | 50 ms |

A warm `board check` reads the decisions the index keeps
(performance/warm/decisions.md).

## Command budgets

These budgets judge the hot commands as agents run them on the owner's
personal board. `board timing check` reads them from
`crates/board-cli/src/timing/budgets.rs`, and a test checks that this table
matches. A budget is the p90 of the command's last 20 top-level runs at a
load no higher than the core count (timing.md, The speed check). The owner
set them on 23 November 2026: 100 ms for queries, checks and the commands a
session's start runs, and half the first proposal for the rest. A call is
named by its capability. `tasks.available` is judged as it usually runs,
syncing notes repositories; the bench row above times it with `--repo /`.

| Command | Budget (p90) | Session start |
| --- | --- | --- |
| `get` | 25 ms | no |
| `put` | 150 ms | no |
| `query` | 100 ms | no |
| `plans.open` | 250 ms | no |
| `plans.waiting` | 100 ms | yes |
| `orders.pinned` | 100 ms | yes |
| `rules.check` | 100 ms | yes |
| `rules.for-path` | 500 ms | no |
| `tasks.available` | 1,500 ms | no |
| `changes.open` | 1,500 ms | no |
| `changes.next` | 10,000 ms | no |

A session-start command whose run is killed counts as far too slow, as
one over three times its budget does.
