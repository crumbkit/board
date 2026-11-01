# inflight

A Board plugin with one read-only query that answers "what is open or
running right now?". After a usage stop, a new session rescues the work in
flight by running it and reading what it prints. The plugin writes nothing
and ships no schema.

## What it reads

- Changes from `changes.open`, and each change's next step and actor from
  `changes.next`, both through `board call`, so the step rules live only in
  the changes plugin (plugins/changes/README.md).
- Subagents from the `runs/agent/v1` records the runs mod writes at
  `/agents/<id>`, and whether each stopped from the `runs/run/v1` records
  whose `agent` names it (plugins/runs/README.md).
- Pull requests from each change's `changes/opened/v1` record.
- Artifacts from the `artifacts/version/v1` records whose `for` names a
  change (plugins/artifacts/README.md).
- Questions from `plans.open` and evals from `evals.open`, through
  `board call`.

The board needs the changes, controls, plans and evals plugins enabled,
since `changes.next` calls `controls.check`. The runs and artifacts records
are read only by type, so a board without them has no agents or artifacts
to show.

## The query

    board --personal call inflight.now [--holder <session>] [--stopped-at <time>] [--limit <n>]

It returns four parts. Each returns at most `limit` entries, 20 by default
and at most 100, and counts in `left_out` the entries the limit left out.

- `changes`: the open changes, by path. Each gives its `title`, `repo`,
  `branch`, `holder` and `worktree` from its newest claim, `next` and
  `actor` from `changes.next`, its `pull_request` URL when an `opened`
  record exists, its `agents` and its `artifacts`. When `changes.next`
  refuses, `next` and `actor` are null and `refused` gives the code and
  message. `unreadable` counts the changes Board marks degraded.
- `unattached`: the agents with no `works_on`, or whose `works_on` names no
  open change, such as a change that landed. `unreadable` counts the agent
  records, here or under a change, that Board marks degraded or that lack a
  `started` time with a zone.
- `questions`: the entries of `plans.open`, the questions waiting on the
  owner. A decision awaiting review appears once, through its question.
  `unreadable` counts the open questions `plans.open` could not read.
- `evals`: the entries of `evals.open`, the evals planned or running.

When `plans.open` or `evals.open` refuses, as when the plugin is not
enabled, that part is empty and its `refused` gives the code and message.

An agent is listed when no run names it. It is `running`, or `stalled` when
`--stopped-at` is given and it started before that time, since nothing will
write its run after its session stopped. Agents are listed oldest first,
each with its `session`, `cwd`, `started`, `works_on` and `prompt`, the
path of the prompt it was started with. Each change lists at most `limit`
agents and artifacts, and counts the rest in `agents_left_out` and
`artifacts_left_out`. An artifact gives its `path`, `url`, and the `title`
and `at` of its newest version, newest first.

`--holder` keeps only the changes whose newest claim names that session,
their agents, and the unattached agents that session started. It takes the
session id as the claims and agent records hold it: the desktop app's
`local_` id when there is one, else the transcript id
(docs/plugins/claude-code/runs.md).

The query makes no network call, so it cannot say that a pull request
merged. A change with an `opened` record gives `pr_state`, the command that
checks it: `gh pr view <url> --json state`. A change that merged has a
`landed` record and is no longer open.

`now` runs `changes.next` once per change it lists, each a plugin call of
its own, so its capability has a timeout of 120 seconds rather than 30.

## Rescue

After a usage stop, a new session runs:

    board --personal call inflight.now --stopped-at <time>

For each change it lists, the session:

1. goes to the change's `worktree`. Uncommitted work there is still there,
   since a usage stop ends the session and leaves its files;
2. writes a claim from there with `changes.claim`;
3. runs `changes.brief` and restarts the `next` step.

A stalled agent's prompt is at its `prompt` path, so the session can start a
replacement with the same brief. The session reads nothing outside the
board to learn what was in flight.

## Installing

    uv sync --frozen --no-dev
    ln -s "$PWD" "${XDG_DATA_HOME:-$HOME/.local/share}/board/plugins/inflight"
    board plugin enable inflight

## Developing

The gates, which CI runs: `uv run ruff format --check .`, `uv run ruff check .`,
`uv run pyright` and `uv run pytest`. The tests drive `$BOARD_BIN`, by default
the workspace's `target/debug/board`, so build it first with
`cargo build -p board-cli`. They install the changes and controls plugins
from this checkout, syncing their environments once. `plugin.json` is
generated from `src/board_inflight/models.py` by `board_inflight.manifest`,
and a test checks that the committed file matches.
