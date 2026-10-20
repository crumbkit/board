# Released interfaces

Board's types, `--json` reports and codes change in place until a release or
a second user, and every reader on the machine moves in the same step
(STANDARDS.md 3 and 4). After a release, a reader built against one release
must read the records and reports of a later one, so:

- a changed type is a new version, `<namespace>/<kind>/v<n+1>`, and the old
  version's schema stays where it was and stays readable;
- a `--json` report keeps every field it had; a field may be added;
- a code is never renamed or removed; a code may be added;
- `plugin enable` reports each schema it replaced, in `replaced`.

The index is rebuilt from the tree, so its format may change freely.

## released.json

`released.json` at the repository root lists what the last release shipped:
each schema file by its path and the git blob id of its bytes, each report by
its `schema` with the paths of the fields its `data` holds (`[]` standing for
every element of an array), and the codes. The `released` test in `board-cli`
reads the file and refuses a listed schema whose bytes changed, a listed field
that is gone and a listed code that is gone; a new version, a new field or a
new code is admitted. The `data` of `query` is the statement's rows and the
`data` of `call` is the plugin's output, so neither is listed.

A release writes the file by running that test with `SNAPSHOTS=overwrite`,
which lists everything the checkout ships, and then runs it with
`RELEASED=all`, which fails unless the file lists exactly that. Until the
first release the file lists nothing.
