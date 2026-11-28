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

Board marks `board/` as a cache when it opens an index or appends a line: it
writes `CACHEDIR.TAG` there, after excluding the directory from Time Machine
on macOS. A directory Board cannot mark usually gives the warning
`board::cache::unmarked`; a run a signal ends does not. The next run tries
again.

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
`0.0.0+45aafa9e1b2c.dirty.3f0c2a9b8d7e6f15`. While `BOARD_BUILD_COMMIT` is
unset, a build outside a git checkout has no suffix, and keeps no decisions.
board-cli's build script asks git, and cargo runs it again after each commit,
so board-cli recompiles after every commit.
A build outside the checkout, such as in a copy with its own git repository
or from a source tarball, sets `BOARD_BUILD_COMMIT` to the commit the source
was taken from. An empty value counts as unset. The build script then names
that commit in place of git's: the first 12 of its 7 to 40 hex digits (all of
them when fewer), lowercased. It fails on any other value. Setting the
variable asserts that the source is that commit. The `.dirty.` part still
comes from the local git. A copy whose own repository committed the snapshot
reads as clean, and a build with no local git cannot check for edits and also
reads as clean, so either shares decisions with every build that names the
same commit. Cargo also reruns the build script when `BOARD_BUILD_COMMIT`
changes.
`board --version` prints `board` and this same version.

| Phase | What it times |
| --- | --- |
| `parse` | parsing the command line |
| `open` | finding and opening the board |
| `plugin.list` | reading the enabled plugins' manifests, for `commands` and `schemas` |
| `resolve` | resolving the path that `get`, `ls`, `refs` or a scoped `check` names |
| `index.open` | opening the index |
| `snapshot` | refreshing the index and deciding every entry, for `check`, `ls`, `query` and `refs` |
| `index.refresh` | the refresh, within `snapshot` |
| `index.refresh.load` | loading the working-tree rows the last refresh stored |
| `index.refresh.walk` | walking the working tree, listing only the directories that changed ([warm/index.md](warm/index.md)) |
| `index.refresh.sync` | storing what the walk found that differs from those rows |
| `index.refresh.facts` | loading or adding each entry's facts |
| `snapshot.mounts` | walking each mounted commit, within `snapshot` |
| `snapshot.head` | listing `HEAD`'s trees, within `snapshot.decide`, when the working tree is decided |
| `snapshot.decide` | deciding every entry, its references and pointers, then storing the mounts and pruning the index |
| `snapshot.commit` | committing the index |
| `view` | filling the views with what the snapshot decided, for `query` |
| `query.sql` | running the SQL statement |
| `query.slow` | running a statement a Board command ran for itself, such as a plugin's query through the host, when it took longer than the 1 second an agent's `board query` may take; such a statement may take 5 seconds |
| `plugin.call` | running a capability |
| `plugin.reply` | from starting a plugin process to the first byte of its reply, once for each process that replied |
| `put.validate` | checking a record before it is written |
| `put.write` | writing the records and committing |
| `emit` | printing the output |

`work` holds the counts a run has: `entries`, `hashed`, `parsed`, `validated`,
`trees`, `rows`, `target_changed`, how many targets `refs --field` left
unread because they changed after the check (it does not name them),
`started`, the plugin processes a call started, `capabilities`, the
capabilities `commands` or `schemas` listed, `repositories`, the
repositories a snapshot opened from disk besides the board's own, `kept`,
the mounted sources a query or check read from kept decisions without
walking them (architecture/performance/warm/decisions.md), `decided`, the entries
a snapshot decided in place of reading kept rows, `walked`, the mounted
sources a snapshot walked from the index to find their mounts or decide
them, and `pointers_read`, the files `snapshot.decide` read to learn whether
they are pointers, once per blob in each source. It leaves out a file whose
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

A record also holds `load1`, the machine's one-minute load average when it
ended (from `vm.loadavg` on macOS, `/proc/loadavg` on Linux), `cpus`, its
logical cores, and `killed`. A run that SIGINT, SIGTERM or SIGHUP ends, such
as a hook that times out, writes its line with `killed: true` and a null
`exit` before it ends as the signal would. `serve` and `ui` stop on those
signals themselves and write no such line. A run that SIGKILL ends, or that
crashes, writes nothing.

A run installs that signal handler once it has parsed its command line. A
signal that lands earlier ends the run with no line. A test that signals a
run sets `BOARD_TIMING_READY=1`, and the run then prints
`board: timing handler ready` to stderr once the handler is installed; the
test reads stderr for that line before it sends the signal. The run prints
nothing when the variable is unset or has another value, when
`BOARD_TIMINGS` is `0`, or when the handler could not be installed. Only
tests set the variable, but a plugin inherits it, so a run a plugin starts
inside such a test prints the line too.

Before a line would take the log past 1 MiB, the run that takes the
rotation lock (`timings.lock`, a kernel lock that a killed run releases)
summarises the log into `timing-digest.jsonl`, then renames the log to
`timings.jsonl.1`, replacing the one before. A run that finds the lock held
appends without rotating. Recording happens after the output is printed. A
record that cannot be written is dropped silently, and the run's output and
exit code stay as they were. Read the log with `jq`. The `root` of each
record shows the names of your directories, so check the log before you
attach it to a bug report.

## The digest

Each line of `timing-digest.jsonl`, tagged `board/timing-digest/v1`, sums
one command's top-level runs on one board (`root`) in one UTC hour
(`start_ms`) of one rotated log, named by the `at_ms` of its first line
(`from_ms`). It counts the `runs`, those `killed` and those `judged`, and
keeps the last 20 judged times in `judged_us`. A run is judged when it was
not killed and `load1` was no higher than `cpus`. Rotation rewrites the
digest whole, keeping its lines that parse, so a line a crash cut short is
dropped. A log whose `from_ms` the digest already holds adds nothing, so a
rotation run again after a crash counts no hour twice. The digest keeps, for
each command on each board, its newest hours until they hold 20 judged
times, every hour of the day before its newest hour, and at most 2,000
hours, about 600 KB. Past that limit the oldest hours go first; the check
reads no further back, so dropping them changes no judgement. A digest that cannot
be written leaves that log's hours out, and the log still rotates.

## The speed check

`board timing check` judges each command in budgets.md's Command budgets
on the personal board: the p90 of its last 20 judged top-level runs, from
the digest's hours of logs no longer kept and then both logs, and their
median where the command has a median budget. A call is named by its
capability. With fewer than 5 judged runs a command is unjudged. A command
over a budget is over; one over three times it, or a session-start command
with a run killed in the last day, is far too slow. The check prints a row
per command with its budgets, p50, p90 and mode, how many lines it could
not parse, and any warnings.

With `--record` it writes the suite `/suites/board-speed-local` (a
`runs/suite/v1` of kind `benchmark`) if absent, imports the newest CI run,
decides each command's mode, then writes a `runs/run/v1` with a result per
command, each naming its `mode`, and stamps `timing-check.stamp` in the
cache. Within an hour of the stamp it writes nothing. The run's `setup`
names the machine's host name, and `dirty` says whether the build's
version has `.dirty.`. Its measures `over_budget` and `killed` count only
live commands; `silent_over_budget` and `silent_killed` count the rest.
The rules mod starts `board timing check --record --session <id>` detached
at each session's start, with the `local_` id the desktop app names, in a
session or process group of its own (`setsid -f` where present, else
`set -m`) and under nohup so it ignores SIGHUP; `board timing` keeps that
ignore. A SIGTERM or SIGKILL sent to the check's own process ends it with
no run written; the next session's check writes one, as the stamp is not
yet set.

### The CI timing run

On each push to main, timing.yml's hot job runs `cargo xtask bench --hot
--json` on Linux (budgets.md, Command budgets) and uploads the run as the
artifact `board-speed-run`. The check finds the newest successful run with
`gh run list`, through board-plugin's process module, and when
`/suites/board-speed-ci/runs/<run id>` is absent downloads it with `gh run
download` into a scratch directory under the cache and writes it there,
with the suite if absent. On import `over_budget` counts the commands live
then. A missing or signed-out gh, an offline machine or an artifact that
does not parse is a warning, and nothing is imported; the local run is
still written.

### Silent and live

A command is silent until it is ready, then live. It is ready when it has
100 judged results in local runs, this check's included, and, once CI has
timed it at all, 14 judged results in CI runs, and fewer than 1 in 10 of
its over-budget results were cleared by its next judged result. A command
whose newest CI result is an `error` waits until CI times it again. The
check reads the newest 120 runs of each suite, so flapping only in older
runs is missed. A command the newest local run that names modes names live
stays live, so live holds across Macs and across a run from a build that
names no modes, and no event makes it silent.

When the first command goes live the check writes `/rules/board/speed-budgets`
and `/rules/board/speed-budgets-ci` if absent: zero rules on each suite's
`over_budget` and `killed`, one per suite since a zero rule reads one suite,
so the rules badge counts a live command over budget in every session.

For each live command over budget the check opens a task at
`/tasks/board-speed-<command>.md` naming the run, if absent; reopens a task
whose status is not `open` in place, naming this run, so each overflow has
its own opening run; gives an open task a title that says so when the
command is far too slow, which the lead names in its next status to the
owner; and files a `plans/question/v1` at
`/projects/board/questions/board-speed-<command>-<opening run>`, only if
absent, once the grace has passed. Each overflow asks at most once.

### The grace

The grace is the lines added plus removed (`git log --numstat`) on the
branch the `/repos/board` mount follows, in the clone it names, since the
run that opened or reopened the task started. The check never fetches that
clone, so the count grows only as something else fetches it. It passes when they exceed the
`overflow-agent-grace` param for `/repos/board`, read through
`params.get`, or its default of 200 lines. The question names the session
in `--session` as its `by`, and `blocks` the task. No question is filed
when the check has no `--session`, git cannot count the lines, the board
mounts no `/repos/board`, or another speed question is open: one whose
status is `open` and whose id has no answer.

### Lifecycle

| From | Event | To | Owner |
| --- | --- | --- | --- |
| a run running | it returns | one line, `killed: false` | the run |
| a run running | SIGINT, SIGTERM or SIGHUP | one line, `killed: true`, then it ends | the run's signal thread |
| a run running | SIGKILL or a crash | no line | the system |
| log under 1 MiB | a line would pass 1 MiB, lock taken | digest rewritten, log renamed | the rotating run |
| log under 1 MiB | the lock is held | the line appended, no rotation | the run |
| no stamp, or one over an hour old | `timing check --record` | suite if absent, CI run imported, a run, the stamp | the check |
| stamp under an hour old | `timing check --record` | nothing written | the check |
| none | a push to main | the hot job queued on Linux | GitHub Actions |
| hot job queued or running | a newer push | cancelled, no artifact | Actions concurrency |
| hot job running | bench finishes | artifact uploaded; the job passes | the hot job |
| hot job running | 15 minutes pass, or the runner fails | failed, no artifact | GitHub Actions |
| newest artifact not imported | a recording check | run written at its run id, if absent | the check, through gh |
| newest artifact imported | a recording check | nothing written | the check |
| gh or the artifact fails | a recording check | a warning, nothing imported | the check |
| silent | a check finds it not ready | silent; its result recorded | the check |
| silent | a check finds it ready | live, named in this run's results | the check |
| live | any later check | live | the check |
| no speed rules | the first command goes live | both rules, if absent | the check |
| live, over, no task | a check | its task, if absent | the check |
| task open | far too slow | the task retitled | the check; the lead names it |
| task done or dropped | the live command is over again | the task reopened in place, naming this run; no question this check | the check |
| task open | the grace passes since its opening run, a session asks, no speed question open | that overflow's question, if absent | the check |
| task open | the grace passes without those | no question; the next check tries | the check |
| question open | the owner answers | the budget changed or retired in budgets.rs | the owner |
| task open | the fix lands and the command is within | done through `tasks.done` | the session that lands it |

Crash points: a run killed while appending leaves a partial line, which
the check skips and counts. A rotation killed after the digest and before
the rename rotates again, and the digest skips the log it holds. The hot
job killed mid-bench uploads nothing; the next push runs again. A check
killed during the download writes nothing and leaves its scratch directory
in the cache; the next check imports the same artifact. One killed after
the CI import and before the local run leaves the import, which the next
check skips. One killed after the local run that first names a live
command and before the rules, a task or a question leaves those absent,
and the next check writes them. One killed after the run and before the
stamp records one more run within the hour. Two checks at once may both
record a run, or both import or ask; each import, task, rule and question
has one path, a question one per overflow, and is written only if absent
or, for a closed task, only over the blob it read, though two different
commands' questions may then open together.

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
