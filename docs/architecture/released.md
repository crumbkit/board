# Released interfaces

Board's types, `--json` reports and codes change in place until a release or
a second user, and every reader on the machine moves in the same step
(STANDARDS.md 3 and 4). After a release, a reader built against one release
must read the records and reports of a later one, so:

- a changed type is a new version, `<namespace>/<kind>/v<n+1>`, and the old
  version's schema stays readable;
- an `enum` outside `if`, `not` and `oneOf` may gain a value, never lose one;
  an older Board may treat a record holding a newer value as invalid;
- a report or capability keeps every field, and a capability field its type;
  no input becomes required; a field may be added;
- a code is never renamed or removed; a code may be added;
- `plugin enable` reports each schema it replaced, in `replaced`.

Until a later release, the `changes/*` types change in place while the
changes plugin is ported, and Board's own `board/*` types, which have no
schema files, are not listed. The index may change freely.

## released.json

`released.json` at the repository root lists what the last release shipped:
each schema under `plugins/<name>/types/` or `crates/board-plugin-<name>/types/`,
as the blob id of its canonical JSON with those enums emptied, and their values
by JSON pointer; each document `board schemas list` names, and each built-in
capability, with the field paths (`[]` for every array element) their JSON
Schemas declare and sample runs print; and Board's and each built-in plugin's
codes. `.board/types/` holds the repository board's own copies and is not
listed.

The `released` test in `board-cli` refuses a listed schema whose shape changed,
and a listed enum value, field or code that is gone. Of `query` and `call`
only the envelopes are listed: a call's output is listed under
its capability.

A release writes the file by running that test with `SNAPSHOTS=overwrite`,
which lists everything the checkout ships, and then runs it with
`RELEASED=all`, which fails unless the file lists exactly that. Until the
first release the file lists nothing.
