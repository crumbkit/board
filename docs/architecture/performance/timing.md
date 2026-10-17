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
run (`command`, such as `plugin find`, never the values given), the plugin call
`depth`, the board's absolute path (`root`), the `exit` code and `error` code,
`total_us`, and `phases`. Each phase gives its
`name`, its start after the run's (`at_us`), its time (`us`) and how often it
was `entered`, in the order the phases started. A record has only the phases
its run entered:

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
| `view` | filling the views with what the snapshot decided, for `query` |
| `query.sql` | running the SQL statement |
| `plugin.extract` | extracting records for one plugin, in `query` |
| `plugin.call` | running a capability |
| `plugin.reply` | from starting a plugin process to the first byte of its reply, once for each process that replied |
| `put.validate` | checking a record before it is written |
| `put.write` | writing the records and committing |
| `emit` | printing the output |

`work` holds the counts a run has: `entries`, `hashed`, `parsed`, `validated`,
`trees`, `rows`, `target_changed`, how many targets `refs --field` left
unread because they changed after the check (it does not name them), and
`started`, the plugin processes a call or a query's extraction started. Each
extraction batch and each call starts one. `plugin.reply` includes the
plugin's own work, such as the `board query` it runs, which records its own
line one `depth` deeper. In a `query`, it sums the replies of the extraction
batches. The runs that refused a changed target, from both log files:
`cat ~/.cache/board/timings.jsonl.1 ~/.cache/board/timings.jsonl 2>/dev/null | jq -c 'select(.work.target_changed) | {at_ms, root, n: .work.target_changed}'`.

Before a line would take the log past 1 MiB, the log is renamed to
`timings.jsonl.1`, replacing the one before. The rename takes no lock, so two
runs that fill the log at once can lose the older file. The run that renames
second also loses its own record, because the log it tried to rename is
already gone. Recording happens after the output is printed. A record that cannot be written is dropped silently, and
the run's output and exit code stay as they were. Read the log with `jq`. The
`root` of each record shows the names of your directories, so check the log
before you attach it to a bug report.
