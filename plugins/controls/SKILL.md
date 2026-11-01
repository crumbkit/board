---
name: controls
description: 'The controls plugin keeps requirements on files as records in the board that holds the files. Use it when you start work in a board or a repository it mounts, and before you edit a file a control may govern. Start with `board --personal call controls.check` in the repository, which lists the budgets it measures and the written requirements on it, the repository''s own included. `board -C <board> call controls.for-path --path /<file>` lists the controls on one file. SKILL.md and README.md in the plugin''s directory say more.'
---

# controls

Follow the requirements a board records for its files.

## Which board

Your instructions name the board that holds the controls. If they name none,
use your personal board, which `--personal` names. A personal board can hold
the controls of the repositories it mounts or records, such as
`/repos/crumbkit`; then ask it about a file by its mounted path, as in
`/repos/crumbkit/docs/README.md`.

To run a command in a board other than the one you are in, add `-C <path>`.

## Steps

1. When you start work, check the repository you will work in, from inside
   it:

       board --personal call controls.check

   It finds the repository by its clone or its origin, lists the personal
   board's controls on it, and, when the repository is itself a board, the
   repository's own controls too; each control's `board` says which board
   holds it. Pass `--path /<directory>` to check a directory instead. To
   check only the files a branch changes, which also works for a repository
   the board does not mount, send them on stdin:

       git diff --name-only origin/main... | jq -R . | jq -sc '{files: .}' | board --personal call controls.check

   `measured` lists the controls with a budget, with `used`, `max` and every
   file the budget counts. A budget with `per: "file"` limits each file on
   its own: `used` is its `largest` file's, and `over` lists each file over
   `max`. `stated` lists the written requirements, with
   their `paths` patterns and how many of their files are under the path;
   `board get <control>` shows which files a control names. Each list holds
   at most `--limit` controls, and `truncated` says when it was cut. A
   control with `basis_superseded` rests on a decision that a later one
   replaced; say so to the owner. The call exits 1 when a budget is
   exceeded or a control is degraded, and the error names each control.
   When `path` is null, the board knows no such repository and `next` says
   what to do.
2. Before you edit a file, ask which controls apply to it:

       board -C <board> call controls.for-path --path /<file>

   Follow each requirement it returns. A new file is asked about by the
   path it will have.
3. Some controls are obligations: work that a landed change left owed. When
   your work does what one asks, delete the control in the commit that does
   the work, and name the work in the commit message. When your work leaves
   something owed, write a new control that says what is owed, which change
   owes it, and the file it applies to.

The plugin's README.md gives the control record's fields.
