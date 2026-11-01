---
name: evals
description: 'Record every test of a library, tool, model or agent as an eval and its runs in the board, and check what was already tested before testing. Use it before you start a test and when you conclude one. Start with `board -C <board> call evals.about --subject <name>`, which lists the evals of what you will test. `board -C <board> call evals.open` lists those still planned or running. SKILL.md and README.md in the plugin''s directory say more.'
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
  the repository's board if that board enables the evals and runs plugins.
  It does when `board plugin list`, run in the repository, shows both as
  `enabled`. Otherwise write it in the personal board.

A decision, such as whether to adopt a library, never goes in a repository's
board.

To run a command in a board other than the one you are in, add `-C <path>`,
as in `board -C ~/Code/personal-board put ...`.

## Find the repository's repo record

The personal board names each repository with a `board/repo/v1` record at
`/repos/<name>`, whose `remote` is the repository's origin in `owner/name`
form for GitHub; one with a `checkout` also mounts the repository there
(docs/architecture/paths/repos.md in Board's repository). Find the record of
the repository you are in before you test.

1. Take the origin from `git remote get-url origin`.
2. `board -C <personal board> ls /repos` lists the records, and fails with
   `board::path::not_found` when there are none. Each is a file
   `repos/<name>.json` in the personal board's directory; read them. The one
   whose `remote` is that origin in `owner/name` form is the record. An older
   board may hold a `board/mount/v1` mount there instead, whose `repo` is the
   repository's path; it serves the same way.
3. If none matches, add one named after the repository:

       echo '{"$type": "board/repo/v1", "remote": "<owner>/<name>"}' \
         | board -C <personal board> put /repos/<name>

   Add a `checkout` only when the owner wants the repository mounted, as the
   repo records doc says.

## Steps

1. Before testing anything, look for earlier evals. Run
   `board call evals.about --repo /repos/<name>` with the repository's
   repo record, and `board call evals.about --subject <name>` with each
   thing you will test. The `repo` lookup finds the evals that name it in
   `repos` and, when the record mounts the repository, the evals stored in
   the repository's own board. It reads that board at the mount's `commit`,
   so it misses evals the repository added after that commit. To see them,
   run `git -C <path> fetch`, then `board mount update /repos/<name>`, which
   moves `commit` to the fetched `origin/main`. Evals not yet on
   `origin/main`, such as ones on your branch, are not seen this way.

   Ask the personal board and any board the owner named for sharing.
   `board call evals.open` lists the evals still planned or
   running.
2. Run the remaining steps in the board you chose. Before gathering
   evidence, write `evals/eval/v1` with `board put /evals/<name> < eval.json`.
   Give the question and the subjects, and in `informs` the question of the
   decision it informs, if any. Nothing stores the status: an eval with no
   runs is planned. The subjects are the names of what is tested, such as
   `crumbkit` or `clap`. Put `repos: ["/repos/<name>"]` with the repo record
   of each repository the eval concerns, so an agent working there finds it.
   A `repos` path is read in the board that holds the eval, and
   `board check` reports `board::ref::dangling` when that board has no such
   record. In the repository's own board, or a shared board without the
   record, leave `repos` out.
3. Say how it is tested in a `runs/suite/v1` at `/suites/<suite>`: the
   subjects, the method, the cases and the measures, each with its unit.
   `board ls /suites` lists the suites; reuse one that already tests this.
   List the suite in the eval's `suites`.
4. Write one `runs/run/v1` per run at `/suites/<suite>/runs/<id>`, where the
   id is a UTC timestamp and a short random suffix, such as
   `20261018T101500Z-a3f9`. Give `by`, `started`, `scope` (`full` when every
   case ran), `dirty` when you know whether the tree under test had
   uncommitted changes, the versions used and any `setup`. List each case's
   outcome or measures, numbering repeated trials of a case with `trial`
   from 1. A case that did not pass needs a `note` saying why. Save raw
   files, such as logs, under `evals/<name>/evidence/` in that board's git
   repository, commit them, and reference them from the results. Add the
   run to the eval's `runs`; from the first run the eval is running.
5. Conclude with `verdict` and `conclusion`; with a verdict the eval is
   concluded. The eval's `runs` lists the runs the verdict rests on. If you
   stop before concluding, say why in `abandoned`. When the eval leads to a
   decision, record a `decisions/decision/v1` whose `basis` names the eval
   (plugins/decisions/README.md); `evals.about` then lists it in the eval's
   `decisions`.
6. After an `inconclusive` verdict, the `conclusion` says what would make
   the question conclusive, such as more trials, fixing a cause of error,
   blind scoring or a threshold set in advance. Keep the verdict and choose
   one:
   - Write a narrower eval that tests one of those changes now, with
     no runs yet (step 2), and give its path in this eval's
     `conclusion`. Give it a new suite whose `method` states the thresholds
     for `supported` and `refuted`, so the earlier runs keep their method.
   - Say in the `conclusion` why the question is not worth pursuing.

   Tell the owner which you chose, because the owner may choose the other.

A personal board that mounts a repository at `/repos/<name>` lists that
repository's evals in `about` and `open`. The plugin's README says how to
write through such a mount.
