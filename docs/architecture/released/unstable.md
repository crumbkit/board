# Unstable interfaces

released.json's `unstable` list names type ids and codes that a release ships
without freezing them, with a note saying until when. The test skips a
schema file, a report or a code that the list names, and the file does not
list them elsewhere. Each type the list names must be one of the external
plugin protocol's, and each code must be one of the protocol's codes, which
`PROTOCOL_CODES` in crates/board-cli/src/tests/released.rs lists.

In 0.1.0 the list holds the external plugin protocol, "unstable until
0.2.0": `board/plugin-manifest/v1`, `board/call/v1`,
`board/call-result/v1` and `board/error/v1`, and the codes about an
external plugin's process or its manifest: `board::plugin::bad_manifest`,
`bad_output`, `failed`, `not_started`, `timeout` and `too_large`. Three
external plugins test the protocol first, and 0.2.0 freezes it by emptying
the list. An external plugin written against 0.1.0 may need changes in
0.2.0. The other `board::plugin::` codes, such as `not_installed` and
`bad_input`, are frozen from 0.1.0, because Board returns them for built-in
plugins and to CLI users too. Built-in plugins' types, capabilities and
codes are frozen from 0.1.0.

Until a later release, the `changes/*` types change in place while the
changes plugin is ported, and Board's own `board/*` types, which the
released test skips, are not listed. The index may change freely.

After 0.1.1, `runs/suite/v1` gained an optional `kind` in place, because
the release had few users. `runs.migrate` adds it to existing suites.

After 0.1.5, `plans/decision/v1` gained an optional `decides` in place,
because the release had few users. A decision without it decides no
single entry, so an approval written before it lapses and is asked again.
A board whose `.board/types/` holds the 0.1.5 copy refuses a decision that
carries `decides` until `board plugin enable` replaces it.

After 0.1.1, the `blocks` of `plans/question/v1` and the `subject` of
`artifacts/artifact/v1` gained `tasks/task/v3` in place, beside
`tasks/task/v2`, because `tasks add` writes v3 tasks and the release had few
users. A board whose `.board/types/` holds the 0.1.1 copies reports a
reference to a v3 task as `board::ref::wrong_type` until
`board plugin enable` replaces them.

The map plugin's `map/layout/v1` changes in place until map increment 4
lands, because increments 3 and 4 add optional fields to it. The unstable
list cannot hold it, because the list accepts only the external plugin
protocol's types, so the released test skips
`crates/board-plugin-map/types/` through `UNFROZEN` in
crates/board-cli/src/tests/released.rs, as it skips the changes plugin's
types. The change that lands increment 4 removes that entry and lists the
type. `map.check` and the `map::` codes are listed as released, because the
increments only add capabilities and codes beside them.

After 0.1.5, `rules/rule/v1` gained an optional `judged_by` in place,
because the release had few users, and released.json's entry for it is
rewritten for the new shape. The `rules.report` capability prints
`rules/report/v2` in place of `rules/report/v1`, so its released entry is
rewritten too. Board still ships the `rules/report/v1` schema, unchanged,
for readers that hold a report an earlier Board printed, and released.json
keeps listing it. A board whose `.board/types/` holds the older copies
refuses rules calls with `board::plugin::schema_differs` until `board
plugin enable rules` replaces them. A Board older than this change refuses
a rule that names `judged_by`, because `rules/rule/v1` admits no field it
does not list, so no rule should name `judged_by` until every machine that
reads the board runs a Board with this change.

After 0.1.6, `rules/rule/v1` gained a second form of `show`, an object
`{"stage": ..., "agent": ...}` that registers the rule against a workflow
stage, and an optional `provides`, in place, for the same reason, and
released.json's entry for it is rewritten again. Both are optional, and
`show` keeps every value it had. A Board older than this change refuses a
rule that uses either, so registrations should be written only once every
machine that reads the board runs a Board with this change.

The `--json` report of `board timing check`, `board/timing-check-report/v2`,
changes in place while the command timing budgets run silently in their first
week, because what that week shows may change the report's fields. The
released test leaves it out through the same `UNFROZEN` list, whose entries
match a report's `schema` as well as a schema file's path, and released.json
does not list its fields. The change that ends the silent week removes that
entry and lists the report.
