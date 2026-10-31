# Decisions

A decision records a choice and why it was made, once. A new `decisions`
plugin owns the record. The owner chose a plugin of its own on 2026-10-31,
because the owner decides about evals, reviews, standards and repositories,
not only about plans. Board's built-in types describe how a board is stored
and read, so a workflow record such as a decision belongs in a plugin.

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
- `supersedes` lists the decisions this one replaces. Only a decision
  supersedes a decision.
- `review` is `wanted` or `none`. It records the request only.

Nothing on the record changes after it is written. Its state is derived:

- **Reviewed** once the owner has answered a question whose `for` names the
  decision (plan/work-records.md). An agent's later decision that builds on
  it does not count. To ask for a review, the agent writes that question; to
  review several decisions at once, it writes one question whose `for` lists
  them.
- **Superseded** once a later decision lists it in `supersedes`.
- **Awaiting review** when `review` is `wanted` and it is neither.

## Capabilities

- `decisions.open [--path <p>] [--limit <n>]`: read-only. Decisions awaiting
  review, oldest first.
- `decisions.about --ref <path>`: read-only. Decisions whose `basis` names
  the path or an entry under it, not superseded, newest first. An agent uses
  it to ask what was decided about an eval, a plan or a file.

## Evals

The eval record stops storing what can be derived. `status` goes, and
`abandoned` (a reason) comes in. `decision` is renamed `informs`, since it
holds the question the eval informs, written before any decision exists.
`mounts` becomes `repos`, holding repo record references (plan/repos.md).

- **Planned**: no `runs`.
- **Running**: `runs` and no `verdict`.
- **Concluded**: a `verdict`.
- **Abandoned**: `abandoned` is set.

`evals.about` and `evals.open` return the derived status, and each eval row
gains `decisions`: the decisions not superseded whose `basis` names that
eval. `evals.about --repo` replaces `--mount`.

## Migration

A one-off script, run once and not kept, rewrites every
`plans/decision/v1` as `decisions/decision/v1` at the same path. `refs`
becomes `basis`; `written_by` is dropped, since only stubs set it; the other
fields copy. A stub whose `why` is empty is listed for the owner and not
rewritten. A decision whose `review` is `wanted` and that a later decision
names as accepted, such as an acceptance listing it in `refs`, gets an
answered question so it stays reviewed. The same script rewrites each eval:
it drops `status`, sets `abandoned` from an `abandoned` status, and renames
`decision` and `mounts`. It writes nothing with `--dry-run`.

The plans hook and the `changes` plan gate read the new type. The runs mod
keeps writing its stub until plan/inflight.md replaces it.
`plans/decision/v1` is removed from the plans plugin after that.

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
