# Work records

Every change to a repository is recorded in the board as small records that
are written once. The `changes` plugin stops storing a change's phase and
editing it. A change's state, its next step and its holder are derived from
which records exist, so no field goes stale when a session stops partway.

The owner's board holds no change records yet, so `changes/change/v1`
changes in place, as its README allows while the plugin has one user.

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

A change's head is the commit that its newest check run names, newest by
the run's `started`, in `versions` under the repo record's name. `done_by` is one of five kinds, each with one rule:

| Kind | The step is done when |
| --- | --- |
| `plan` | a plan exists and every reference in its `uses` resolves. |
| `answer` | an answer whose `choice` is `approve` exists to a question whose `for` names the change. |
| `check` | a full run of the change's `suite`, on a clean revision, with no `fail` or `error`, whose `for` names the change and has the head in `versions`. |
| `review` | an approving report with no high finding answers a brief whose `for` names the change, whose `step` is this step's name, and whose `range` ends at the head. |
| `landed` | `/changes/<name>/landed` exists. |

A new run names a new head, so the review step is not done again until a
review covers it. A commit pushed with no new run leaves the steps marked
done until the next run. An answer with any other choice leaves the change
at approve, and the agent either writes a new plan and asks again, or drops
the change.

`board call changes.seed` writes `/workflows/personal` (plan, build, review,
land) and `/workflows/work` (approve added after plan) when they are
missing. `start` picks a change's workflow from a link (`board/mount/v1`
with `path`): `/workflows/repos/<name>` for one repository, else
`/workflows/owners/<owner>` for the remote's owner, such as its GitHub
organisation. With neither link, `start` refuses with
`changes::workflow::unchosen`, naming both paths, so a work repository never
falls into the personal workflow by default. `--workflow` overrides both.

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
| change | `title`, `repo` (a repo record), `branch`, `issue`, and the `workflow`, `suite` and `route` resolved at start |
| `plans/<stamp>` + `.md` | `uses`: references to standards nodes, controls, decisions, evals and other changes. The `.md` holds at most 3,000 words. The newest plan is the current one. |
| `claims/<stamp>` | `session`. The newest claim's session is the holder. |
| `files/<head>` | `files`: the repository-relative paths the branch touches at that head |
| `opened` | the pull request URL |
| `landed` | the merge commit |
| `dropped` | why the change stopped |

A review brief stays at `/reviews/<id>/brief`, where `review.run` takes it,
and gains `for` (the change), `step` and `repo`. With `repo`, `review.run`
reads the range in the caller's clone, after checking that its
`caller_remote()` is that repo record's `remote`; it reads the board's own
repository as today when `repo` is absent.

A question that blocks a change names it in `for`, which plan/decisions.md
adds; the question's stored `status`, including `withdrawn`, is unchanged.
A check run gains `for` too, and stays a `runs/run/v1` in its suite's
`runs/`, where `runs.latest` reads it.

`start` sets `route` from CODEOWNERS and `suite` from the contributing docs
when the repository has a checkout, as today. Without one it sets route
`unread`, which is distinct from `unowned`, where CODEOWNERS names nobody,
and takes the suite from `--suite`; with neither it refuses with
`changes::suite::unknown`.

## What is derived

- **Steps**: the workflow's steps, plus each step that a matching control
  adds.
- **Next step**: the first step that is not done, and its actor.
- **Holder**: the session in the newest claim.
- **Done**: `landed` or `dropped` exists.

Files touched decide which controls add steps. The agent records them once
per head with `changes.files`, from the same `git diff --name-only` it
passes to `controls.check`, so every reader, `inflight.now` included,
derives the same steps. Until a files record exists at the head, the steps
every control on the repository could add are listed, marked `if touched`.

## Capabilities

Each takes the change as `--path /changes/<name>`, as today. A plugin's
environment holds no session id, so the caller passes `--session`, as the
tasks plugin takes `owner`. `start` reads the branch from the caller's
clone, as `caller_remote()` reads its origin.

- `start --title '<title>' --session <s> [--name <n>] [--handle @<you>] [--issue <url>] [--suite <s>] [--workflow <w>] [--repo-name <r>]`
  creates the repo record if needed, then the change and a claim, for the
  caller's repository.
- `plan` writes a plan from `{path, file, uses}` on stdin, since `uses` is
  an array. It refuses a reference that reaches nothing
  (`changes::ref::dangling`) and a file over 3,000 words
  (`changes::plan::too_long`).
- `claim --session <s>` writes a claim.
- `files` writes the files record from `{path, head, files}` on stdin.
- `next`: read-only. Steps, each done or not with the record that did it,
  the next step and its actor.
- `brief`: read-only. What an agent needs to start: the plan's path, one
  line per record in `uses` (a control's `summary`, a decision's text, an
  eval's question and verdict, a document's first heading), the next step,
  and the exact `controls.check` command for the change's repository. It
  holds at most 600 words and says what it left out.
- `landed --commit <sha>` and `drop --why '<text>'` end the change.
- `open [--limit <n>]`: read-only. Changes without `landed` or `dropped`.
- `pr` still drafts the pull request body, with its `Closes` line. `pr.md`
  is a draft file, rewritten on each run; it is not a record.

`attach` and `advance` go: a record under the change needs no attaching, and
there is no phase to advance. The build gate on attached `tasks/task/v2`
records goes too. Tasks stay in the tasks plugin, and the check run is the
build step's evidence.

## Agents and the change

A subagent learns its change from the last line of its prompt,
`Board: /changes/<name>`. The changes skill tells every lead to end each
brief with that line. The runs mod reads it (plan/inflight.md).

Work that spans two repositories is two changes. Each plan names the other
change in `uses`, so each brief shows both.

## Build

One pull request in the changes, review and runs plugins. The tests:

- Each kind's rule, with a twin record set that does not satisfy it.
- A new head undoes the build and review steps.
- A control's added step appears only when the files record at the head
  includes a match.
- `brief` stays within 600 words with 100 `uses` and says what it left out.

Done when a change in the owner's board goes from `start` to `landed` with
`next` reporting each step correctly.
