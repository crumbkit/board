---
name: rules
description: 'The rules plugin, formerly controls, keeps requirements on files as records in the board that holds the files. Use it when you start work in a board or a repository it mounts, and before you edit a file a rule may govern. Start with `board -C <board> rules check --path /<directory>`, which lists the budgets it measures and the written requirements under that path. `board -C <board> rules for-path --path /<file>` lists the rules on one file. `board -C <board> rules evaluate` checks text, such as a draft, against the checkers of the rules it names before the text is written. SKILL.md and README.md in the plugin''s directory say more.'
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

       board -C <board> rules check --path /<directory>

   `measured` lists the controls with a budget, with `used`, `max` and every
   file the budget counts under the path. A budget with `per: "file"` limits each file on
   its own: `used` is its `largest` file's, and `over` lists each file over
   `max`. `versions` lists the controls with a version rule, and `behind`
   names each file and line that pins a version older than `at_least`.
   `stated` lists the written requirements, with
   their `paths` patterns and how many of their files are under the path;
   `board get <control>` shows which files a control names. `passed` is
   false when a budget is exceeded, a pin is behind or a measured control
   lists a file in `unreadable`; the call still exits 0. It exits 1 when a
   control is degraded or a version rule's capture is invalid, and the error
   names each control.

   A mount is read at the commit its pointer pins, which follows the
   repository's main branch. On a branch, commit your edits and run the
   check from your checkout with `--checkout`, so it reads your HEAD:

       board --checkout rules check --path .

   `read` names each mount the files came from and the commit it read.
   When you run the check from a checkout without `--checkout`, and its
   HEAD is not the commit read or it has uncommitted changes to tracked
   files, the output carries `checkout`. It names both commits and gives
   in `run` the command that measures your HEAD. When HEAD differs from
   the commit read, the check did not measure your branch, whatever
   `passed` says. When `uncommitted` is true, HEAD is the commit read, so
   the check measured your last commit but missed your uncommitted edits;
   commit them and run the command in `run`.
2. Before you edit a file, ask which controls apply to it:

       board -C <board> rules for-path --path /<file>

   Follow each requirement it returns. A new file is asked about by the
   path it will have. For several files or a pattern, such as
   `/repos/<name>/crates/store/**`, pass them on stdin:

       echo '{"paths": ["/<pattern>", "/<file>"]}' | board -C <board> rules for-paths
3. Some controls are obligations, with `kind: obligation`: work that a
   landed change left owed, and the output gives each its `close` line.
   Once the work one names is committed, delete it with
   `board rm <path> -m '<message naming the work>'`, so the board's
   history says which work closed it; merging the work does not clear it. When
   your work leaves something owed, write a new control with
   `kind: obligation` that says what is owed, which change owes it, and the
   file it applies to. Set `owed_by` to the path of the record that owes
   it: a `github/pull/v1`, a `review/report/v1` or a `plans/decision/v1`.

4. To check text before it leaves the checkout, such as a pull request
   body, pass the controls and the named parts on stdin:

       echo '{"rules": ["/<rule>"], "parts": {"pr.body": "<text>"}}' | board -C <board> rules evaluate

   Each result says whether that part passed that control, with the line
   and text of each finding. Fix each failed part and evaluate it again.

The plugin's README.md gives the control record's fields and rules.
