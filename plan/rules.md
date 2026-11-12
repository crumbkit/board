# Rules: the second shape and migration

Rules gain a second shape that says what they govern, when they run, how
each expectation is checked and what a failure does. Every board migrates in
place, and Board tags its first release. This milestone starts only if the owner's trial
in plan/map.md passes. PLAN.md links this leaf; when the milestone lands, its
design moves into docs/ and this leaf is deleted.

## Rule v2

```
RULE = why + subject + when + expectations + runs + on_failure
  subject:     files, repos, pathspec area, change, plan, diff,
               plan-paths, path; later crossing and host
  when:        section, answers, absent, route (moved from flow checks)
  runs:        navigation, edit, crossing, transition, schedule,
               threshold, demand
  expectation: statement, value (literal, fact or command),
               optional subject, checkers
  checkers:    program, model, person, ci-test, suite or none
  on_failure:  tell, report, ask or refuse, with opens_task, remedy,
               waivable_by and code
```

Program checkers are budget (words, bytes, lines), version, pattern,
present, lookup, sections, field, exists, dependency, command and builtin.
The rules plugin runs a `command` only for a rule in the board's own tree,
never for one reached through a mount, because mounting brings data and never
code. A model, person, ci-test or suite checker reads a verdict record, such
as a signed decision, a review report or a run, through one filter. A
judged check stores the pinned model and a hash of its rubric. `on_failure`
is optional when every checker is `none`. Each subject, trigger and value
kind joins the schema in the step that builds it, so 0.1.0 ships every kind
built. The `change`, `plan`, `diff` and `plan-paths` subjects, `when` and the
`transition` run join the schema only if the deferred move of flow checks is
built.

## One strictness mechanism

`on_failure` is the only place that says how strict Board is (decided
2026-11-12). `tell` warns the agent. `report` puts the rule in the daily loop
and may open one task. `ask` asks the owner through `host.decide`. `refuse`
stops the action. A signed `rules/waiver/v1` is the one record of a bypass. A
limit that grows stricter is two rules on one subject with different limits. A
preference that only instructs is a rule with a `none` checker, and one that
confirms or blocks an action is a rule on that action. No plugin adds a mode, a
ladder, a friction setting or an override type of its own.

## The control scope fields

The scope fields once planned for control v1 become parts of rule v2:

- `repos` becomes `subject.repos`. Each pattern in the subject's paths is
  matched from the root of each listed repository, and the rule reaches
  nothing outside them.
- `basis` becomes `taught_by`, the decisions and records the rule rests on.
  An `applies_to` entry that the paths do not cover moves to
  `taught_by.stated_in`. A rule whose decision is superseded is reported.
- `adds_check` waits with the move of flow checks. Until then the
  lifecycle-paths rules join a gate through the flow's `rule-paths` check, as
  they do today.
- `summary` stays, one line that `changes.brief` shows.

## Flow checks stay in flows for now

The owner deferred the move of flow checks into rules on 2026-11-12. Flows
keep `changes/flow/v1` and their inline checks. The move is planned again,
against the gates that remain, once the change that makes rules guide plans
and reviews has landed. The earlier design, with `changes/flow/v2`, seven
builtins and 25 shipped rules, is in `git show f3c2ebc1:plan/rules.md`.

## Migration

1. Rule v2 replaces rule v1 in place. One pull request moves every reader to
   v2, and no reader accepts both shapes. Only `rules.migrate` reads v1.
2. `rules.migrate --to v2` maps each field: `requirement` becomes one
   expectation, `purpose` becomes `why`, `paths` becomes `subject.paths`, a
   `rule` becomes a program checker and `judged` a model checker. `summary`
   and `sources` are kept. `runs` is written from what v1 implies today. A
   rule with no checker gets a `none` checker and no `on_failure`. An
   obligation carries `owed_by`, which the obligations gate and the mod read
   in place of `kind`.
3. Splitting a requirement into several expectations and choosing checkers
   is judgement. An agent proposes them and the owner confirms each board
   once. Until then the mechanical record behaves as v1 did.
4. Each board migrates at a stop point: the owner pushes his boards and stops
   writes, and only then is the migration applied, the personal board first. A
   board's rules are unread between the install of the new binary and its
   migration, so both happen in one sitting. The next pull request deletes rule
   v1 and the migration.

A migration whose records changed since its dry run is refused with
`board::put::stale` before any write, and the dry run is repeated.

## Build order

Each step is one pull request, delivered one at a time, numbered after
the steps in plan/map.md.

12. The v2 schema and types.
13. The checker library: one module per checker kind, shared by `check` and
    `evaluate`.
14. Verdict checkers, with pinning tests that prove the gates' output is
    unchanged.
15. `rules.migrate --to v2`.
16. Every reader moves to rule v2 in one pull request: the rules plugin
    (check, for-path, evaluate and the index), the boundary, the changes gates
    and brief, which find obligations by `owed_by`, and the mods.
20. The boards migrate at a stop point, in records-only commits.
21. Rule v1 and `rules.migrate --to v2` are deleted.

Steps 17 to 19 are now part of step 16. Steps 22 to 26, the move of flow
checks, are deferred. The numbers are not used again.

Steps 28 to 34 build the remaining kinds. The first tagged release follows
step 34.

28. **Schedules.** `rules.due` lists rules whose schedule has passed with no
    check since. The session-start mod runs due checks whose checker is
    built in, and a scheduled Claude task that the owner approves runs
    `rules.check --due` each morning, model checkers included. Board runs no
    scheduler itself.
29. **Failure actions.** `report` puts the rule in the daily loop, and
    `opens_task` adds one task per failing rule, once. `ask` asks the owner through `host.decide` and keeps his signed
    answer.
30. **A fact holds a version, and a rule compares for equality,** so one
    fact holds every repository to one version.
31. **Waivers.** An agent writes a `rules/waiver/v1` with a reason and an
    expiry at most 30 days away, and it takes effect when the owner signs
    it. A waiver is also the record of an override, and no other override type
    exists. A new rule that fails its first run is saved with such a proposed
    waiver by default.
32. The dependency checker for Cargo.lock and Cargo.toml.
33. **Area churn.** A subject may be a hand-kept list of git pathspecs, and a
    `threshold` trigger fires when the lines changed since the last audit
    pass a limit.
34. Crossing and host subjects.

**The first tagged release** comes after step 34, once every kind is built.
`released.json` lists the shipped types, reports and codes, the version
becomes 0.1.0, and a changelog starts. After it, a changed type is a new
version, reached through `migrate`.
