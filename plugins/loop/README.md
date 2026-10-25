# loop

A Board plugin that walks an agent through one change to a mounted
repository, from shaping the request to learning from it, and gathers every
record the change produces. SKILL.md holds the procedure for each phase.

## Records

`loop/change/v1` at `/changes/<name>.json`. Its plan and other files
sit in `/changes/<name>/`. Git history holds earlier phases.

```json
{
  "$type": "loop/change/v1",
  "title": "Let a suite declare a timeout",
  "repo": "/repos/viitta",
  "phase": "build",
  "route": "owner",
  "plan": "/changes/suite-timeout/plan.md",
  "head": "a3f9c0d",
  "records": ["/tasks/suite-timeout-schema", "/suites/board-tests/runs/20261025T101500Z-a3f9"]
}
```

- `title`, `repo` and `phase` are required.
- `phase`: `shape`, `plan`, `build`, `review`, `land`, `learn`, `done`, or
  `dropped` to end a change early.
- To drop a change, `board put` it with `phase` set to `dropped`.
- `route`: `owner` (nobody else owns a CODEOWNERS pattern), `ask-first`
  (someone else does) or `proactive` (no owners named).
- `repo`, `plan` and each of `records` are references, so `board check`
  reports one that reaches nothing.
- `issue` and `pull_request` are URLs. `head` is the commit under test and
  review.

## Capabilities

- `start --repo /repos/<name> --title '<title>' [--name <name>] [--handle @<you>]`
  creates the change in `shape` and a plan with empty Problem, Outcome and
  Out of scope sections. It reads `CODEOWNERS` (`.github/`, root, `docs/`,
  the first found) to set the route and list `ask_first` patterns. `docs`
  lists the contributing docs found (`CONTRIBUTING.md` at the root, in
  `.github/` or in `docs/`, and `AGENTS.md`). `suite` is the first
  inline-code path in them that holds a `runs/suite/v1`, looked up in the
  repository's own board first. Refuses an existing name
  (`loop::change::exists`) and a repository path holding nothing
  (`loop::repo::not_found`).
- `next --path /changes/<name>`: read-only. The phase, each gate check as
  `pass` or `missing` with a detail, and the instruction.
- `attach --path /changes/<name> [--ref <path>] [--route <route>] [--head <sha>] [--issue <url>] [--pull-request <url>]`
  adds a reference once or sets the fields. Refuses a reference that reaches
  nothing (`loop::ref::dangling`).
- `advance --path /changes/<name>` moves to the next phase when the gate
  passes. Otherwise refuses with `loop::gate::unmet`, whose `missing`
  lists the failed checks. Leaving `done` or `dropped` is
  `loop::phase::final`, which points to `open` and `start`.
- `open [--limit <n>]`: read-only. Changes not `done` or `dropped`, by path,
  at most 100 by default, with `total`, `truncated` and `unreadable`.

A path that holds no change is `loop::change::not_found`; a degraded
change is `loop::change::degraded`.

Each gate reads only the change, the records it references, its plan file
and, in `build`, the repository's contributing docs. A referenced record
Board marks degraded satisfies nothing.

| Leaving | Gate |
| --- | --- |
| shape | Each plan section has text. No attached `plans/question/v1` is `open`. |
| plan | A `review/report/v1` whose `verdict` is not `changes` is attached. On `ask-first`, also a `plans/decision/v1` whose `refs` name the plan or the change. Any such decision passes, including a rejection. |
| build | Every attached `tasks/task/v1` is `done` or `dropped`. An attached `runs/run/v1` under `<suite>/runs/` of the check suite is full, clean, has no `fail` or `error`, and names `head` in `versions`. |
| review | An attached `review/report/v1` whose `range` ends at `head`, whose `verdict` is not `changes`, and which has no `critical` or `high` finding. |
| land | `pull_request` is set. |
| learn | None. |

Commits match when one is a prefix of the other and both have at least 7
digits.

## Installing

    uv sync --frozen --no-dev
    ln -s "$PWD" "${XDG_DATA_HOME:-$HOME/.local/share}/board/plugins/loop"
    board plugin enable loop

Enabling writes the schema to `.board/types/loop/` and names the
plugin in `.board/board.json`. The type stays at v1 and changes in place while
the plugin has one user; after such a change, enable it again.

## Developing

The gates, which CI runs: `uv run ruff format --check .`, `uv run ruff check .`,
`uv run pyright` and `uv run pytest`. The tests drive `$BOARD_BIN`, by default
the workspace's `target/debug/board`, so build it first with
`cargo build -p board-cli`. `plugin.json` and the shipped schema are generated
from `src/board_loop/models.py` by `board_loop.manifest`, and
a test checks that the committed files match.
