# The map: facts, rules and work

Board becomes a map of what the owner and his agents record. A session that
opens a node sees what matters there, and the owner sees one daily view of
what is due, failing, waiting on him and in flight. PLAN.md links this leaf.
The milestone after it, in plan/rules.md, gives rules their second shape
and migrates every board in place. When this milestone lands, its design
moves into docs/ and this leaf is deleted.

## Problem

A session that opens a node cannot see what matters there. No control records
whether it passed, 37 of 79 controls have no checker that runs, no record
describes a node, and nothing collects what is due, failing or waiting.

## Outcome

A session that opens a node sees a short view chosen for its task: the node's
description, the latest handoff, facts, rules with their last-checked state,
work, and the records that name the node. The owner sees one daily view of
what is due, failing, waiting on him and in flight. Rules gain expectations,
checkers, triggers and a failure action, and one fact can hold a value that
many repositories must match.

## The model

Board holds three kinds of record.

- **Facts** are taught: nodes, decisions, facts, attention orders, rules and
  flows. A rule is a fact.
- **Observations** are appended by checkers: rule checks, runs, review
  reports, boundary crossings and handoffs.
- **Work** is what a failing rule or an open question produces: tasks,
  changes and questions.

`map.view` and `map.today` read all three kinds.

## Where each part lives

- The **rules plugin** is the controls plugin renamed. It owns
  `rules/rule/v2`, `rules/fact/v1`, `rules/waiver/v1` and `rules/check/v1`,
  and serves `rules.check` and `rules.evaluate`.
- The **changes plugin** keeps phases, transitions, the change record and
  waivers. Its flows keep their inline checks, and moving them into rules is
  deferred (plan/rules.md).
- The **map plugin** is a new Rust built-in. It owns `map/node/v1` and the
  read-only capabilities `map.view` and `map.today`.
- **Claude Code mods** deliver the views. Once the map mod shows the landing
  view, it owns the session-start block, and the rules mod keeps only its
  edit-time reminders.

The boundary plugin reads rule v2 and keeps its own record. Tasks, evals,
runs, review and attention are unchanged: their records are the work and the
verdicts that rules read, and attention signs waivers and approvals. A
`plans/answer/v1` stores the text of the option it chose. Nothing retires
except rule v1. A rule that only instructs stays a rule with a `none`
checker.

## Build order

Each step is one pull request and its own change, delivered one at a time.
Each step with a UI first agrees its screen in the wireframe the owner
reviewed, which shows the landing view, capture, the rule card, the approval
pane and the daily loop.

1. **Typed plan answers.** Built: a plan gives `answers.ux`, `answers.data`
   and `answers.lifecycle`, and the development flow's conditions read them
   in place of parsing prose.
2. **This plan.**
3. **Control becomes rule.** The plugin, its capabilities, record types,
   the `/controls/` paths, mods, skills and docs all say "rule", and a rule
   with no checker shows as "described". `rules.migrate --rename` moves each
   board's records from `/controls/` to `/rules/` in one commit per board and
   rewrites the paths that boundaries, plan `owed` fields and flow checks
   name. Signed attention orders are never rewritten. The owner signs a new
   order, each board migrates at a stop point, and rename PR 8
   deletes every reader of the old name. The rename goes before every new
   record type, so new types are born with the final name.
4. **Rules record their state.** A check appends a `rules/check/v1` under
   `/rules/<name>/checks/` with the expectation's index, the outcome, the
   observed value and the commit it read. A session-start run writes only
   when the result changes. A locked or read-only board shows the result and
   skips the write with a warning. The session block shows how long ago a
   rule was checked and since when it has failed.
5. **Mounts on a second machine.** A records-only commit turns each absolute
   mount path into a `~/` path, and `repo_missing` names the clone command
   from the repository's `remote`. Done when `board check` exits 0 on a
   second machine.
6. **Answers carry their meaning.** A `plans/answer/v1` stores the chosen
   option's text.
7. **Nodes have descriptions.** `map/node/v1` holds a description and the
   date it was `verified`. `map.view --path` returns the description, the
   latest handoff, facts, rules with their state, work, records that name the
   node and outside references, five per section with a count.
8. **The landing view opens with the session.**
9. **The view is chosen for the task.** `map.view --task <ref>` ranks
   records one reference away from the task first, then by recency. Done
   when it routes at least 4 of 5 sample tasks from the root; otherwise it
   returns candidates and the agent picks.
10. **The daily loop.** `map.today` composes `tasks.available`, `plans.open`,
    failing check records and `changes.open` into one inbox.
11. **Signing is enforced at the boundary.** A boundary may say
    `approval: owner`. `boundary.check` asks `host.decide` to sign the
    content's digest and verifies the signature before any connector sends,
    and the pre-push hook finds that approved crossing, so one push asks
    once. A request unanswered after 10 minutes becomes a `plans/question/v1`
    in the daily loop, and the agent moves on. It does not stop an agent that
    calls `gh` directly. It changes auth, so it gets an adversarial review.

**The trial.** After step 10 the owner uses the landing view and the daily
loop for one week of real work, and no second person uses Board before then.
Step 11 is built during that week. The trial passes when he can resume a node
after three weeks from its view alone, and when the daily loop replaces his
checks of the issue tracker, GitHub and his notes. If either test fails, the
steps in plan/rules.md wait for a revised design.

## Decided

1. Landing entries are chosen for the task, because fixed selection rules
   failed when tried by hand.
2. A session's position on the map is derived and never stored.
3. A rule that nothing checks is shown as "described".
4. Every rule that applies to a file applies, and two rules that contradict
   each other both report.
5. Mount paths are stored as `~/` paths.
6. A second user tries Board only after the owner's trial.
7. Board commits use the owner's personal address.
8. Capture, the approval pane, packs, connectors and a teach and refine
   vocabulary come later, each after its screen is tried. Connectors will
   speak versioned JSON on stdin and stdout.
9. Capture sends a note that fits no node to the one inbox, which the
   weekly review empties.
