# decisions

A plugin built into Board for decisions. A decision records a choice and why
it was made, once. Nothing on the record changes after it is written; its
state is derived from the records written after it.

## The decision record

A decision is a `decisions/decision/v1`, conventionally at
`/projects/<project>/decisions/<id>`, where `<id>` is a sortable UTC time
and a short suffix. The schema declares that folder, so
`board put --new --var project=<project>` names a new one there.

```json
{
  "$type": "decisions/decision/v1",
  "decision": "Use gix for ref resolution in board-tree.",
  "why": "It resolved 1,000 refs in 41 ms against 63 ms for git2.",
  "alternatives": ["Keep git2."],
  "decided": "2026-10-20T09:00:00Z",
  "session": "local_335a90f9",
  "basis": ["/evals/gix-refs"],
  "urls": ["https://github.com/crumbkit/board/pull/12"],
  "supersedes": []
}
```

- `decision`, `why`, `decided` and `session` are required, and none of them
  is empty.
- `basis` lists the records the decision rests on: an eval, an owner's
  answer, a review report or an earlier decision. Each is a board reference,
  so `board check` reports `board::ref::dangling` for one that reaches
  nothing. A reference into a repository, such as `/repos/board/docs/x.md`,
  needs the repo record's checkout (docs/architecture/paths/repos.md); for a
  repository without one, `basis` names the repo record and `decision` or
  `why` names the file.
- `urls` lists web pages, such as a pull request, which nothing checks.
- `supersedes` lists the decisions this one replaces. `board check` reports
  `board::ref::wrong_type` for an entry that is not a `decisions/decision/v1`.

A decision does not say what it binds. When it governs future work, the
agent that records it also writes a `controls/control/v1` whose `basis` names
the decision, and the control says which files the rule covers. A decision
that binds nothing, such as "merge this pull request", needs no control.

## Derived state

- **Awaiting review** while a `plans/question/v1` whose `for` names the
  decision is `open`. To ask the owner for a review, write that question;
  one question whose `for` lists several decisions reviews them together.
- **Reviewed** once such a question is `answered` and none is open. A later
  decision whose `basis` names it does not count. A `withdrawn` question
  counts for nothing, so the decision stays unreviewed until a new question
  for it is answered.
- **Unreviewed** otherwise.
- **Superseded** once any decision lists it in `supersedes`. This is
  separate from review: a superseded decision may still await review.

`for`, `basis` and `supersedes` are compared with a decision's path as
written. A reference through a link, or one that a record in a mounted board
makes to its own board, matches nothing. Decisions and their questions live
in the owner's board, where neither happens.

## Capabilities

`board call decisions.open [--path /projects/<project>] [--limit <n>]` lists
the decisions below the path that await review, oldest first.
`board call decisions.about --ref <path> [--limit <n>]` lists the decisions
not superseded whose `basis` names the path or an entry under it, newest
first, so an agent can ask what was decided about an eval, a plan or a file.
Both read this board and its mounted boards, take a `limit` of 1 to 10,000
(100 by default), and return `{"decisions": [...], "total": n,
"truncated": bool}`. Each decision is its `path`, `decision`, `why`,
`decided`, `basis`, `urls`, `review` (`awaiting`, `reviewed` or
`unreviewed`), `questions` (the open questions whose `for` names it) and
`superseded`. A decision Board marks degraded for anything but a reference
is left out, as is one whose `decided` is not a time.

`evals.about` and `evals.open` read decisions too: each eval lists in
`decisions` those not superseded whose `basis` names it.

## The old type and the migration

Decisions used to be `plans/decision/v1`, which the plans plugin still
ships. The runs mod still writes a `plans/decision/v1` stub on each
`gh pr create`, so until it stops, the plans mod's `/yesterday` report lists
decisions of both types and finds stubs among `plans/decision/v1` records,
and the `changes` plan gate accepts a `decisions/decision/v1` whose `basis`
names the plan or a `plans/decision/v1` whose `refs` do.

`migrate.py` moves a board to the new records. Install a `board` with this
plugin built in, run `board plugin enable` for `decisions`, `evals`, `plans`
and `controls` in the board, then, from this repository:

    uv run --project plugins/decisions python plugins/decisions/migrate.py \
      --session local_<id> --dry-run <board>

Its docstring says what it rewrites. It prints each record it would write
and each eval whose derived status differs from the status it stored, and
writes nothing with `--dry-run`. Without it, one `board put --batch` writes
every record. The script stays in the repository until the change that ends
the stub, which runs it again for the stubs written in between, deletes it
and removes `plans/decision/v1` from the plans plugin.

## Enabling

Board has the plugin built in, so nothing is installed:

    board plugin enable decisions

## Developing

The plugin is the crate `crates/board-plugin-decisions`. It generates its
record schema and the manifest's input and output schemas from Rust structs
with schemars. The record schema is committed in its `types/` directory, and
a test fails when it differs from what the struct generates; run it with
`SNAPSHOTS=overwrite` to rewrite it.

This directory keeps a uv project that tests the plugin and the migration
through the real binary. The gates, which CI runs:
`uv run ruff format --check .`, `uv run ruff check .`, `uv run pyright` and
`uv run pytest`. The tests drive `$BOARD_BIN`, by default the workspace's
`target/debug/board`, so build it first with `cargo build -p board-cli`.
