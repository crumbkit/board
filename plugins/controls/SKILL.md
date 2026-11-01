---
name: controls
description: 'The controls plugin keeps requirements on files as records in the board that holds the files. Use it when you start work in a board or a repository it mounts, and before you edit a file a control may govern. Start with `board -C <board> call controls.check --path /<directory>`, which lists the budgets it measures and the written requirements under that path. `board -C <board> call controls.for-path --path /<file>` lists the controls on one file. SKILL.md and README.md in the plugin''s directory say more.'
---

# controls

Follow the requirements a board records for its files.

## Which board

Your instructions name the board that holds the controls. If they name none,
use the board you are in. A personal board can hold the controls of the
repositories it mounts, such as `/repos/crumbkit`; then ask it about a file
by its mounted path, as in `/repos/crumbkit/docs/README.md`.

To run a command in a board other than the one you are in, add `-C <path>`.

## Steps

1. When you start work, check the directory you will work in:

       board -C <board> call controls.check --path /<directory>

   `measured` lists the controls with a budget, with `used`, `max` and every
   file the budget counts. A budget with `per: "file"` limits each file on
   its own: `used` is its `largest` file's, and `over` lists each file over
   `max`. `stated` lists the written requirements, with
   their `paths` patterns and how many of their files are under the path;
   `board get <control>` shows which files a control names. The call exits
   1 when a budget is exceeded or a control is degraded, and the error names
   each control.
2. Before you edit a file, ask which controls apply to it:

       board -C <board> call controls.for-path --path /<file>

   Follow each requirement it returns. A new file is asked about by the
   path it will have.
3. Some controls are obligations: work that a landed change left owed. When
   your work does what one asks, delete the control in the commit that does
   the work, and name the work in the commit message. When your work leaves
   something owed, write a new control that says what is owed, which change
   owes it, and the file it applies to.

4. To check text before it leaves the checkout, such as a pull request
   body, pass the controls and the named parts on stdin:

       echo '{"controls": ["/<control>"], "parts": {"pr.body": "<text>"}}' | board -C <board> call controls.evaluate

   Each result says whether that part passed that control, with the line
   and text of each finding. Fix each failed part and evaluate it again.

The plugin's README.md gives the control record's fields and rules.
