# Decisions

A decision records a choice and why it was made, once. A new `decisions`
plugin owns the record. The owner chose a plugin of its own on 2026-10-31,
because the owner decides about evals, reviews, standards and repositories
as well as plans. Board's built-in types describe how a board is stored
and read, so a workflow record such as a decision belongs in a plugin.

A decision does not say what it binds. When a decision governs future work,
the agent that records it also writes a control whose `basis` names the
decision. The control says which repositories and files the rule covers, and
`controls.check` is the one command an agent runs before it works. A
decision that binds nothing, such as "merge this pull request", needs no
control.

The control schema forbids unknown fields, so the decisions pull request
adds `basis` to it: an optional list of board references. The control scope
step, built later, adds the other new control fields and reports a control
whose decision is superseded.

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
  `board check` reports one that reaches nothing. A reference into a
  repository needs the repo record's checkout; for a repository without one,
  `basis` names the repo record and the decision's text names the file.
- `urls` lists web pages, which nothing can check.
- `supersedes` lists the decisions this one replaces. Only a decision
  supersedes a decision.

Nothing on the record changes after it is written. Its state is derived:

- **Awaiting review** while a question whose `for` names the decision is
  open. The decisions pull request adds `for` to `plans/question/v1`: a list of
  references to what the question belongs to, beside the prose `blocks`. To ask for a review, the agent writes
  that question; one question whose `for` lists several decisions reviews
  them together.
- **Reviewed** once that question is answered. An agent's later decision
  that builds on it does not count. A withdrawn question leaves the decision
  unreviewed until a new question for it is answered.
- **Superseded** once a later decision lists it in `supersedes`.

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
- **Abandoned**: `abandoned` is set. This wins over the other three.

`evals.about` and `evals.open` return the derived status, and each eval row
gains `decisions`: the decisions not superseded whose `basis` names that
eval. `evals.about --repo` replaces `--mount`.

## Migration

A migration script rewrites every
`plans/decision/v1` as `decisions/decision/v1` at the same path. `refs`
becomes `basis`; `written_by` is dropped, since only stubs set it; the other
fields copy. A stub whose `why` is empty is rewritten with a `why` that
says it was written when the pull request in its `urls` was opened and that
no reason was recorded. `review` is dropped. Every decision whose `review`
is `wanted`, and every stub, waits on the owner through an open question.
A later decision that names it in `refs` does not change that, as the
review rule above says. The script writes one question per project, at
`/projects/<project>/questions/<id>`, whose `for` lists that project's
waiting decisions. It takes `--session`, the session of the agent that runs
it, for the questions' required `session`.

The same script rewrites each eval: it drops `status`, sets `abandoned`
from an `abandoned` status, and renames `decision` and `mounts`. It prints
each eval whose derived state differs from its stored `status`, such as a
`planned` eval that has runs. It writes nothing with `--dry-run`.

The runs mod keeps writing its `plans/decision/v1` stub on each
`gh pr create` until the pull request that writes `opened` records, in the
inflight step. Until then, the plans hook and the `changes` plan gate read
both types, and the hook keeps finding stubs among `plans/decision/v1`
records, so the morning report still lists the stubs written in between.
That later pull request ends the stub, runs the migration script again for
those stubs, then deletes the script and removes `plans/decision/v1` from
the plans plugin. Until then the script stays in the repository.

## Build

One pull request: the plugin, the eval changes, `basis` on controls, the
migration script and the readers that move to the new type. The tests:

- Each derived state, from records written in order, with a twin that does
  not reach it.
- `board check` reports a `basis` or `supersedes` reference that reaches
  nothing, and a reference of the wrong type in `supersedes`.
- A control with `basis` is admitted, and `board check` reports one whose
  `basis` reaches nothing.
- The plans hook lists a `plans/decision/v1` stub written after the
  migration.
- The migration keeps every field of the owner's records except those this
  section drops or renames, checked by `board check` on a copy of the
  board, and it prints each eval whose derived state changes.

Done when the owner's board has migrated and `decisions.open` lists the
decisions that still wait on the owner.
