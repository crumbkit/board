# Runs: recording test, perf and eval results

Board records test results, perf results and eval results so they can be
referred to, built on and searched. PLAN.md links this leaf; when the
milestone lands, its settled design moves into docs/ and this file is
deleted.

An outside adopter, referred to here as adopter X, will later stop writing its
own test-run and startup-timing records and write them to Board by calling a
shipped `board` binary. Adopter X stays unchanged until the Board code below
is written.

## Build order

1. The `runs` plugin in viitta: `runs/suite/v1`, `runs/run/v1`, `latest` and
   `trend`.
2. `evals` refers to suites and lists runs (decisions 4 and 9). The evals
   skill changes in the same step.
3. `controls` gains a measure rule kind, and `controls.check` calls
   `runs.latest` (decision 5).
4. `cargo xtask bench --record` (decision 13).
5. The `github` plugin in `board-github` and the CI board (decisions 14 and
   15).
6. Adopter X moves onto runs once packaging is decided.

## Decided

1. **Composition.** Each record has one purpose. Schemas stay close to first
   normal form. An eval stays an eval; standing requirements belong in
   controls.
2. **No version bumps until a second user exists.** Schemas change in place.
   Each change updates the plugin's schemas, the copies in every board that
   enabled it (the personal board at ~/Code/board and viitta on this machine)
   and existing
   records together.
3. **A new `runs` plugin** owns two types:
   - `runs/suite/v1`: what gets run. Its fields are settled in decision 10.
   - `runs/run/v1`: one execution of a suite. `suite` ref, `by`, `started`,
     `finished`, `versions`, `scope` (full or targeted), `dirty`, host
     (`os`, `arch`, `logical_cpus`) as an embedded value, run-level measures,
     and results.
   `evals` and `controls` both refer to it. `runs` knows nothing about either.
4. **`evals/eval/v1` loses `method`, `cases` and `measures`** (they move to
   the suite) and refers to suites. It keeps the question, decision, verdict
   and conclusion. Decision 10 lists the suite's fields.
5. **`controls/control/v1` gains rule kinds that point at a suite**, for
   example "the latest run passes" or "measure `get_ms` is at most 5".
   `controls.check` judges only the latest full run of a clean revision and
   reports its outcome and age, or says no run qualifies. There is no filter
   on a control (AGENTS.md rule 6).
6. **Results.** A run records only the cases that did not pass, each with a
   short curated `note`, plus run-level measures `tests`, `failures`,
   `errors` and `skipped`. Board holds curated data: raw files such as
   `junit.xml` stay outside the board (adopter X keeps them in its state
   directory).
7. **Paths.** A suite is `/suites/<name>.json`. Its runs are
   `/suites/<name>/runs/<id>.json`, where the id is a sortable UTC timestamp
   plus a short random suffix, such as `20261018T101500Z-a3f9`.
8. **Where adopter X writes.** A dedicated board in adopter X's state
   directory, mounted into the personal board (~/Code/board), so its churn stays in its own history
   while evals and controls can read it.

9. **An eval lists the runs its verdict rests on.** `evals/eval/v1` gets
   `runs` (refs to `runs/run/v1`) beside `suites`. A run knows only its
   suite, so `runs` never depends on `evals`. `board refs` on a run shows the
   evals that cite it. The rejected alternatives were a run naming its eval
   and an eval storing a time window (a filter, against rule 6).

10. **The fields of `runs/suite/v1`.** A suite has these fields:
    - `command`: the command that runs the suite.
    - `cases` or `cases_file`: the cases, listed inline in `cases` or kept in
      the file that `cases_file` names. A suite has one or the other.
    - `measures`: the measures a run records, each with its unit.
    - `subjects`: what the suite tests.
    - `method`: how the suite tests its subjects. It moves here from
      `evals/eval/v1` (decision 4).

    A suite has no `name` field. Its name comes from its path: the suite at
    `/suites/<name>.json` is named `<name>`.

11. **Adopter X creates a suite on the suite's first run.** When adopter X
    records a run and `/suites/<name>.json` does not exist yet, adopter X
    writes the suite first and then the run.

12. **`runs` offers two capabilities.** Both consider only full runs of clean
    revisions, so the rule for which run counts lives only in `runs`.
    - `latest {suite}` returns the latest such run, with its outcome,
      measures and age, or says no run qualifies. `controls.check` calls it
      through `board call`.
    - `trend {suite, measure, limit}` returns one point per such run, newest
      first: the run's path, its `started` time and the measure's value. It
      reports how many points it left out. The bench and CI timings
      (decisions 13 and 14) are its first users.
    Flaky tests stay plain SQL through `board query` until a caller needs
    them.

13. **`cargo xtask bench --record` writes one run** to viitta's own board at
    `/suites/board-bench`, with one case per operation and a `median_ms`
    measure. The owner runs it locally. CI keeps printing to stdout, because
    CI cannot commit to a board. The budgets stay where they are (decision
    16).

14. **Board tracks CI performance across repositories.** The owner wants it
    because no one watches CI times across repos today. A local importer
    reads finished main-branch workflow runs from GitHub and writes one
    `runs/run/v1` per workflow run to a dedicated CI board, mounted into the
    personal board. Each repository and workflow is one suite, such as
    `/suites/viitta-ci`. Each job is a case with `duration_s` and `queued_s`
    measures, and failed jobs are non-passing results. PR runs are not
    recorded. CI never commits to a board.

15. **The CI importer is a new `github` plugin** in its own repository,
    `board-github`. It offers `github.ci {repo}`, which writes runs into the
    CI board through `board put`, one repository per call, continuing from
    the newest run already recorded so each call fits the 30-second limit.
    `runs` stays free of any source. It is the first importer of the kind
    PLAN.md's Adoption sketch describes.

16. **The bench budgets stay in
    docs/architecture/performance/budgets.md.** The bench keeps enforcing
    them in CI, and `runs.trend` shows the history. Controls with measure
    rules get their first use from CI timings or the adopter's suites.

## Still to decide

- Packaging: the plugins are Python run through `uv`. Adopter X's users would
  need Python and `uv`, or the plugins need to ship with the binary. Check
  whether `board put` needs only the schemas in the tree.
- PLAN.md settled decision 2 says Board is proprietary and unpublished. The
  owner plans to open-source Board (not adopter X); that decision is not
  settled.

## Adopter X's records, for reference

Moves to Board: adopter X's test-run record and its startup-run record.
Later and separately: task workspace state and Markdown, summaries with
experiments, friction reports.
Stays in adopter X: instance and workspace journals, cloud state, credentials,
gateway state and command history, which are machine-local operational state.
