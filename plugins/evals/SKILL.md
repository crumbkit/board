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

## Find the repository's mount

The personal board mounts each repository at `/repos/<name>`: a
`board/mount/v1` record whose `repo` is the repository's path. Find the mount
of the repository you are in before you test.

1. Take the repository's path from
   `dirname "$(git rev-parse --path-format=absolute --git-common-dir)"`. It
   gives the main checkout even from a worktree.
2. `board -C <personal board> ls /repos` lists the mounts, and fails with
   `board::path::not_found` when there are none. Each is a file
   `repos/<name>.json` in the personal board's directory; read them. The one
   whose `repo` is that path (after expanding a leading `~/`) is the mount.
3. If none matches, add one named after the repository's directory, with
   `commit` set to the output of `git -C <path> rev-parse origin/main`:

       echo '{"$type": "board/mount/v1", "repo": "<path>", "follow": "origin/main", "commit": "<commit>"}' \
         | board -C <personal board> put /repos/<name>

   `board mount update /repos/<name>` later moves `commit` to the current
   `origin/main`. It cannot create a mount.

## Steps

1. Before testing anything, look for earlier evals. Run
   `echo '{"repo": "/repos/<name>"}' | board call evals.about` with the
   repository's mount, and `echo '{"subject": "<name>"}' | board call evals.about`
   with each thing you will test. The `repo` lookup finds the evals that
   name the mount in `repos` and the evals stored in the repository's own
   board. It reads that board through the mount at the mount's `commit`, so
   it misses evals the repository added after that commit. To see them, run
   `git -C <path> fetch`, then `board mount update /repos/<name>`, which
   moves `commit` to the fetched `origin/main`. Evals not yet on
   `origin/main`, such as ones on your branch, are not seen this way.

   Ask the personal board and any board the owner named for sharing.
   `echo '{}' | board call evals.open` lists the evals still planned or
   running.
2. Run the remaining steps in the board you chose. Before gathering
   evidence, write `evals/eval/v1` with `board put /evals/<name> < eval.json`.
   Give the question, the subjects, the method, the cases and the measures,
   and `status: planned`. The subjects are the names of what is tested, such
   as `crumbkit` or `clap`. Put `repos: ["/repos/<name>"]` with the mount of
   each repository the eval concerns, so an agent working there finds it.
   A `repos` path is read in the board that holds the eval, and `board check`
   reports `board::ref::dangling` when that board has no such mount. In the
   repository's own board, or a shared board without the mount, leave
   `repos` out.
3. Write one `evals/run/v1` at `/evals/<name>/runs/<id>` per run, with each
   case's outcome or measures and the versions used. Save raw files, such as
   logs, under `evals/<name>/evidence/` in that board's git repository,
   commit them, and reference them from the results.
4. Set `status: running` at the first run.
5. Conclude with `verdict` and `conclusion`, and set `status: concluded`.

A personal board that mounts a repository at `/repos/<name>` lists that
repository's evals in `about` and `open`. The plugin's README says how to
write through such a mount.
