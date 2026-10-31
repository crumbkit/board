# The timing log

This file holds detail for docs/architecture.md. It says what each `board` run
records about its own timing, and where.

Every `board` run whose command line parses appends one JSON line, tagged
`board/timing/v1`, to `$XDG_CACHE_HOME/board/timings.jsonl` (by default
`~/.cache/board/`), beside the boards' indexes. Help, `--version`, completion
and a refused command line record nothing. The log is created readable only by
its owner (mode 0600). `BOARD_TIMINGS=0` turns recording off. A plugin inherits
`BOARD_TIMINGS` and `XDG_CACHE_HOME`, so the runs it starts record to the same
log.

A record holds the start time (`at_ms`), Board's `version`, the subcommands
run (`command`, such as `plugin find`, never the values given), the
`capability` a `call` ran as `<plugin>.<capability>` (null for any other run,
and never the call's input), the plugin call
`depth`, the board's absolute path (`root`), the logical path the run
started in when a known board's mount holds it (`prefix`, otherwise null), the `exit` code and `error` code,
`total_us`, and `phases`. Each phase gives its
`name`, its start after the run's (`at_us`), its time (`us`) and how often it
was `entered`, in the order the phases started. A record has only the phases
its run entered.

The `version` is board-cli's version, then `+` and the 12-character commit
it was built from, then `.dirty.` and a hash of the changes when `crates/`,
`Cargo.toml`, `Cargo.lock` or `rust-toolchain.toml` differed from that
commit, untracked files included, such as
`0.0.0+45aafa9e1b2c.dirty.3f0c2a9b8d7e6f15`. A build outside a git checkout
has no suffix, and keeps no decisions. board-cli's build script asks git, and cargo runs it again
after each commit, so board-cli recompiles after every commit.
`board --version` prints the version alone.

| Phase | What it times |
| --- | --- |
| `parse` | parsing the command line |
| `open` | finding and opening the board |
| `resolve` | resolving the path that `get`, `ls`, `refs` or a scoped `check` names |
| `index.open` | opening the index |
| `snapshot` | refreshing the index and deciding every entry, for `check`, `ls`, `query` and `refs` |
| `index.refresh` | the refresh, within `snapshot` |
| `index.refresh.load` | loading the working-tree rows the last refresh stored |
| `index.refresh.walk` | walking the working tree |
| `index.refresh.sync` | storing what the walk found that differs from those rows |
| `index.refresh.facts` | loading or adding each entry's facts |
| `index.refresh.head` | listing `HEAD`'s trees |
| `snapshot.mounts` | walking each mounted commit, within `snapshot` |
| `snapshot.decide` | deciding every entry, its references and pointers, then storing the mounts and pruning the index |
| `snapshot.commit` | committing the index |
| `view` | filling the views with what the snapshot decided, for `query` |
| `query.sql` | running the SQL statement |
| `plugin.call` | running a capability |
| `plugin.reply` | from starting a plugin process to the first byte of its reply, once for each process that replied |
| `put.validate` | checking a record before it is written |
| `put.write` | writing the records and committing |
| `emit` | printing the output |

`work` holds the counts a run has: `entries`, `hashed`, `parsed`, `validated`,
`trees`, `rows`, `target_changed`, how many targets `refs --field` left
unread because they changed after the check (it does not name them),
`started`, the plugin processes a call started, `repositories`, the
repositories a snapshot opened from disk besides the board's own, `kept`,
the mounted sources a query read from kept decisions without walking them
(architecture/performance/decisions.md), `walked`, the mounted sources a
snapshot walked from the index to find their mounts or decide them, and `pointers_read`, the files `snapshot.decide` read to learn whether they
are pointers, once per blob in each source. It leaves out a file whose
facts in the index say it is JSON that parsed and has no `$type`, or a
valid one other than a pointer's, because such a file holds no pointer. It
also leaves out the pointers read while walking mounts and while resolving
a path before the snapshot. When another process holds the index, a snapshot runs again: the
second run waits for the index's write lock, and runs in memory only if that
wait times out. The counts are those of the run that answered, except
`repositories`, which counts each repository opened in either run once. `plugin.reply` includes the
plugin's own work, such as the `board query` it runs, which records its own
line one `depth` deeper. The runs that refused a changed target, from both log files:
`cat ~/.cache/board/timings.jsonl.1 ~/.cache/board/timings.jsonl 2>/dev/null | jq -c 'select(.work.target_changed) | {at_ms, root, n: .work.target_changed}'`.

Before a line would take the log past 1 MiB, the log is renamed to
`timings.jsonl.1`, replacing the one before. The rename takes no lock, so two
runs that fill the log at once can lose the older file. The run that renames
second also loses its own record, because the log it tried to rename is
already gone. Recording happens after the output is printed. A record that cannot be written is dropped silently, and
the run's output and exit code stay as they were. Read the log with `jq`. The
`root` of each record shows the names of your directories, so check the log
before you attach it to a bug report.

## Reading the log and profiling

The median time of each phase, and of the whole run as `total`, over the
last 20 top-level runs of `query` (use
`select(.capability == "tasks.available" and .depth == 0)` for a call):

    cat ~/.cache/board/timings.jsonl.1 ~/.cache/board/timings.jsonl 2>/dev/null |
      jq -s '[.[] | select(.command == "query" and .depth == 0)] | .[-20:]
        | [.[].phases[]] + map({name: "total", us: .total_us}) | group_by(.name)
        | map({name: .[0].name, runs: length, ms: ((map(.us) | sort)[length / 2 | floor] / 1000)})'

Check `version` first, so that runs of an older build do not mix in.

`cargo xtask profile [--save <file>] -- <board arguments>` builds `board`
in the `profiling` cargo profile, which is release with line tables, runs
it once to warm the index, then records 20 runs at 4,000 samples a second
under [samply](https://github.com/mstange/samply), which must be on `PATH`;
without it the command fails with `xtask::profile::no_samply`. samply then
opens the Firefox Profiler. With `--save`, samply writes the profile to the
file instead, and `samply load <file>` opens it later. The runs use the
environment xtask runs in, so set `XDG_CACHE_HOME` to keep them out of
your own index and log.
