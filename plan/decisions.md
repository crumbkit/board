# Decisions

A decision records a choice and why it was made, once. A new `decisions`
plugin owns the record, because a choice is not specific to plans: the owner
decides about evals, reviews, standards and repositories too. Board's core
types stay protocol types.

A decision does not say what it binds. When a decision governs future work,
the agent that records it also writes a control whose `basis` names the
decision (plan/control-scope.md). The control says which repositories and
files the rule covers, and `controls.check` is the one command an agent runs
before it works. A decision that binds nothing, such as "merge this pull
request", needs no control.

## The record

`decisions/decision/v1`, wherever the writer keeps decisions, such as
`/projects/<project>/decisions/<id>`.

```json
{
  "$type": "decisions/decision/v1",
  "decision": "Use gix for ref resolution in board-tree.",
  "why": "It resolved 1,000 refs in 41 ms against 63 ms for git2.",
  "alternatives": ["Keep git2."],
  "decided": "2026-10-20T09:00:00Z",
  "session": "local_335a90f9",
  "review": "wanted",
  "basis": ["/evals/gix-refs"],
  "urls": ["https://github.com/crumbkit/board/pull/12"],
  "supersedes": []
}
```

- `decision`, `why`, `decided` and `session` are required, and `why` is never
  empty.
- `basis` lists the records the decision rests on: an eval, an owner's
  answer, a review report or an earlier decision. Each is a board reference,
  including one into a repository such as `/repos/board/docs/x.md`, so
  `board check` reports one that reaches nothing.
- `urls` lists web pages, which nothing can check.
- `supersedes` lists the decisions this one replaces.
- `review` is `wanted` or `none`. It records the request only.

Nothing on the record changes after it is written. Its state is derived:

- **Reviewed** once a later decision lists it in `basis`. An owner's
  acceptance of several decisions is one decision whose `basis` names them.
- **Superseded** once a later decision or control lists it in `supersedes`.
- **Awaiting review** when `review` is `wanted` and it is neither.

## Capabilities

- `decisions.open [--path <p>] [--limit <n>]`: read-only. Decisions awaiting
  review, oldest first.
- `decisions.about --ref <path>`: read-only. Decisions whose `basis` names
  the path or an entry under it, not superseded, newest first. An agent uses
  it to ask what was decided about an eval, a plan or a file.

## Evals

The eval record stops storing what can be derived. `status` and `decision`
go, and `abandoned` (a reason) comes in. `mounts` becomes `repos`, holding
repo record references (plan/repos.md).

- **Planned**: no `runs`.
- **Running**: `runs` and no `verdict`.
- **Concluded**: a `verdict`.
- **Abandoned**: `abandoned` is set.

`evals.about` and `evals.open` return the derived status, and each eval row
gains `decisions`: the decisions not superseded whose `basis` names that
eval. `evals.about --repo` replaces `--mount`.

## Migration

A one-off script, run once and not kept, rewrites every
`plans/decision/v1` as `decisions/decision/v1` at the same path: `refs`
becomes `basis`, and the other fields copy. A stub whose `why` is empty is
listed for the owner and not rewritten. The same script rewrites each eval:
it drops `status` and `decision`, sets `abandoned` from an `abandoned`
status, and renames `mounts`. It writes nothing with `--dry-run`.

The `runs` mod stops writing a decision stub when a pull request is opened.
The work records record the pull request instead (plan/work-records.md).
The `changes` gate that looked for a `plans/decision/v1` reads the new type.
`plans/decision/v1` is then removed from the plans plugin.

## Build

One pull request: the plugin, the eval changes, the migration script and the
readers that move to the new type. The tests:

- Each derived state, from records written in order, with a twin that does
  not reach it.
- `board check` reports a `basis` or `supersedes` reference that reaches
  nothing, and a reference of the wrong type in `supersedes`.
- The migration round-trips the owner's records with no loss, checked by
  `board check` on a copy of the board.

Done when the owner's board has migrated and `decisions.open` lists the
decisions that still wait on the owner.
