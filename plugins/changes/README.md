# changes

This plugin is still a Python program (docs/plugins/python.md). A new plugin
is a crate built into Board (docs/plugins/built-in/writing.md).

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
  "tickets": ["/jira/acme/ACME-123"],
  "pulls": ["/gh/octo/widgets/pulls/7"],
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
- `tickets` references the `jira/issue/v1` or `github/issue/v1` records the
  change works on, and `pulls` its `github/pull/v1` records. The jira and
  github plugins write those records. `board check` reports a ticket or pull
  that reaches nothing (`board::ref::dangling`) or a record of another type
  (`board::ref::wrong_type`). `board refs <ticket>` lists the changes that
  reference a ticket.
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
- `attach --path /changes/<name> [--ref <path>] [--ticket <path>] [--pull <path>] [--route <route>] [--head <sha>] [--issue <url>] [--pull-request <url>]`
  adds a reference once or sets the fields. `--ref` adds to `records`,
  `--ticket` to `tickets` and `--pull` to `pulls`. Refuses a reference that
  reaches nothing (`changes::ref::dangling`), and a ticket or pull that is
  not a record of a type its list accepts (`changes::ref::wrong_type`).
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
  (`changes::phase::final`). It keeps each reference in `uses` once. It
  refuses a reference that reaches nothing, or a record Board marks degraded
  for anything but a broken reference of its own (`changes::ref::dangling`,
  whose `refs` lists them); a directory (`changes::plan::directory`); a text
  over 3,000 words (`changes::plan::too_long`); and Markdown documents in
  `uses` over 3,000 words in all (`changes::plan::uses_too_long`). Words are
  separated by whitespace. The last limit comes from
  `/evals/board-standards-selection` in the owner's board: agents that chose
  standards from each directory's `index.md` stayed within it.
- `brief --path /changes/<name>`: read-only. `brief_line`,
  `board-brief: /changes/<name>`, which comes first; a builder's first
  message starts with it (see "The write guard"). The newest plan's text path,
  even once a reference in it breaks,
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

## The write guard

`claude-code/` holds a Claude Code mod, `board-changes`, that runs before
every Edit, Write and NotebookEdit. Install it with
`claude plugin install board-changes@board`. It needs `git`, and `board` on
`PATH` or in `~/.local/bin`, `/opt/homebrew/bin` or `/usr/local/bin`.

1. It places the file: it expands a leading `~/` from `HOME`, follows every
   symbolic link, and takes a file that does not exist yet from its nearest
   directory that does. A path it cannot place, such as `~user/x` or a link
   that leads nowhere, is refused with `changes::guard::unknown`.
2. It finds the git repository that holds that directory, through its
   common directory, so a worktree counts as its repository. It lists the
   mounts of each board in `boards` of `$XDG_CONFIG_HOME/board/config.json`
   (by default `~/.config/board/config.json`). A mount holds the file when
   the file's repository is a worktree of the mount's `repo`, compared as
   spelled. Otherwise, when the repository's `origin` is a local path or a
   `file://` URL, a mount holds the file when that origin is a worktree of
   the mount's `repo`. Otherwise a mount holds the file when both
   repositories' `origin` remotes name the same repository; ssh and https
   forms, letter case and a trailing `.git` do not matter. So a separate
   clone is guarded too. A file that no mount holds is admitted, so plans,
   other board records, memory and scratch directories stay writable. A
   repository with no `origin` that is no worktree of a mount is admitted.
3. Otherwise it reads the acting agent's first user message: a subagent's
   own, or the main conversation's. The first line that holds text must be
   `board-brief: /changes/<name>`. The guard checks that line, and does not
   check that `changes.brief` produced it, so a parent agent can write the
   line itself.
4. It reads that change from each board that mounts the repository. It
   admits the call when the change's `phase` is `build` and its `mount` is the
   mount that holds the file.

Before any of that, the guard refuses three kinds of file to every agent,
brief or not, because they change only through board commands: the board
config, each known board's change records (`changes/<name>.json`) and its
mount records. A change's plan, `changes/<name>/plan.md`, stays writable.

Each refusal names one code and what fixes it:

- `changes::guard::board_record`: the file is the board config, a change
  record or a mount record. The refusal names the board command to use.

- `changes::guard::no_brief`: the first message has no brief line, or names
  a change no such board holds. Start a builder whose first message is the
  output of `changes.brief`.
- `changes::guard::not_building`: run `changes.advance` until the change is
  in `build`.
- `changes::guard::wrong_mount`: the brief's change is for another
  repository.
- `changes::guard::unknown`: the path could not be placed, git, the config
  or `board` failed, or the conversation could not be read. The refusal says
  what failed.

The guard is a guardrail for agents that follow the process, not a defence
against an agent set on getting round it. The known routes round it are
Bash writes, a hand-typed brief line and a clone of a local clone.

There is no bypass flag. The guard cannot see:

- files written through Bash, such as `sed -i` or a heredoc. The review gate
  checks that a change's commits came from its builder sessions;
- a clone of a mounted repository with no `origin` remote, or one whose
  `origin` points somewhere else, such as a fork or an ssh host alias like
  `git@github-work:owner/name.git`;
- a brief line that a parent agent wrote by hand. The review gate checks
  that a change's commits came from its builder sessions;
- a first message that has left the newest 4,096 entries of a conversation,
  or that a compaction replaced, before the agent's first guarded edit. The
  guard keeps a brief it has found for each session and agent until the mod
  reloads, and `/clear` or `/resume` starts over. With no brief kept and a
  full window of 4,096 entries, the guard refuses with
  `changes::guard::unknown`, since the oldest row left may be one another
  agent wrote.

The guard refuses an edit of every mounted repository, including the notes
repositories a personal board mounts, such as `/repos/notes-personal`. An
agent without a brief writes notes and tasks through the `tasks` and `notes`
capabilities instead.

`claude plugin test plugins/changes/claude-code` runs the mod's tests.

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
