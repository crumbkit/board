# Work records

Every change to a repository is recorded in the board as small records that
are written once. The `changes` plugin stops storing a change's phase and
editing it. A change's state, its next step and its owner are derived from
which records exist, so no field goes stale when a session stops partway.

The owner's board holds no change records yet, so `changes/change/v1`
changes in place, as its README allows while the plugin has one user.

## Workflows

A workflow is a record at `/workflows/<name>` listing its steps in order.
Each step has a name, an actor (`agent`, `reviewer` or `owner`) and the kind
of record that completes it.

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

`done_by` is one of five kinds, each with one rule:

| Kind | The step is done when |
| --- | --- |
| `plan` | `/changes/<name>/plan` exists and every reference in its `uses` resolves. |
| `answer` | an answer exists to a question whose `for` is the change. |
| `check` | a passing `runs/run/v1` under `/changes/<name>/checks/` names the newest head. |
| `review` | approving review reports whose brief has this step's name cover every commit to the head (`review.covered`). |
| `landed` | `/changes/<name>/landed` exists. |

Board ships two workflows. `/workflows/personal` is plan, build, review and
land. `/workflows/work` adds approve after plan. `/workflows/default` is a
link (`board/mount/v1` with `path`) to one of them. A repo record's
`workflow` overrides the default (plan/repos.md).

A step that a control adds (plan/control-scope.md), such as
`security-review`, comes after the last step whose actor is `reviewer` and
uses the `review` kind. A change whose CODEOWNERS route is `ask-first` gains
the `approve` step if its workflow lacks one.

## Records of a change

All live under `/changes/<name>/`, except the change itself at
`/changes/<name>.json`.

| Record | Holds |
| --- | --- |
| change | `title`, `repo` (a repo record), `branch`, `workflow` resolved at start, `route` |
| `plan` + `plan.md` | `uses`: references to standards nodes, controls, decisions, evals and other changes. `plan.md` holds at most 3,000 words. A revision is a new plan record that `follows` the last. |
| `claims/<stamp>` | `session`. The newest claim is the owner. |
| `checks/<stamp>` | a `runs/run/v1` of the repository's checks at one head |
| `reviews/<id>/brief`, `report` | as the review plugin has them; the brief gains `step` |
| `opened` | the pull request URL |
| `landed` | the merge commit |
| `dropped` | why the change stopped |

A question that blocks a change gains `for`, the change's path. A question's
`status` is derived as well: it is answered once an answer names it. The
stored `status` stays valid on old records and is ignored.

## What is derived

- **Steps**: the workflow's steps, plus each step that a matching control
  adds.
- **Next step**: the first step that is not done, and its actor.
- **Owner**: the session in the newest claim.
- **Done**: `landed` or `dropped` exists.

Files touched decide which controls add steps. A plugin never reads git, so
`changes.next` and `changes.brief` take `--files -`, as `controls.check`
does. Without it they list the steps every control on the repository could
add, marked `if touched`.

## Capabilities

- `start --title '<title>' [--name <n>] [--repo /repos/<r>]` creates the
  repo record if needed, then the change and a claim. Without `--repo` it
  uses the current checkout. It resolves the workflow and the route.
- `plan --change <c> --file <plan.md> --uses <ref>…` writes a plan. It
  refuses a reference that reaches nothing
  (`changes::ref::dangling`) and a file over 3,000 words
  (`changes::plan::too_long`).
- `claim --change <c>` writes a claim for the calling session.
- `next --change <c> [--files -]`: read-only. Steps, each done or not with
  the record that did it, the next step and its actor.
- `brief --change <c> [--files -]`: read-only. What an agent needs to start:
  the plan's path, one line per record in `uses` (a control's `summary`, a
  decision's text, an eval's question and verdict, a document's first
  heading), the next step, and the exact `controls.check` command for the
  change's repository. It holds at most 600 words and says what it left out.
- `landed --change <c> --commit <sha>` and `drop --change <c> --why '<text>'`
  end the change.
- `open [--limit <n>]`: read-only. Changes without `landed` or `dropped`.
- `pr` still drafts the pull request body.

`attach` and `advance` go. A record under the change needs no attaching, and
there is no phase to advance.

## Agents and the change

A subagent learns its change from the last line of its prompt,
`Board: /changes/<name>`. The changes skill tells every lead to end each brief
with that line. The runs mod reads it (plan/inflight.md).

Work that spans two repositories is two changes. Each plan names the other
change in `uses`, so each brief shows both.

## Build

One pull request in the changes, plans and review plugins. The tests:

- Each kind's rule, with a twin record set that does not satisfy it.
- A control's added step appears only when `--files` includes a match.
- The owner from two claims is the newer.
- `brief` stays within 600 words with 100 `uses` and says what it left out.

Done when a change in the owner's board goes from `start` to `landed` with
`next` reporting each step correctly.
