---
name: evals
description: Record every test of a library, tool, model or agent as an eval and its runs in the board, and check what was already tested before testing. Use before you start a test and when you conclude one.
---

# evals

Record every test of a library, tool, model or agent as an eval.

1. Before testing anything, run
   `echo '{"subject": "<name>"}' | board call evals.about` to see what has
   been tested already. `echo '{}' | board call evals.open` lists the evals
   still planned or running.
2. Before gathering evidence, write `evals/eval/v1` with
   `board put /evals/<name> < eval.json`. Give the question, the subjects, the
   method, the cases and the measures, and `status: planned`.
3. Write one `evals/run/v1` at `/evals/<name>/runs/<id>` per run, with each
   case's outcome or measures and the versions used. Save raw files, such as
   logs, under `evals/<name>/evidence/` in the repository, commit them, and
   reference them from the results.
4. Set `status: running` at the first run.
5. Conclude with `verdict` and `conclusion`, and set `status: concluded`.

Where to write:

- Write in the board of the repository the eval is about, at `/evals/...`.
  That repository must have run `board plugin enable evals` and committed the
  result, because a board validates records against its own schemas.
- A personal board that mounts the repository at `/repos/<repo>` lists its
  evals in `about` and `open`. It cannot write them yet: `board put` to
  `/repos/<repo>/...` is refused with `board::put::through_mount` until Board
  can write through a mount. After a put in the repository, run
  `board mount update /repos/<repo>` in the personal board to see it there.
