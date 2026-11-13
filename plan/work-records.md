# Work records

Every change to a repository is recorded as small records written once. The `changes` plugin stops storing a change's phase and
editing it. A change's state, its next step and its holder are derived from
which records exist, so no field goes stale when a session stops partway.

The owner's board holds no change records yet, so `changes/change/v1`
changes in place, as its README allows.

## Flows

A change follows a `changes/flow/v1` (plugins/changes/README.md), whose
phases list the checks that end them, each with an actor (`agent`,
`reviewer` or `owner`). This milestone adds five check kinds and derives a
change's place in its flow from which records exist.

A change's head is the commit that its newest check names: a
`checks/<stamp>` record holding `run`, a reference to a `runs/run/v1` in its
suite's `runs/`, which names the commit in `versions` under the repo
record's name. The agent writes it with `changes.check --path <c> --run <r>`
after the suite runs. Each new kind has one rule:

| Kind | The check passes when |
| --- | --- |
| `plan` | a plan exists and every reference in its `uses` resolves. |
| `answer` | an answer whose `choice` is `approve` exists to a question whose `for` names the change. |
| `check` | the newest check's run is a full run of the change's `suite`, on a clean revision, with no `fail` or `error`. |
| `review` | an approving report with no high finding answers a brief whose `for` names the change, whose `step` is the check's phase, and whose `range` ends at the head. |
| `landed` | `/changes/<name>/landed` exists. |

A new check names a new head, so the review check fails again until a
review covers it. A commit pushed with no new run leaves the checks passing
until the next run. Any other choice leaves the change at approve;
the agent plans and asks again, or drops it.

`start` picks the flow as plugins/changes/README.md describes, and also
reads `/flows/owners/<owner>` for the remote's owner, such as its GitHub
organisation, when no repository link names one. `board changes seed`
writes the `work` flow, development with an `approve` phase after plan, at
`/flows/work` when it is missing.

A check that a rule adds (plan/rules.md), such as `security-review`, joins
the phase its `transition` run names, or the last phase when the flow lacks
that one. A change whose route is `ask-first` gains the `approve` phase if
its flow lacks one, and so does a change whose route is `unread` unless its
flow is `development`. A rule adds a check only once a file rule can join a
phase, which waits with the deferred move of flow checks; until then a
change's checks are its flow's own.

## Records of a change

All live under `/changes/<name>/`, except the change itself at
`/changes/<name>.json`.

| Record | Holds |
| --- | --- |
| change | `title`, `repo` (a repo record), `branch`, `issue`, and the `flow` copied at start, `suite` and `route` resolved at start |
| `plans/<stamp>` + `.md` | Built: `changes/plan/v1` (plugins/changes/README.md). The newest plan is the current one. |
| `claims/<stamp>` | `session`, and `worktree`: the root of the caller's clone. The newest claim names the holder and where the work is. |
| `checks/<stamp>` | `run`: the check run at a head |
| `files/<head>` | Deferred. `files`: the repository-relative paths the branch touches at that head |
| `opened` | the pull request URL |
| `landed` | the merge commit |
| `dropped` | why the change stopped |

A review brief stays at `/reviews/<id>/brief`, where `review.run` takes it,
and gains `for` (the change), `step` and `repo`. With `repo`, `review.run`
reads the range in the caller's clone, after checking that
`shown_remote(caller_remote())` is that repo record's `remote`; without
`repo` it reads the board's own repository.

A question that blocks a change names it in `for`, which plan/decisions.md
adds; the question's stored `status`, including `withdrawn`, is unchanged.

`start` sets `route` from CODEOWNERS and `suite` from the contributing docs
when the repository has a checkout, as today. Without one it sets route
`unread`. `--suite` overrides the docs. When neither names a suite, it
refuses with `changes::suite::unknown`.

## What is derived

- **Checks**: the flow's checks, plus each check that a matching rule
  adds (the rule-added part is deferred with the move of flow checks).
- **Next step**: the first phase with a failing check, and its actor.
- **Done**: `landed` or `dropped` exists.

Deferred with the move of flow checks: files touched decide which rules add
checks. The agent records them once
per head with `changes.files`, from the same `git diff --name-only` it
passes to `rules.check`, so every reader, `inflight.now` included,
derives the same checks. Until a files record exists at the head, the checks
every rule on the repository could add are listed, marked `if touched`.

## Capabilities

Each takes the change as `--path /changes/<name>`, as today. A plugin's
environment holds no session id, so the caller passes `--session`. `start` reads the branch from the caller's
clone, as `caller_remote()` reads its origin, and refuses the default branch
with `changes::branch::default`, so a change always names its work branch.
Every call these plans add runs as `board --personal call`, because in a checkout of a repository that is itself a
board, Board finds its `.board/board.json` first and runs as that board.

- `start --title '<title>' --session <s> [--name <n>] [--handle @<you>] [--issue <url>] [--suite <s>] [--flow <f>] [--repo-name <r>]`
  creates the caller's repo record at `/repos/<name>`, the remote's last
  path segment, unless one has its remote, then the change and a claim.
  Another remote's record there is `changes::repo::name_taken`, suggesting
  `--repo-name`.
- `plan` and `brief` are built (plugins/changes/README.md). This milestone
  moves `brief`'s next step to the derived checks.
- `claim --session <s>` writes a claim, reading `worktree` from
  `BOARD_CALLER_DIR`.
- `files` writes the files record from `{path, head, files}` on stdin
  (deferred with the move of flow checks), and `check --run <r>` writes a check.
- `next`: read-only. Checks, each passing or not with the record that
  passed it, the next step and its actor.
- `landed --commit <sha>` and `drop --why '<text>'` end the change.
- `open [--limit <n>]`: read-only. Unended changes.
- `pr` still drafts the pull request body; `pr.md` stays a draft file.

`attach`, `advance` and the build gate on attached `tasks/task/v2` records
go.

## Agents and the change

A subagent learns its change from the last line of its prompt,
`Board: /changes/<name>`. The changes skill tells every lead to end each
brief with it, and the runs mod reads it (plan/inflight.md).

Work across two repositories is two changes, each plan naming the other in
`uses`.

## Build

One pull request in the changes, review and runs plugins. The tests:

- Each kind's rule, with a twin record set that does not satisfy it.
- A new head fails the review check again.
- A claim from a subdirectory records its worktree's root.
- `start` in an unmounted clone creates the repo record once, and a second
  start reuses it.
- Deferred with the move of flow checks: a rule's added check appears only
  when the files record at the head includes a match.

Done when a change in the owner's board goes from `start` to `landed` with
`next` reporting each check correctly.
