# Released interfaces

Board's types, `--json` reports and codes change in place until a release or
a second user, and every reader on the machine moves in the same step
(STANDARDS.md 3 and 4). After a release, a reader built against one release
must read the records and reports of a later one, so:

- a changed type is a new version, `<namespace>/<kind>/v<n+1>`, and the old
  version's schema stays where it was and stays readable;
- an `enum` may gain a value in place but never loses one, and a reader
  treats a value it does not know as unknown;
- a `--json` report keeps every field it had; a field may be added;
- a code is never renamed or removed; a code may be added;
- `plugin enable` reports each schema it replaced, in `replaced`.

The `changes/*` types change in place until a later release, while the
changes plugin is ported. The index may change freely.

## released.json

`released.json` at the repository root lists what the last release shipped.
Each schema under `plugins/<name>/types/` or `crates/board-plugin-<name>/types/`
is listed with the blob id of its canonical JSON with every `enum` emptied, and
each `enum`'s values by JSON pointer. Each document `board schemas list` names
is listed with its field paths (`[]` for every array element): those its JSON
Schema declares and those a sample run prints in `data`. The codes are Board's
and each built-in plugin's. `.board/types/` holds the repository board's own
copies and is not listed.

The `released` test in `board-cli` refuses a listed schema whose shape changed,
and a listed enum value, field or code that is gone. The `data` of `query` and
`call` is not Board's, so only their envelopes are listed.

A release writes the file by running that test with `SNAPSHOTS=overwrite`,
which lists everything the checkout ships, and then runs it with
`RELEASED=all`, which fails unless the file lists exactly that. Until the
first release the file lists nothing.
