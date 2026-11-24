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

After 0.1.1, the `blocks` of `plans/question/v1` and the `subject` of
`artifacts/artifact/v1` gained `tasks/task/v3` in place, beside
`tasks/task/v2`, because `tasks add` writes v3 tasks and the release had few
users. A board whose `.board/types/` holds the 0.1.1 copies reports a
reference to a v3 task as `board::ref::wrong_type` until
`board plugin enable` replaces them.
