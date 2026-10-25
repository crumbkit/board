# runs

A Board plugin for recording what test suites, benchmarks and evals gave each
time they ran. A suite says what gets run. Each run of it is a separate
record holding curated results. Raw output, such as `junit.xml`, stays outside
the board.

## The suite record

A suite is a file `/suites/<name>.json`, and its name is `<name>`. Its runs
sit in `/suites/<name>/runs/`.

```json
{
  "$type": "runs/suite/v1",
  "subjects": ["board"],
  "method": "Time each operation over a 10,000-file board; take the median of 20 runs.",
  "command": "cargo xtask bench",
  "cases": [{"id": "get"}, {"id": "put"}],
  "measures": [{"name": "median_ms", "unit": "ms"}]
}
```

- `subjects` and `method` are required.
- `subjects` are the names of what is tested, without versions.
- `cases` lists the cases inline: each has an `id`, which a run's results name,
  and optional `input` and `expected`. `cases_file` references a file that
  lists them instead. A suite whose cases are found when it runs, such as a
  test suite, has neither. The schema refuses both.
- `measures` declares what a run records: a `name`, a `unit` and optionally a
  `description`.

## The run record

A run is a file `/suites/<name>/runs/<id>.json`. The id is a sortable UTC
timestamp and a short random suffix, such as `20261018T101500Z-a3f9`.

```json
{
  "$type": "runs/run/v1",
  "suite": "/suites/board-tests",
  "by": "claude",
  "started": "2026-10-18T10:15:00Z",
  "finished": "2026-10-18T10:16:12Z",
  "scope": "full",
  "dirty": false,
  "versions": {"board": "38a3b7e"},
  "host": {"os": "macos", "arch": "aarch64", "logical_cpus": 12},
  "measures": {"tests": 412, "failures": 1, "errors": 0, "skipped": 2},
  "results": [
    {"case": "sync::pushes_after_replay", "outcome": "fail", "note": "push refused: the remote moved during the test"}
  ]
}
```

- `suite`, `by`, `started`, `scope` and `results` are required.
  `suite` must reach a `runs/suite/v1` record, or `check` reports
  `board::ref::dangling` or `board::ref::wrong_type`.
- `started` and `finished` carry a time zone.
- `scope` is `full` when every case ran and `targeted` when some did.
  `dirty` says whether the tree under test had uncommitted changes. A run
  that leaves it out is never counted as clean.
- `versions` maps what was used, such as a commit, a model or a library, to
  its version. `host` names the machine, with all three fields.
- `measures` holds the run's own measures, such as the counts above.
- `results` lists only the cases worth keeping: each case that did not pass
  (`fail`, `error` or `skipped`), with a `note` saying why, and each case that
  measured something, such as a benchmark's operation. The schema refuses a
  case that did not pass without a note, and a result with neither an
  `outcome` nor `measures`. A passing case without measures is left out,
  except that an eval's run may list every case.
- A result's `trial` numbers repeated trials of one case, from 1, and its
  `evidence` references raw files kept in the board, such as an eval's logs.
- `setup` says how the run was prepared and `summary` what it showed, both
  as prose.
- `written_by` names the plugin and hook that wrote the run, such as
  `runs/subagent-stop`, when a hook wrote it without an agent. A run an agent
  or a person wrote leaves it out.

## Capabilities

Both capabilities count only the full runs of a clean revision: `scope` is
`full` and `dirty` is false. Each reports how many runs of the suite it left
out for being targeted, dirty or silent about `dirty` in `not_counted`, and in `unreadable` how many
it left out because Board marks them degraded for anything but a broken
reference or the plugin cannot read them. Each takes the suite by its
logical path, which may be in a mounted board, and refuses a path that holds
no suite with `runs::suite::not_found`.

`board call runs.latest --suite /suites/board-tests` returns the newest such
run by `started`, a tie going to the run whose path sorts last. It gives the run's `path`, `started`,
`finished`, `measures`, the cases that failed or erred in `failed`, each once, and
`outcome`: `fail` when any result failed or erred, else `pass`. `age_s` is the
whole seconds since the run finished, or since it started when it records no
finish. With no run that counts, `run` is `null`.

`board call runs.trend --suite /suites/board-bench --measure median_ms --case get`
returns one point per run that counts, newest first: its `path`, `started` and
the measure's `value`. With `--case`, the value is that case's measure,
taken from its lowest `trial` when the case ran several; without, it is the
run's own. `limit` is 1 to 1,000 (default 1,000). `total`
counts every point, `truncated` says whether the limit left some out, and
`without_measure` counts the runs that give no number for the measure.

Each capability reads the records under the suite's `runs/` directory with
one `board query`, so a run stored elsewhere is not counted. Flaky cases
have no capability: query the `records` view with `json_each` over
`$.results`.

## The Claude Code mod

`claude-code/` holds a Claude Code mod that records each subagent and task a
session delegates as a run, and each pull request it opens as a decision
stub. docs/plugins/claude-code.md says what it writes and how to load it.

## Installing

    uv sync --frozen --no-dev
    ln -s "$PWD" "${XDG_DATA_HOME:-$HOME/.local/share}/board/plugins/runs"
    board plugin enable runs

`board plugin enable runs` writes the two schemas to `.board/types/runs/` and
names the plugin in `.board/board.json`. The types stay at v1 and change in
place while the plugin has one user; after such a change, run
`board plugin enable runs` again.

## Developing

The gates, which CI runs: `uv run ruff format --check .`, `uv run ruff check .`,
`uv run pyright` and `uv run pytest`. The tests drive `$BOARD_BIN`, by default
the workspace's `target/debug/board`, so build it first with
`cargo build -p board-cli`. `plugin.json` and the shipped schemas are
generated from the models in `src/board_runs/models.py` by
`board_runs.manifest`, and a test checks that the committed files match.
