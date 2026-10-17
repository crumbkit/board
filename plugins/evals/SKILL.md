---
name: evals
description: Record every test of a library, tool, model or agent as an eval and its runs in the board, and check what was already tested before testing. Use before you start a test and when you conclude one.
---

# evals

Record every test of a library, tool, model or agent as an eval.

## Which board

Your instructions name your personal board. If they name none, ask the owner
for its path. An eval goes in the personal board unless one of these applies:

- The owner names another board to share it in, such as a team's board.
  Write it there.
- The eval tests the code of the repository you are in, and its cases should
  change with that code, as an agent's regression suite does. Write it in
  the repository's board if that board enables the evals plugin. It does
  when `board plugin list`, run in the repository, shows `evals` as
  `enabled`. Otherwise write it in the personal board.

A decision, such as whether to adopt a library, never goes in a repository's
board.

To run a command in a board other than the one you are in, add `-C <path>`,
as in `board -C ~/Code/personal-board put ...`.

## Steps

1. Before testing anything, look for earlier evals. Run
   `echo '{"subject": "<name>"}' | board call evals.about` with each of
   these subjects:
   - each thing you will test;
   - the repository you are in, as `owner/name`. Take it from
     `git remote get-url origin`: `git@github.com:crumbkit/crumbkit.git`
     gives `crumbkit/crumbkit`.

   Ask each of these boards: the personal board, any board the owner named
   for sharing, and the repository's board if it enables the plugin.
   `echo '{}' | board call evals.open` lists the evals still planned or
   running.
2. Run the remaining steps in the board you chose. Before gathering
   evidence, write `evals/eval/v1` with `board put /evals/<name> < eval.json`.
   Give the question, the subjects, the method, the cases and the measures,
   and `status: planned`. The subjects include each repository the eval
   concerns, as `owner/name`, so an agent working there finds it.
3. Write one `evals/run/v1` at `/evals/<name>/runs/<id>` per run, with each
   case's outcome or measures and the versions used. Save raw files, such as
   logs, under `evals/<name>/evidence/` in that board's git repository,
   commit them, and reference them from the results.
4. Set `status: running` at the first run.
5. Conclude with `verdict` and `conclusion`, and set `status: concluded`.

A personal board that mounts a repository at `/repos/<repo>` lists that
repository's evals in `about` and `open`. The plugin's README says how to
write through such a mount.
