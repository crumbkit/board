# Flow checks as controls: a design test

This directory tests one claim: a flow's phase gate is a control triggered on
a transition. It rewrites the small and development flows as sample
`changes/flow/v2` records whose phases name controls by path, and adds the 25
sample controls they name. No code reads these files.

This directory stands for a board's root, so the flow's reference
`/controls/changes/considered` is the file `controls/changes/considered.jsonc`.
The files end in `.jsonc` because `changes/flow/v2` and `controls/control/v2`
have no schema. As `.json` files, `board check` would mark each one degraded
with `board::record::unknown_type`, and CI runs `board check`.

## Result

Both flows can be written under the rule that a flow holds states,
transitions and references and no expressions. Every check, condition,
remedy and waiver moved into a control. The small flow needs 7 controls for
its 8 checks. The development flow needs 23 controls for its 25 checks. The
two flows share 5 controls: considered, head-recorded, suite-run,
review-approved and pull-request.

The rule holds only because the controls get four things that
`controls/control/v1` lacks. A control's subject can be a change and what it
references, and v1 subjects are only files. A control can carry a `when`
condition. An expectation names a checker and, for a person, a model or a
test, the verdict record that checker writes. And a control has an action on
failure with a remedy, a waiver and a refusal code.

## The sample control shape

| Field | Meaning |
| --- | --- |
| `summary`, `requirement`, `purpose` | As in v1. |
| `subject` | What the control reads: `change` (the change record and its attached records), `plan` (the plan text), `diff` (files the diff edits), `plan-paths` (the plan's declared paths), or `path` (one board path). `within` narrows a set of files to those other controls list. |
| `when` | When the control applies. It takes today's `section`, `section_not`, `absent` and `route` conditions unchanged. |
| `trigger` | `transition`. The flow phase that names the control supplies which transition. |
| `expectations` | One or more. Each names a `checker`. A `program` checker has a `rule`. A `person`, `model` or `test` checker has a `verdict`: the type of record it writes and what the record must hold to count as a pass. An expectation can carry its own refusal `code`. |
| `on_failure` | `action` (`block` or `warn`), `remedy`, `waivable_by` and `code`. |

Placeholders such as `{repo}`, `{change}`, `{head}`, `{plan}`, `{suite}` and
`{name}` stay in the controls. The transition binds them from the change. A
placeholder names a value and computes nothing, so this test does not count
it as an expression.

### What v1 fields become

| v1 field | In the sample shape |
| --- | --- |
| `paths`, `applies_to` | Kept for file subjects. The sample controls do not use them, because their subject is a change. |
| `rule` | Becomes the `rule` of a program expectation. v1's `judged` rule is already a model checker, so it becomes a `model` expectation. |
| `kind` (`standing`, `obligation`) | Kept unchanged. All sample controls are standing. |
| `show` (`start`, `edit`, `failing`) | Overlaps `trigger`. `show: edit` is an edit trigger whose action is to show the requirement. A v2 schema would replace `show` with `trigger` and an action. |
| `sources` | Kept unchanged. |

### New verdict and rule options

The samples use these options, which no record type has today.

| Option | Used by | Meaning |
| --- | --- | --- |
| `at_head: versions` | suite-run | A run's `versions` names the head. Today `at_head` has only `range`. |
| `no_outcomes` | suite-run | No result has one of these outcomes. |
| `under` | suite-run | The record's path starts with this prefix. |
| `suite` | suite-run | The suite a test checker runs. |
| ref target as an object | data-understood, data-rechecked | A ref names a record of a type whose values match. |
| `every`, `count`, `degraded` | data-rechecked, data-understood | Every matching record meets `where`, a count over an array field, and failing on a degraded record. |
| `names_each` | mockups-agreed | The decision names each item of a list a program extracts. |
| `max`, `except`, `values_not` | tasks-closed | At most this many records, leaving one out, whose value is not in a list. |
| `{task}` | tasks-closed | A new placeholder for the change's own task. Board fills no such placeholder today. |

## How each check option maps

| Option today | Where it goes |
| --- | --- |
| `id` | Dropped. The control's path identifies it. Small's review and land phases both name `small/lifecycle-paths`, where today they need two ids. |
| `kind` and its parameters | The `rule` of a program expectation, or the `verdict` of any other checker. |
| `actor` | The `checker`. `owner` becomes `person` with `who: owner`. `reviewer` becomes `model` with `role: reviewer`. No actor becomes `program`. |
| `where` (`values`, `no_findings`, `owner_signed`, `refs`, `at_head`) | The `verdict.where` of the expectation, unchanged. |
| `min`, `distinct` | `verdict.where`, unchanged. No shipped flow uses `distinct`. |
| `scope` | `verdict.where.values.scope` of the run. |
| `subject` (`understood`) | `verdict.where.values.subject` of the understanding check. |
| `part` (`mockups`) | One expectation per part, each with its own refusal code. |
| `rechecks` (`understood`) | `verdict.where.count.min`. |
| `when` | The control's `when`. |
| `blocks: false` | `on_failure.action: warn`. |
| `remedy` | `on_failure.remedy`. A check without one gets its kind's default text, copied into the sample. |
| `waivable` | `on_failure.waivable_by: owner`. The waiver's ref changes from `{change}#suite-run` to `{change}#/controls/changes/suite-run`. |
| Refusal codes from `code_of` (`changes::ux::not_agreed`, `changes::ux::no_neighbours`, `changes::data::not_signed`, `changes::paths::outside_plan`) | The `code` on the expectation that produces it. |
| `{repo}`, `{head}` and the other placeholders | Unchanged, bound by the transition. |

## Mapping table

| Kind | Subject | Expectation | Checker | Fit |
| --- | --- | --- | --- | --- |
| `sections` | plan | Each named heading has text. | program | Fits. |
| `field` (task, route, head, plan, tickets, pulls, with `types` and `min`) | change | The field is set, holds at least `min` refs, and its refs have the named types. | program | Fits. |
| `records` with `actor: owner` | change | The owner signed a decision meeting `where`. | person; the gate reads the decision | Fits. |
| `records` with `actor: reviewer` | change | A reviewer's report covers the head with an allowed verdict and no blocking finding. | model; the gate reads the report | Fits. |
| `records` with no actor | change | An attached record of the type meets `where`. | program | Fits. |
| `run` | change | A full, clean, passing run of `{suite}` names `{head}`. | test; the gate reads the run | Fits, with the new options `at_head: versions`, `no_outcomes`, `under` and `suite`. |
| `paths` | diff | Every edited file is inside the plan's paths. | program | Fits as a built-in rule, `within-plan-paths`. |
| `tasks` | change | No attached task other than the change's own is open. | program | Fits as a `records` rule with `max: 0`, `except` and `values_not`. |
| `understood` | change | The owner signed a decision whose refs name an understanding check of the subject, and none is degraded. | person | Fits, with object ref targets and `degraded`. |
| `understood` with `rechecks` | change | The same, and every understanding check of the subject holds N matching rechecks. | person | Fits badly. See below. |
| `mockups` | plan | The owner agreed each listed mockup, and a neighbours list follows. | person, and program for neighbours | Fits badly. See below. |
| `considered` | change | Every surfaced record is in the plan's uses or considered. | program | Fits as a built-in rule. |
| `obligations` | plan-paths | Every obligation on the plan's paths is listed as owed. | program | Fits. It is a control that reads other controls. `escape_section` is dropped, because the code already ignores the old heading. |
| `controlled-paths` | diff within other controls' paths | The diff touches none of those files. | program | Fits. ui-paths stays a plain fact, and the gate control reuses its paths. |
| `planned-controlled` | plan-paths within other controls' paths | The plan's paths reach none of those controls. | program | Fits. It is `controlled-paths` with a different subject, so both become one `untouched` rule. |
| `flow-valid` | change | Each attached draft flow reads and has no problems. | program | Fits as a built-in rule. It is the same validation `put` runs. |
| `exists` | path | Something is at the path. | program | Fits. |

The prototype verdict check is hard-coded in `gate()` and is in no flow, so
this test leaves it out.

## Kinds that fit badly

Two kinds fit only with a rule that is still specific to changes.

- **`understood` with `rechecks`** needs a verdict filter that applies to
  every matching record, requires at least one, fails on a degraded record,
  and counts the matching items of an array field. The sample writes this as
  `every`, `min`, `degraded` and `count`. `every` and `count` exist only for
  this one check.
- **`mockups`** reads its list of things to approve by parsing the plan's
  Markdown: list items under `### Mockups` and the first URL or board path in
  each. The person is the real checker, but the set they must approve comes
  from a parser. The sample writes this as `names_each: plan-mockups`, which
  is a named program extraction inside a person's verdict.

`considered`, `obligations`, `paths` and `flow-valid` fit, but each stays a
built-in program rule with one user. Moving them into a shared checker does
not make them shorter.

The rewrite also showed two costs.

- A check that behaves differently in two phases becomes two controls. The
  development flow warns on obligations at plan and blocks at review, so it
  has `obligations-noted` and `obligations-settled`. Small's lifecycle check
  has no condition and development's has one, so they are separate controls
  too.
- The pattern `^none\s*(?:[.:]|\n?\z)` appears in 6 controls, because the
  plan's UX, Data and Lifecycle answers are prose that each control parses
  with a regular expression. Counting the No UI change pattern, 8 controls
  carry a regular expression.

Sharing controls between the flows changed two remedies. Small's suite-run
and review checks used their kind's default remedy, and the shared controls
carry development's longer text.

## Recommendation

Adopt with named changes, and build only the first step now.

The test shows that the idea is sound but that a full move gains little
today. The control count is close to the check count, most of the code in
`gates.rs` and `gates/kinds.rs` would move without shrinking, the control
schema grows by four fields, and two kinds keep rules specific to changes.
The part that does merge code is the verdict filter: `records`, `run` and
`understood` all read a record that a person, a model or a test wrote, and
today each reads it with its own code.

The first step, to build now:

1. **Make one verdict filter.** Move `where` and the new options in the
   table above into one shared filter that reads a checker's verdict record.
   The `records`, `run` and `understood` kinds call it. Flows keep their
   checks inline, so no flow changes.
2. **Make the plan's UX, Data and Lifecycle answers typed fields** of the
   plan record, such as `data: none`. That removes the regular expressions
   from 8 development checks, whether they stay in flows or move to
   controls.

The second step, to build only when a second trigger needs a gate check:

3. **Widen a control's subject from files to a change**, with `change`,
   `plan`, `diff`, `plan-paths`, `path` and `within`. The changes plugin
   computes the change's facts, and the checker reads nothing itself, so
   `board-tree` stays the one resolver.
4. **Add `when`, `trigger` and `on_failure`** to controls, replacing `show`.
   Waivers name the control's path. Changes on a v1 flow keep their stored
   flow version, so their waivers keep working.
5. **Move the kinds into about 10 control rules**: `sections`, `field`,
   `verdict`, `records`, `untouched`, `within-plan-paths`, `considered`,
   `obligations-owed`, `flow-valid` and `exists`, plus the named rules
   `neighbours-listed` and the mockup extraction.

The second trigger would be a boundary or an edit-time check that reuses a
gate check. For example, `within-plan-paths` could warn when a file outside
the plan is edited, before review blocks it. AGENTS.md says a new core
concept needs two plugins that use it, and that second user is what would
justify step two.

If the owner does not expect a second trigger soon, the alternative is to drop
steps three to five and keep only step one.

## What this test does not prove

No checker exists, so nothing shows that the sample controls produce the
same pass or fail as today's gates. The equivalence comes from reading
`gates.rs`, `gates/kinds.rs` and `gates/remedy.rs` against each sample. No
schema validates the sample shape. The flow-authoring flow is covered by the
table but was not rewritten.
