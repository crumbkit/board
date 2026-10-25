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
| full index, cold | 1 s |
| `board call tasks.available --repo /`, warm | 500 ms |
| `board put` of a small record, warm | 300 ms |

`cargo xtask bench` is the timing job. It runs on a generated board in CI's
slow lane, on Linux nightly, on macOS on Monday nights, and on both before each release. It reports every run and fails above three times a budget. A pull request that takes an operation past its budget is caught by the next nightly run on Linux, or by the next Monday run when only macOS is slow. In nextest, `xtask/tests/bench.rs` runs the real bench on a 20-record board without `uv` and checks its six rows against three times each budget; a board that small does not show a regression. The owner times the
private export locally with `--board`. The work-count tests are the hard gate.

The plugin call row calls this repository's tasks plugin in process, as the
other rows run, and Board starts the plugin through `uv`. The generated board
enables tasks and holds 100 open tasks; one in five waits on another. The row
calls `tasks.available` with `--repo /`, so it syncs no mount and reads no git
remote. The call that syncs notes repos waits on git and their remotes, and has
no budget. The row times the snapshot, the plugin's start, and the `board query` and `board refs`
runs the plugin makes with the `board` binary beside xtask. The first call is discarded,
because it extracts and Python compiles the plugin's bytecode. Bench skips the
row, and says why, when the board does not enable tasks or `uv` is not on
`PATH`. With `uv`, a missing `board` beside xtask fails bench with
`xtask::bench::no_board`. Bench does not rebuild `board` and cannot tell a
stale one, so build board-cli in xtask's profile first, as the timing job does:
`cargo build --release --package board-cli`. `bin/test` gives bench no `uv`,
so the gates need neither `uv` nor a network.

The generated board commits its records in one commit, so git's index and
HEAD hold them as a real board's do; a put pays for both, and a put into a
board whose records are untracked takes a fifteenth of the time. The put row
writes a new small record for each run, committed as the board's own
identity. On a board given with `--board`, bench skips the row and says why,
because bench writes only a board it generated.

Targets are the speed an agent loop or a web view needs. None is met or
enforced yet:

| Operation | Target |
| --- | --- |
| `board check`, warm, nothing changed | 100 ms |
| simple `board query`, warm | 100 ms |
