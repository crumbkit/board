# places

A Board plugin that joins an mxv workspace on one machine to the Board change
it works on. `attach` writes a join, `detach` removes one, and `list` prints
this machine's joins with each change's title and phase and the workspace's
status from mxv.

## Enabling

Board has the plugin built in, so nothing is installed:

    board plugin enable places

- Enabling writes the schema it ships to `.board/types/places/`.
- Each command reads and writes the board it runs in. Joins belong in the
  personal board, so callers run `board --personal places ...`.

## Records

`places/workspace-change/v1` is one join at
`/places/<machine>/<workspace>/changes/<change-name>`, such as
`/places/quokka/fix-login/changes/fix-login`:

```json
{
  "$type": "places/workspace-change/v1",
  "workspace": "/places/quokka/fix-login",
  "change": "/changes/fix-login",
  "workspace_id": "0123456789abcdef0123456789abcdef",
  "since": "2026-11-20T09:00:00Z",
  "by": {"program": "claude-code"}
}
```

- `workspace` is a reference to the join's own grandparent directory,
  `/places/<machine>/<workspace>`. `board refs` on that directory lists the
  workspace's joins.
- `change` is a reference to a `changes/change/v2` record. `board refs` on the
  change lists every workspace joined to it, on every machine.
- `workspace_id` is the mxv workspace's id when the join was written: 32
  lowercase hex digits.
- `since` is when the join was written, in UTC to the second.
- `by` is who attached it, when the call says.
- The record holds no absolute path, branch, port, project or state. mxv owns
  those values.

## The machine label

- The label is the second segment of every join's path.
- It comes from the first of these that names anything: `BOARD_MACHINE`, the
  first line of `~/.config/board/machine`, `scutil --get LocalHostName`, then
  `hostname -s`.
- The name is lower-cased, and each run of characters other than `a` to `z`
  and `0` to `9` becomes `-`. A `-` at either end is dropped.
- Joins of other machines reach the board through git and stay under their
  own machine's directory.

## Capabilities

`attach --change <path> [--workspace <name>]`:

- It runs `mxv list --all --json --fields name,id,path` with 10 seconds to
  answer.
- The workspace is the live one `--workspace` names, or the one whose
  directory holds the directory `board` was started in.
- It refuses a change path that holds no `changes/change/v2` record.
- With no join at the path, it writes one with a put that expects nothing
  there.
- With a join to the same change and the same id, it writes nothing and
  reports `written` false.
- With a join to the same change and another id, it rewrites the join with the
  new id and reports `replaced` true.
- `session`, `program`, `person` and `agent` fill `by`.
- The output holds `path`, `machine`, `workspace`, `change`, `workspace_id`,
  `written` and `replaced`.

`detach --change <path> [--workspace <name>]`:

- With `--workspace` it runs no mxv. Without it, it finds the workspace as
  `attach` does.
- It removes the join when it joins the workspace to that change, and
  reports `removed` false otherwise.
- The output holds `path` and `removed`.

`list [--limit <n>] [--all true]`:

- It reads this machine's joins with one query, and their changes with one
  read.
- It runs `mxv list` and `mxv list --archived`, each with
  `--all --json --fields name,id,path`.
- `limit` is 1 to 10,000 and 100 by default. `all` returns every join.
- Each row holds `path`, `workspace`, `workspace_id`, `since`, `change` and
  `status`. `change` holds `path`, `title` and `phase`. Title and phase are
  null when the change cannot be read, and a warning says why.
- A join the plugin cannot read is left out with a warning.
- The output holds `machine`, `joins`, `total` and `truncated`.

Workspace statuses:

- `live`: a live workspace of that name has the join's id.
- `replaced`: a live workspace of that name has another id.
- `archived`: no live workspace has that name, and an archived one of that
  name has the join's id.
- `gone`: no workspace of that name is live or archived.
- `unknown`: mxv prints no id for the live workspace of that name, or no
  live one exists and the archived ones of that name have no id or another
  id.

Every capability is given `BOARD_MACHINE`, `MXV_ROOT` and `XDG_STATE_HOME`.

## Codes

- `places::machine::unnamed`: no source gives a label.
- `places::mxv::failed`: mxv is not on PATH, did not start, exited non-zero,
  ran past 10 seconds, printed more than 16 MiB, or printed no `mxv/list/v1`
  document.
- `places::mxv::no_id`: mxv is older than 0.3.0. It refused
  `--fields name,id,path`, or printed rows with no `id`.
- `places::mxv::cut`: mxv's listing says `truncated` even with `--all`.
- `places::workspace::no_id`: mxv prints a null `id` for the workspace
  `attach` found. The message names mxv's `problem` when the row has one.
- `places::workspace::outside`: no `--workspace`, and no workspace holds the
  caller's directory.
- `places::workspace::not_found`: no live workspace has the name given.
- `places::change::not_found`: nothing is at the change's path.
- `places::change::wrong_type`: the change's path holds a directory or a
  record of another type.
- `places::attach::taken`: the path holds a join to another change with the
  same last segment. `fields.change` names it.
- `places::record::unreadable`: the record at the join's path is not a join
  this plugin reads. `fields.path` names it.
- `places::board::refused`: Board refused a read or a write. `fields.code`
  holds Board's code.
- `places::call::unknown_capability`: the plugin offers `attach`, `detach` and
  `list`.

## Developing

- The plugin is the crate `crates/board-plugin-places`.
- Its `types/` directory holds the join's schema as a committed golden, which
  a test compares with what `WorkspaceChange` generates.
  `SNAPSHOTS=overwrite` rewrites it.
- The crate's tests run each capability over a real board with a host that
  answers for mxv, `scutil` and `hostname`.
- The tests through the binary are in
  `crates/board-cli/src/tests/built_in/places.rs`. They put a stub `mxv` first
  on the PATH the plugin's processes see, and set `BOARD_MACHINE`.
