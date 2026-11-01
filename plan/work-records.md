# Work records

Every change to a repository is recorded in the board as small records that
are written once. The `changes` plugin stops storing a change's phase and
editing it. A change's state, its next step and its holder are derived from
which records exist, so no field goes stale when a session stops partway.

The owner's board holds no change records yet, so `changes/change/v1`
changes in place, as its README allows.

## Workflows

A workflow is a `changes/workflow/v1` record at `/workflows/<name>` listing
its steps in order. Each step has a name, an actor (`agent`, `reviewer` or
`owner`) and the kind of record that completes it.

```json
{
  "$type": "changes/workflow/v1",
  "steps": [
    {"name": "plan", "actor": "agent", "done_by": "plan"},
    {"name": "approve", "actor": "owner", "done_by": "answer"},
    {"name": "build", "actor": "agent", "done_by": "check"},
    {"name": "review", "actor": "reviewer", "done_by": "review"},
    {"name": "land", "actor": "owner", "done_by": "landed"}
  ]
}
```

A change's head is the commit that its newest check names: a
`checks/<stamp>` record holding `run`, a reference to a `runs/run/v1` in its
suite's `runs/`, which names the commit in `versions` under the repo
record's name. The agent writes it with `changes.check --path <c> --run <r>`
after the suite runs. `done_by` is one of five kinds, each with one rule:

| Kind | The step is done when |
| --- | --- |
| `plan` | a plan exists and every reference in its `uses` resolves. |
| `answer` | an answer whose `choice` is `approve` exists to a question whose `for` names the change. |
| `check` | the newest check's run is a full run of the change's `suite`, on a clean revision, with no `fail` or `error`. |
| `review` | an approving report with no high finding answers a brief whose `for` names the change, whose `step` is this step's name, and whose `range` ends at the head. |
| `landed` | `/changes/<name>/landed` exists. |

A new check names a new head, so the review step is not done again until a
review covers it. A commit pushed with no new run leaves the steps done
until the next run. An answer with any other choice leaves the change
at approve, and the agent writes a new plan and asks again, or drops
the change.

`board call changes.seed` writes `/workflows/personal` (plan, build, review,
land) and `/workflows/work` (approve added after plan) when they are
missing. `start` picks a change's workflow from a link (`board/mount/v1`
with `path`): `/workflows/repos/<name>` for one repository, else
`/workflows/owners/<owner>` for the remote's owner, such as its GitHub
organisation. With neither link, `start` refuses with
`changes::workflow::unchosen`, naming both paths. `--workflow` overrides
both.

A step that a control adds (plan/control-scope.md), such as
`security-review`, comes after the last step whose actor is `reviewer` and
uses the `review` kind. A change whose route is `ask-first` gains the
`approve` step if its workflow lacks one, and so does a change whose route is
`unread` unless its workflow is `/workflows/personal`.

## Records of a change

All live under `/changes/<name>/`, except the change itself at
`/changes/<name>.json`.

| Record | Holds |
| --- | --- |
| change | `title`, `repo` (a repo record), `branch`, `issue`, and the `workflow` (the link's resolved target), `suite` and `route` resolved at start |
| `plans/<stamp>` + `.md` | Built: `changes/plan/v1` (plugins/changes/README.md). The newest plan is the current one. |
| `claims/<stamp>` | `session`, and `worktree`: the root of the caller's clone. The newest claim names the holder and where the work is. |
| `checks/<stamp>` | `run`: the check run at a head |
| `files/<head>` | `files`: the repository-relative paths the branch touches at that head |
| `opened` | the pull request URL |
| `landed` | the merge commit |
| `dropped` | why the change stopped |

A review brief stays at `/reviews/<id>/brief`, where `review.run` takes it,
and gains `for` (the change), `step` and `repo`. With `repo`, `review.run`
reads the range in the caller's clone, after checking that
`shown_remote(caller_remote())` is that repo record's `remote`; it reads the board's own
repository as today when `repo` is absent.

A question that blocks a change names it in `for`, which plan/decisions.md
adds; the question's stored `status`, including `withdrawn`, is unchanged.

`start` sets `route` from CODEOWNERS and `suite` from the contributing docs
when the repository has a checkout, as today. Without one it sets route
`unread`. `--suite` overrides the docs. When neither names a suite, it
refuses with `changes::suite::unknown`.

## What is derived

- **Steps**: the workflow's steps, plus each step that a matching control
  adds.
- **Next step**: the first step that is not done, and its actor.
- **Done**: `landed` or `dropped` exists.

Files touched decide which controls add steps. The agent records them once
per head with `changes.files`, from the same `git diff --name-only` it
passes to `controls.check`, so every reader, `inflight.now` included,
derives the same steps. Until a files record exists at the head, the steps
every control on the repository could add are listed, marked `if touched`.

## Capabilities

Each takes the change as `--path /changes/<name>`, as today. A plugin's
environment holds no session id, so the caller passes `--session`. `start` reads the branch from the caller's
clone, as `caller_remote()` reads its origin, and refuses the default branch
with `changes::branch::default`, so a change always names its work branch.
Every command here runs as `board --personal call` (plan/repos.md).

- `start --title '<title>' --session <s> [--name <n>] [--handle @<you>] [--issue <url>] [--suite <s>] [--workflow <w>] [--repo-name <r>]`
  creates the repo record if needed, then the change and a claim, for the
  caller's repository.
- `plan` and `brief` are built (plugins/changes/README.md). This milestone
  moves `brief`'s next step to the derived steps.
- `claim --session <s>` writes a claim, reading `worktree` from
  `BOARD_CALLER_DIR`.
- `files` writes the files record from `{path, head, files}` on stdin, and
  `check --run <r>` writes a check.
- `next`: read-only. Steps, each done or not with the record that did it,
  the next step and its actor.
- `landed --commit <sha>` and `drop --why '<text>'` end the change.
- `open [--limit <n>]`: read-only. Changes without `landed` or `dropped`.
- `pr` still drafts the pull request body, with its `Closes` line. `pr.md`
  stays a draft file, not a record.

`attach`, `advance` and the build gate on attached `tasks/task/v2` records
go.

## Agents and the change

A subagent learns its change from the last line of its prompt,
`Board: /changes/<name>`. The changes skill tells every lead to end each
brief with it, and the runs mod reads it (plan/inflight.md).

Work across two repositories is two changes, each plan naming the other in
`uses`.

## Build

One pull request in the changes, review and runs plugins. It includes
`start`'s creation of repo records (plan/repos.md). The tests:

- Each kind's rule, with a twin record set that does not satisfy it.
- A new head undoes the review step.
- A claim from a subdirectory records its worktree's root.
- `start` in an unmounted clone creates the repo record once, and a second
  start reuses it.
- A control's added step appears only when the files record at the head
  includes a match.

Done when a change in the owner's board goes from `start` to `landed` with
`next` reporting each step correctly.
