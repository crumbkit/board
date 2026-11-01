# Control scope

A control can name the repositories it governs by reference, carry the
decision it comes from, add a step to the work on the files it covers, and
live in the repository it governs. An agent runs one command,
`controls.check`, and sees every control that applies to its work, from the
personal board and from the repository's own board.

## New fields

```json
{
  "$type": "controls/control/v1",
  "requirement": "Changes to secrets handling get a security review by a clean-context agent; high and critical findings are fixed before merge.",
  "purpose": "Secrets handling is where a mistake leaks a credential.",
  "summary": "Secrets module: adds a security review.",
  "repos": ["/repos/app"],
  "paths": ["src/secrets/*"],
  "adds_step": "security-review",
  "basis": ["/projects/app/decisions/20261031T1200Z-sec"]
}
```

- `summary` is one line of at most 120 characters that names the code the
  control covers and what it asks. `changes.brief` shows it, so a brief stays
  short. `controls.check` shows the full `requirement`, as today.
- `repos` lists repo record references (docs/architecture/paths/repos.md). With `repos`, each
  pattern in `paths` is matched against a file's path from the root of each
  listed repository, and the control reaches nothing outside them. A
  repository the board does not mount still matches, through files the
  caller names (below).
- `adds_step` names a workflow step, which the work records step builds. A
  change gains the step when a file in its files record matches the control.
  Until then the field is stored and shown, and adds nothing.
- `basis` lists the decisions and other records the control rests on. The
  decisions pull request already added it (plugins/controls/README.md); this step
  reads it.

A control ends when it is deleted, and git history keeps it. A control whose
`basis` names a decision copies that decision's rule into `requirement` when
the control is written, and agents follow the requirement. Changing the rule
means a new decision that supersedes the old one, and a new control citing
it in place of the old control. Until the old control is deleted,
`controls.check` marks it `basis_superseded`, so a control that outlives its
decision is reported. Decisions live in the personal board, and an absolute
reference inside a mounted board means that board's own path. So only a
personal-board control can name a personal-board decision in `basis`, and
a repository's own control names records in its own board.

Existing controls stay valid. Without `repos`, `paths` match from the
board's root, as today.

## The repository's own controls

A repository that is itself a board keeps its controls in its own tree, as
crumbkit/board does. Today the plugin reads controls only from the board's
own working tree, never from a mounted board. That rule changes for the
repository a check names: `controls.check --path /repos/<name>` also reads
the controls in that repository's own board, through the mount at its pinned
commit, with `board query` and `board get` as for any other record. Their
`paths` are anchored at the mount's path, so they match beneath
`/repos/<name>/`. It reads
controls as data and runs nothing from the repository. The answer depends
only on the pinned commit, so it is the same from a cold rebuild.

A repo record without a checkout has no tree to read, so the result says
`own_controls: "not_read"` and suggests mounting the repository.

The personal board's controls and the repository's own controls are listed
together. Each entry names its `board`.

## Files the caller names

The index holds a repository's mounted commit. It does not hold the files a
branch changes, and an unmounted repository has no files in the board. So
the caller passes the branch's files as `files`, an array of
repository-relative paths. An array is not a flag, so the input goes on
stdin as JSON:

    git diff --name-only origin/main... | jq -R . | jq -sc '{path: "/repos/app", files: .}' | board --personal call controls.check

Without `files`, `board --personal call controls.check --path /repos/<name>` lists
every control on the repository, which is what an agent needs when it
starts.

## Checking from a checkout

An agent in a checkout does not know its repository's path in the board.
So `controls.check` with neither `--path` nor `files.path` checks the
caller's repository: it finds the repo record whose `remote` equals
`shown_remote(caller_remote())` of `BOARD_CALLER_DIR` (docs/architecture/paths/repos.md) and
checks `/repos/<name>`. With no such record it says so and checks nothing.
The command from any checkout is therefore:

    board --personal call controls.check

This works in a checkout that is itself a board, where
`board call controls.check --path .` would read only that board's own
controls. The controls mod and the controls skill switch to this command.
The owner's signed controls order names the old command, which keeps its
meaning for repositories that are not boards; the owner re-signs the order
with the new command once this is built.

## Output

Each control in `stated` and `measured` gains `board`, `adds_step`,
`basis` and, when it applies, `basis_superseded`. Budget controls keep their measurement.

Today `check` returns every applying control with no limit, and adding a
repository's own controls makes the list longer. So `check` gains
`--limit`, with the default and maximum of the other listings, and each of
`stated` and `measured` reports its `total` and whether it was cut short.

## Build

One pull request in the controls plugin, its mod and its skill. The index
already holds a mounted board's records, so no crate changes. The tests:

- A control with `repos` matches a file in each listed repository and not
  the same path in another.
- `files` matches an unmounted repository's files.
- With no path, a check from a clone checks its repo record's path, and
  one from a clone with no record checks nothing and says so.
- `--limit` cuts each list and reports its total.
- The repository's own controls appear beside the personal board's, each
  with its board, and `own_controls` is `not_read` for a repo record without
  a checkout, with a twin that has one.

Done when `board --personal call controls.check`, run in a crumbkit/board
checkout, lists both the personal board's controls on it and the repository's own
controls.
