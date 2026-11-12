# Rules: the second shape, flows that name rules, and migration

Rules gain a second shape that says what they govern, when they run, how
each expectation is checked and what a failure does. Flows stop holding
checks and name the rules each transition runs. Every board migrates, and
Board tags its first release. This milestone starts only if the owner's trial
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
  on_failure:  tell, report or refuse, with remedy, waivable_by and code
```

Program checkers are budget (words, bytes, lines), version, pattern,
present, lookup, sections, field, exists, dependency, command and builtin.
The rules plugin runs a `command` only for a rule in the board's own tree,
never for one reached through a mount, because mounting brings data and never
code. A model, person, ci-test or suite checker reads a verdict record, such
as a signed decision, a review report or a run, through one filter. A
judged check stores the pinned model and a hash of its rubric. `on_failure`
is optional when every checker is `none`. Rule v2's schema declares every
subject, trigger and value kind from the start, and `rules.check` reports a
kind that is not built yet as unsupported.

## The control scope fields

The scope fields once planned for control v1 become parts of rule v2:

- `repos` becomes `subject.repos`. Each pattern in the subject's paths is
  matched from the root of each listed repository, and the rule reaches
  nothing outside them.
- `basis` becomes `taught_by`, the decisions and records the rule rests on.
  An `applies_to` entry that the paths do not cover moves to
  `taught_by.stated_in`. A rule whose decision is superseded is reported.
- `adds_check` becomes a `transition` run on a file rule. A rule whose `runs`
  names a transition, and whose files match the change's diff or plan paths,
  joins that phase's gate. The lifecycle-paths controls become such rules.
- `summary` stays, one line that `changes.brief` shows.

`rules.check` also reads the rules in a mounted repository's own board, as
data at the mount's pinned commit, and lists each rule with the board that
holds it. With no path it checks the caller's repository, found by the repo
record whose `remote` matches the caller's. The caller passes a branch's
files on stdin, and `--limit` cuts each list and reports its total.

## Flows name rules

The changes plugin binds the change subject at a transition: the plan text,
its typed answers, the diff paths, the plan paths, the attached records and
placeholders such as `{head}`. It calls `rules.evaluate` with the rules the
phase names. An expectation whose checker is a builtin that changes owns
comes back as delegated, and changes evaluates it. Changes owns seven
builtins: considered, obligations-owed, within-plan-paths, untouched,
flow-valid, mockups-agreed and understood-rechecks. No plugin depends on
another plugin's crate.

`changes/flow/v2` holds states, transitions and rule paths. The small flow
needs 7 rules and the development flow 23, with 5 shared, so 25 in all. They
ship under `/rules/changes/`, and `plugin enable changes` installs them.

## Migration

1. Every reader learns to read rule v1 or v2 before any board migrates, so
   no gate or session block goes blank.
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
4. The personal board migrates first, then the other boards. Then rule v1's
   struct, schema and readers are deleted.
5. Each inline flow check maps to a shipped rule. Its `kind` becomes a
   program checker or a verdict, its `actor` the checker, and its `when`,
   `blocks`, `remedy`, `waivable` and refusal codes move into the rule.
   `changes.migrate --to flow-v2` re-pins every in-flight change to the
   matching flow v2 and rewrites waiver refs from `{change}#<id>` to
   `{change}#/rules/changes/<name>`. A board's own flows get one rule per
   check under `/rules/flows/<flow>/`. Then flow v1's inline checks are
   deleted. Boundaries do not migrate.

A migration whose records changed since its dry run is refused with
`board::put::stale` before any write, and the dry run is repeated.
`/suites/board-gates` passes on flow v2 before any change is re-pinned.

## Build order

Each step is one pull request, delivered one at a time, numbered after
the steps in plan/map.md.

12. The v2 schema and types.
13. The checker library: one module per checker kind, shared by `check` and
    `evaluate`.
14. Verdict checkers, with pinning tests that prove the gates' output is
    unchanged.
15. `rules.migrate --to v2`.
16. The rules plugin reads v1 or v2: check, for-path, evaluate and the index.
17. The boundary reads v1 or v2.
18. The changes gates and brief read v1 or v2, and find obligations by
    `owed_by`.
19. The mods read v1 or v2.
20. The boards migrate, in records-only commits.
21. Rule v1 is deleted.
22. Changes binds the change subject and serves its seven builtins.
23. Flows name rules, and the shipped flows ship their 25 rules.
24. A file rule joins a phase, which is what control v1's planned
    `adds_check` asked for.
25. `changes.migrate --to flow-v2`.
26. Flow v1's inline checks are deleted.
27. **The first tagged release.** `released.json` lists the shipped types,
    reports and codes, the version becomes 0.1.0, and a changelog starts.
    After it, a changed type is a new version, reached through `migrate`.

After the release:

28. **Schedules.** `rules.due` lists rules whose schedule has passed with no
    check since. The session-start mod runs due checks whose checker is
    built in, and a scheduled Claude task that the owner approves runs
    `rules.check --due` each morning, model checkers included. Board runs no
    scheduler itself.
29. **Failure actions.** `report` puts the rule in the daily loop, and
    `opens_task` adds one task per failing rule, once.
30. **A fact holds a version, and a rule compares for equality,** so one
    fact holds every repository to one version.
31. **Waivers.** An agent writes a `rules/waiver/v1` with a reason and an
    expiry at most 30 days away, and it takes effect when the owner signs
    it. A new rule that fails its first run is saved with such a proposed
    waiver by default.
32. The dependency checker for Cargo.lock and Cargo.toml.
33. **Area churn.** A subject may be a hand-kept list of git pathspecs, and a
    `threshold` trigger fires when the lines changed since the last audit
    pass a limit.
34. Crossing and host subjects.
