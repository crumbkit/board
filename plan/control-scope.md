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
- `repos` lists repo record references (plan/repos.md). With `repos`, each
  pattern in `paths` is matched against a file's path from the root of each
  listed repository, and the control reaches nothing outside them. A
  repository the board does not mount still matches, through files the
  caller names (below).
- `adds_step` names a workflow step (plan/work-records.md). A change gains
  the step when any file it touches matches the control.
- `basis` lists the decisions and other records the control rests on.

A control ends when it is deleted, and git history keeps it. A control whose
`basis` names a decision states that decision's rule as its `requirement`,
the text agents follow. Both records are written once, so the two cannot
drift apart. Changing the rule means a new decision that supersedes the old
one, and a new control citing it in place of the old control.

Existing controls stay valid. Without `repos`, `paths` match from the
board's root, as today.

## The repository's own controls

A repository that is itself a board keeps its controls in its own tree, as
crumbkit/board does. Today the plugin reads controls only from the board's
own working tree, never from a mounted board. That rule changes for the
repository a check names: `controls.check --path /repos/<name>` also reads
the controls in that repository's own board. `board-tree` resolves the
repository for the plugin, through the repo record's checkout or through the
clone the current directory is in (plan/repos.md), so the plugin reads only
resolved Locations. It reads controls as data and runs nothing from the
repository.

When Board can resolve neither, the result says `own_controls: "not_read"`
and gives the reason.

The personal board's controls and the repository's own controls are listed
together. Each entry names its `board`.

## Files the caller names

The index holds a repository's mounted commit, not the files a branch
changes, and an unmounted repository has no files in the board at all. So
the caller passes the branch's files:

    git diff --name-only origin/main... | board call controls.check --path /repos/app --files -

`--files -` reads one repository-relative path per line. Without `--files`,
`controls.check --path /repos/<name>` lists every control on the
repository, which is what an agent needs when it starts.

## Output

Each control in `stated` and `measured` gains `board`, `adds_step` and
`basis`. Budget controls keep their measurement. The output stays bounded by
default and says what it left out.

## Build

One pull request in `board-tree` and the controls plugin. The tests:

- A control with `repos` matches a file in each listed repository and not
  the same path in another.
- `--files` matches an unmounted repository's files.
- The repository's own controls appear beside the personal board's, each
  with its board, and `own_controls` is `not_read` when Board resolves no
  checkout or clone, with a twin that reads them.

Done when `controls.check --path /repos/board` in a clone of crumbkit/board
lists both the personal board's controls on it and the repository's own
controls.
