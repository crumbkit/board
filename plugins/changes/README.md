# changes

A Board plugin that walks an agent through one change to a mounted
repository, from its plan to its pull request, and gathers every record the
change produces. SKILL.md holds the procedure for each phase.

## Records

`changes/change/v1` at `/changes/<name>.json`. Its plan and other files
sit in `/changes/<name>/`. Git history holds earlier phases.

```json
{
  "$type": "changes/change/v1",
  "title": "Let a suite declare a timeout",
  "mount": "/repos/board",
  "phase": "build",
  "route": "owner",
  "plan": "/changes/suite-timeout/plan.md",
  "head": "a3f9c0d",
  "records": ["/tasks/suite-timeout-schema", "/suites/board-tests/runs/20261025T101500Z-a3f9"]
}
```

- `title`, `mount` and `phase` are required.
- `phase`: `plan`, `build`, `review`, `land`, `done`, or `dropped` to end a
  change early.
- To drop a change, `board put` it with `phase` set to `dropped`.
- `route`: `owner` (nobody else owns a CODEOWNERS pattern), `ask-first`
  (someone else does) or `unowned` (no CODEOWNERS pattern names anyone).
- `mount`, `plan` and each of `records` are references, so `board check`
  reports one that reaches nothing.
- `issue` and `pull_request` are URLs. `head` is the commit under test and
  review.

`changes/plan/v1` at `/changes/<name>/plans/<stamp>.json`, beside its text
at `<stamp>.md`. The newest stamp is the change's current plan.

```json
{
  "$type": "changes/plan/v1",
  "text": "/changes/suite-timeout/plans/20261101T120000000000Z.md",
  "uses": ["/repos/board/STANDARDS.md", "/controls/rules/pair-refusals", "/evals/board-standards-selection"]
}
```

- `text` and each of `uses` are references. `uses` names the standards
  documents, controls, decisions, evals and other changes the plan relies
  on.

## Capabilities

- `start --mount /repos/<name> --title '<title>' [--name <name>] [--handle @<you>]`
  creates the change in `plan` and a plan with empty Problem, Outcome and
  Out of scope sections. It reads `CODEOWNERS` (`.github/`, root, `docs/`,
  the first found) to set the route and list `ask_first` patterns. `docs`
  lists the contributing docs found (`CONTRIBUTING.md` at the root, in
  `.github/` or in `docs/`, and `AGENTS.md`). `suite` is the first
  inline-code path in them that holds a `runs/suite/v1`, looked up in the
  repository's own board first. Refuses an existing name
  (`changes::change::exists`) and a repository path holding nothing
  (`changes::mount::not_found`).
- `next --path /changes/<name>`: read-only. The phase, each gate check as
  `pass` or `missing` with a detail, and the instruction.
- `attach --path /changes/<name> [--ref <path>] [--route <route>] [--head <sha>] [--issue <url>] [--pull-request <url>]`
  adds a reference once or sets the fields. Refuses a reference that reaches
  nothing (`changes::ref::dangling`).
- `advance --path /changes/<name>` moves to the next phase when the gate
  passes. Otherwise refuses with `changes::gate::unmet`, whose `missing`
  lists the failed checks. Leaving `done` or `dropped` is
  `changes::phase::final`, which points to `open` and `start`.
- `open [--limit <n>]`: read-only. Changes not `done` or `dropped`, by path,
  at most 100 by default, with `total`, `truncated` and `unreadable`.

- `pr --path /changes/<name>` drafts the pull request. It writes the body
  to `/changes/<name>/pr.md`, replacing an earlier one, and returns `path`,
  `command`, `pull_request` and `unreadable`. It never opens the pull request, never runs
  `gh` or git, and makes no model calls. See "The pull request body".

- `plan` takes `{path, text, uses}` on stdin, since `uses` is an array. It
  writes the plan record and its text and points the change's `plan` at the
  text, in one write, in any phase but `done` and `dropped`
  (`changes::phase::final`). It refuses a reference that reaches nothing or
  is degraded (`changes::ref::dangling`, whose `refs` lists them), a text over
  3,000 words (`changes::plan::too_long`), and Markdown documents in `uses`
  over 3,000 words in all (`changes::plan::uses_too_long`). Words are counted
  as `wc -w` counts them. The second limit comes from
  `/evals/board-standards-selection` in the owner's board: agents that chose
  standards from each directory's `index.md` stayed within it.
- `brief --path /changes/<name>`: read-only. The newest plan's text path,
  one line per record it uses (a control's `summary`, else its
  `requirement`; a decision's `decision`; an eval's question and verdict; a
  document's first heading), `left_out`, the phase and instruction `next`
  gives, and the `controls.check` command for the change's mount. It holds at
  most 600 words, counting each line with its reference, and `left_out`
  counts the lines it dropped.

A path that holds no change is `changes::change::not_found`; a degraded
change is `changes::change::degraded`.

Each gate reads only the change, the records it references, its plan file
and, in `build`, the repository's contributing docs. A referenced record
Board marks degraded satisfies nothing.

| Leaving | Gate |
| --- | --- |
| plan | Each plan section has text, and `route` is set. On `ask-first`, also a `plans/decision/v1` whose `refs` name the plan or the change. Any such decision passes, including a rejection. |
| build | Every attached `tasks/task/v2` is `done` or `dropped`. A repeating task never reaches `done`, so drop it to release the gate. An attached `runs/run/v1` under `<suite>/runs/` of the check suite is full, clean, has no `fail` or `error`, and names `head` in `versions`. |
| review | An attached `review/report/v1` whose `range` ends at `head`, whose `verdict` is not `changes`, and which has no `critical` or `high` finding. |
| land | `pull_request` is set. Advancing enters `done`. |

Commits match when one is a prefix of the other and both have at least 7
digits.

## The pull request body

`pr` returns:

- `command`: `gh pr create --title '<title>' --body-file '<file on disk>'`,
  quoted for a POSIX shell. Run it from a checkout of the repository on the
  change's `head`. The title is the change's `title` with its whitespace
  collapsed to single spaces.
- `pull_request`: when the change records one, its URL. `command` is then
  `null` and `pr.md` is not written.
- `unreadable`: the references that reach nothing or that Board marks
  degraded.

`pr` reads every reference, in any phase. A reference degraded only by a
broken reference of its own is still read.

| Section | From |
| --- | --- |
| (first) | The plan's Outcome. |
| Why | The plan's Problem. |
| Out of scope | The plan's Out of scope, when it has text. |
| What the tests prove | The plan's section of that name, then each attached `runs/run/v1`: suite, scope, results by outcome, summary. |
| What the tests do not prove | The plan's section of that name, then each run's limits: targeted, not clean, not on `head`, and each case that failed, errored or was skipped. Says so when no run is attached. |
| Review | Each attached `review/report/v1`: verdict, author, range, findings by severity. |
| Evidence | The change, the plan, and every reference with its type (`file` for a file that is not a record, `directory` for a directory), or the code that hid it. |
| `Closes <issue>` | The change's `issue`, when set. |

Plan headings match without case. A plan section holds its subsections, and
a `#` line inside a code fence is not a heading. An empty plan section
becomes a line that names it. The plan's sections are copied as written.
Text from a referenced record is put on one line, so it cannot add a heading
or a list item. Paths and free text (a run's suite and summary, a case and
its note, a report's author) go in inline code; empty text shows as
`(empty)`. Each run names at most 20 failed, errored or skipped cases and
counts the rest. `Closes` is left out when the
issue holds whitespace. Evidence is listed by logical path, since a board path
has no URL.

## Installing

    uv sync --frozen --no-dev
    ln -s "$PWD" "${XDG_DATA_HOME:-$HOME/.local/share}/board/plugins/changes"
    board plugin enable changes

Enabling writes the schema to `.board/types/changes/` and names the
plugin in `.board/board.json`. The type stays at v1 and changes in place while
the plugin has one user; after such a change, enable it again.

## Developing

The gates, which CI runs: `uv run ruff format --check .`, `uv run ruff check .`,
`uv run pyright` and `uv run pytest`. The tests drive `$BOARD_BIN`, by default
the workspace's `target/debug/board`, so build it first with
`cargo build -p board-cli`. `plugin.json` and the shipped schema are generated
from `src/board_changes/models.py` by `board_changes.manifest`, and
a test checks that the committed files match.
