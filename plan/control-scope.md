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
  "basis": ["/projects/app/decisions/20261031T1200Z-sec"],
  "supersedes": []
}
```

- `summary` is one line of at most 120 characters, saying what the control
  governs. Briefs and `controls.check` show it in place of the requirement,
  so it names the code it covers, not only the rule.
- `repos` lists repo record references (plan/repos.md). With `repos`, each
  pattern in `paths` is matched against a file's path from the root of each
  listed repository, and the control reaches nothing outside them. A
  repository the board does not mount still matches, through files the
  caller names (below).
- `adds_step` names a workflow step (plan/work-records.md). A change gains
  the step when any file it touches matches the control.
- `basis` and `supersedes` mean what they mean on a decision
  (plan/decisions.md). A control that a later control supersedes applies to
  nothing, and `controls.check` lists it under `superseded` only when asked
  with `--all`.

Existing controls stay valid. Without `repos`, `paths` match from the
board's root, as today.

## The repository's own controls

A repository that is itself a board keeps its controls in its own tree, as
crumbkit/board does. `controls.check --repo /repos/<name>` reads them from
the repo record's checkout when it has one, or else from the current
directory when that is a clone of the repository. It reads them as data,
never as code, so "mounting brings no code" holds. When neither is
available, the result says `own_controls: "not_read"` and why, rather than
leaving them out silently.

The personal board's controls and the repository's own controls are listed
together. Each entry names its `board`, so an agent can see which board a
control lives in.

## Files the caller names

A plugin never reads git. For files on a branch that the board has not
mounted, the caller passes the paths:

    git diff --name-only origin/main... | board call controls.check --repo /repos/app --files -

`--files -` reads one repository-relative path per line. Without `--files`,
`controls.check --repo` lists every control on the repository, which is what
an agent needs when it starts.

## Output

Each control in `stated` and `measured` gains `summary`, `board`,
`adds_step` and `basis`. Budget controls keep their measurement. The output
stays bounded by default and says what it left out, as the CLI rule
requires.

## Build

One pull request in the controls plugin. The tests:

- A control with `repos` matches a file in each listed repository and not
  the same path in another.
- `--files` matches an unmounted repository's files.
- The repository's own controls appear beside the personal board's, each
  with its board, and `own_controls` says `not_read` when there is no
  checkout or clone, with a twin that reads them.
- A superseded control applies to nothing.

Done when `controls.check --repo` in a clone of crumbkit/board lists both
the personal board's controls on it and the repository's own controls.
