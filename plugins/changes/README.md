# changes

A Board plugin that records one change to a repository, from its plan to its
merge, as small records written once. A change's steps, its next step and
its holder are derived from which records exist, so no field goes stale
when a session stops partway. SKILL.md holds the procedure for each step.

Every command runs as `board --personal call …`. In a checkout of a
repository that is itself a board, Board finds that board's
`.board/board.json` first and runs as that board.

## Records

The change is `changes/change/v1` at `/changes/<name>.json`. Every other
record of the change sits in `/changes/<name>/`.

```json
{
  "$type": "changes/change/v1",
  "title": "Let a suite declare a timeout",
  "repo": "/repos/board",
  "branch": "suite-timeout",
  "issue": "https://github.com/crumbkit/board/issues/12",
  "workflow": "/workflows/personal",
  "suite": "/suites/board-tests",
  "route": "owner"
}
```

- `repo` references the repository's repo record
  (docs/architecture/paths/repos.md) or mount. `workflow` is the workflow the
  change follows, resolved through any link when it started. `suite` is the
  check suite. All three are references, so `board check` reports one that
  reaches nothing.
- `route`: `owner` (nobody else owns a CODEOWNERS pattern), `ask-first`
  (someone else does), `unowned` (no CODEOWNERS pattern names anyone) or
  `unread` (the board holds no checkout to read CODEOWNERS from).
- `issue` is optional.

| Record | Type | Holds |
| --- | --- | --- |
| `plans/<stamp>` + `.md` | `changes/plan/v1` | `text`, the plan's Markdown beside it, and `uses`, the standards documents, controls, decisions, evals and other changes it relies on. The newest is the current plan. |
| `claims/<stamp>` | `changes/claim/v1` | `session` and `worktree`, the root of the clone the session works in. The newest names the holder. |
| `checks/<stamp>` | `changes/check/v1` | `run`, a `runs/run/v1`. The commit the newest check's run names in `versions` under the repo record's name is the change's head. |
| `files/<head>` | `changes/files/v1` | `files`, the repository-relative paths the branch touches at that head. |
| `opened` | `changes/opened/v1` | `pull_request`, the URL. |
| `landed` | `changes/landed/v1` | `commit`, the merge commit. It ends the change. |
| `dropped` | `changes/dropped/v1` | `why` the change stopped. It ends the change. |

A `<stamp>` is a UTC time to the microsecond, such as
`20261101T120000000000Z`, so names sort by time.

A workflow is a `changes/workflow/v1` at `/workflows/<name>` whose `steps`
list, in order, each step's `name`, its `actor` (`agent`, `reviewer` or
`owner`) and `done_by`, the kind of record that completes it:

| Kind | The step is done when |
| --- | --- |
| `plan` | the newest plan exists and every reference in its `uses` resolves. |
| `answer` | a `plans/answer/v1` whose `choice` is `approve` answers a `plans/question/v1` whose `for` names the change. |
| `check` | the newest check's run is under the change's suite's `runs/`, has `scope` `full` and `dirty` false, and no result is `fail` or `error`. |
| `review` | a `review/report/v1` whose verdict is `approve` and which has no high or critical finding answers a `review/brief/v1` whose `for` names the change and whose `step` is this step's name, and the report's `range` ends at the head. |
| `landed` | the `landed` record exists. |

Commits match when one is a prefix of the other and both have at least 7
digits. A new check names a new head, so a review step is not done again
until a review covers it. Any other answer than `approve` leaves the answer
step undone.

## What is derived

- **Steps**: the workflow's steps; then `approve` (owner, `answer`), after
  the first `plan` step, when the route is `ask-first`, or `unread` with any
  workflow but `/workflows/personal`, unless a step is already done by an
  answer; then each step a control adds.
- A control adds the step its `adds_step` names (plugins/controls/README.md)
  when it matches a file in the files record at the head. `next` asks
  `controls.check` for the repository with those files. An added step has
  actor `reviewer` and kind `review`, and comes after the last reviewer step.
  Without a files record at the head, the steps every control reaching the
  repository's mounted files could add are listed with `if_touched: true`.
  An unmounted repository has no files in the board, so it lists none
  until the files are recorded.
- **Next step**: the first step that is not done, and its actor.
- **Ended**: `landed` or `dropped` exists. An ended change has no next step
  and takes no new record.

## Capabilities

Each takes the change as `--path /changes/<name>`, except `seed`, `start`
and `open`.

- `seed` writes `/workflows/personal` (plan, build, review, land) and
  `/workflows/work` (approve after plan) when they are missing, and returns
  the paths it wrote.
- `start --title '<title>' --session <s> [--name <n>] [--handle @<you>] [--issue <url>] [--suite <s>] [--workflow <w>] [--repo-name <r>]`
  runs in the repository's clone. It writes, in one commit, the repo record
  when the board has none for the clone, the change, and its first claim.
  - The repository is the mount whose clone shares the caller's git
    directory, else the repo record whose `remote` is the stored form of the
    clone's origin, else a new repo record at `/repos/<name>`, the remote's
    last path segment or `--repo-name`. A record already there is
    `changes::repo::name_taken`.
  - The branch is the clone's. A detached HEAD is
    `changes::branch::detached`, and the default branch (`origin/HEAD`, else
    `main` or `master`) is `changes::branch::default`. A clone with no
    origin is `changes::clone::no_remote`, and a directory in no clone is
    `changes::clone::none`.
  - The workflow is `--workflow`, else the one the link
    `/workflows/repos/<name>` resolves to, else `/workflows/owners/<owner>`
    for the remote's owner, such as its GitHub organisation. A link is a
    `board/mount/v1` with `path`. With neither,
    `changes::workflow::unchosen` names both; a choice that holds no
    workflow is `changes::workflow::not_found`.
  - With a mounted repository, it reads `CODEOWNERS` (`.github/`, root,
    `docs/`, the first found) to set the route and list `ask_first`
    patterns, lists the contributing docs found in `docs`
    (`CONTRIBUTING.md` at the root, in `.github/` or in `docs/`, and
    `AGENTS.md`), and takes the suite as the first inline-code path in them
    that holds a `runs/suite/v1`, in the repository's own board first.
    Without a checkout the route is `unread`. `--suite` overrides the docs;
    with no suite, or one that holds none, it is `changes::suite::unknown`.
  - An existing name is `changes::change::exists`.
- `plan` takes `{path, text, uses}` on stdin, since `uses` is an array, and
  writes the plan record and its text in one write. It keeps each reference
  in `uses` once. It refuses a reference that reaches nothing, or a record
  Board marks degraded for anything but a broken reference of its own
  (`changes::ref::dangling`, whose `refs` lists them); a directory
  (`changes::plan::directory`); a text over 3,000 words
  (`changes::plan::too_long`); and Markdown documents in `uses` over 3,000
  words in all (`changes::plan::uses_too_long`). Words are separated by
  whitespace. The last limit comes from `/evals/board-standards-selection`
  in the owner's board: agents that chose standards from each directory's
  `index.md` stayed within it.
- `brief`: read-only. The newest plan's text path, even once a reference in
  it breaks, one line per record it uses (a control's `summary`, else its
  `requirement`; a decision's `decision`; an eval's question and verdict; a
  document's first heading), `left_out`, the next step and its actor, and
  `controls`, the command `board --personal call controls.check`, which
  checks the caller's repository from any directory in its clone. It holds
  at most 600 words, counting each line with its reference, and `left_out`
  counts the lines it dropped.
- `claim --session <s>` writes a claim whose `worktree` is the root of the
  clone `BOARD_CALLER_DIR` is in, from any subdirectory.
- `files` takes `{path, head, files}` on stdin and writes the files record
  at that head, replacing an earlier one for the same head.
- `check --run <r>` writes a check and returns the head. A path that holds
  no readable `runs/run/v1` is `changes::run::not_found`, and a run that
  names no commit under the repo record's name is `changes::run::no_commit`.
- `next`: read-only. `head`; each step with `name`, `actor`, `done_by`,
  `done`, `record` (the record that did it), `added_by` (the control that
  added it, `route` for an approve step a route added, else null) and
  `if_touched`; `next` and `actor`; and `ended`.
- `landed --commit <sha>` and `drop --why '<text>'` end the change.
- `open [--limit <n>]`: read-only. Changes with no `landed` or `dropped`
  record, by path, at most 100 by default, with `total`, `truncated` and
  `unreadable`.
- `pr` drafts the pull request; see below.

A path that holds no change is `changes::change::not_found`; a degraded
change is `changes::change::degraded`. Every capability that writes a
record of a change refuses an ended one with `changes::change::ended`.

A plugin's environment holds no session id, so the caller passes
`--session`.

## The pull request body

`pr` writes the body to `/changes/<name>/pr.md`, replacing an earlier one,
and returns `path`, `command`, `pull_request` and `unreadable`. It never
opens the pull request, never runs `gh` or git, and makes no model calls.

- `command`: `gh pr create --title '<title>' --body-file '<file on disk>'`,
  quoted for a POSIX shell. Run it from the clone on the head. The title is
  the change's `title` with its whitespace collapsed to single spaces.
- `pull_request`: the URL in the change's `opened` record. `command` is then
  `null` and `pr.md` is not written.
- `unreadable`: the runs the checks name that reach nothing or that Board
  marks degraded.

| Section | From |
| --- | --- |
| (first) | The newest plan's Outcome. |
| Why | The plan's Problem. |
| Out of scope | The plan's Out of scope, when it has text. |
| What the tests prove | The plan's section of that name, then each run a check names: suite, scope, results by outcome, summary. |
| What the tests do not prove | The plan's section of that name, then each run's limits: targeted, not clean, not on the head, and each case that failed, errored or was skipped. Says so when no check names a run. |
| Review | Each report answering a brief for the change: verdict, author, range, findings by severity. |
| Evidence | The change, the plan, and each run and report with its type (`file` for a file that is not a record, `directory` for a directory), or the code that hid it. |
| `Closes <issue>` | The change's `issue`, when set. |

Plan headings match without case. A plan section holds its subsections, and
a `#` line inside a code fence is not a heading. An empty plan section
becomes a line that names it. Text from a record is put on one line, so it
cannot add a heading or a list item. Paths and free text (a run's suite and
summary, a case and its note, a report's author) go in inline code; empty
text shows as `(empty)`. Each run names at most 20 failed, errored or skipped
cases and counts the rest. `Closes` is left out when the issue holds
whitespace. Evidence is listed by logical path, since a board path has no
URL.

## Installing

    uv sync --frozen --no-dev
    ln -s "$PWD" "${XDG_DATA_HOME:-$HOME/.local/share}/board/plugins/changes"
    board plugin enable changes
    board --personal call changes.seed

`next` and `brief` call `controls.check`, so the board needs the controls
plugin enabled too. Enabling writes the schemas to `.board/types/changes/`
and names the plugin in `.board/board.json`. The types stay at v1 and change
in place while the plugin has one user; after such a change, enable it
again.

## Developing

The gates, which CI runs: `uv run ruff format --check .`, `uv run ruff check .`,
`uv run pyright` and `uv run pytest`. The tests drive `$BOARD_BIN`, by default
the workspace's `target/debug/board`, so build it first with
`cargo build -p board-cli`. They install the controls plugin from this
checkout, syncing its environment once. `plugin.json` and the shipped schemas
are generated from `src/board_changes/models.py` by `board_changes.manifest`,
and a test checks that the committed files match.
