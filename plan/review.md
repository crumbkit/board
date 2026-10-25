# Review: briefs, reports and review evidence

The `review` plugin records which commits a review covered, what it found and
what it cost. A pull request carries a review report as its review evidence,
and a per-repository control checks that evidence in CI. PLAN.md links this
leaf; when the milestone lands, its settled design moves into docs/ and this
file is deleted.

## Build order

1. The manifest names a per-capability timeout and the environment variables
   a capability may read.
2. `review/brief/v1`, `review/report/v1` and `review.run`.
3. `review.post` posts a report as a pull request comment through the effect
   check.
4. `review.covered`, which CI calls to check a pull request is covered by
   reports.

The trial runs on the owner's own repositories, viitta first. Other Rust
repositories join as it proves itself.

## Decided

1. **Names.** The plugin is `review`. The author writes a review brief; a run
   produces a review report.
2. **`review/brief/v1`**, written by the author of a change:
   - `range`: the commits under review, as `base..head`.
   - `focus`: what matters in this change and what to check first.
   - `questions`: optional; what the author wants answered.
   - `follows`: optional; the report this brief continues from.
3. **`review/report/v1`**, written by `review.run` or a person:
   - `brief`: ref to the brief, absent for a human review.
   - `range` and `patch_ids`: the commits covered, and their `git patch-id`
     values.
   - `findings`: each with severity, file, line and text.
   - `verdict`: `approve`, `changes` or `comment`.
   - `by`: the agent and model, or the person.
   - `cost`: duration, and tokens when a model ran.
   - `source`: optional URL, such as a GitHub review or the posted comment.
   - `follows`: optional ref to the previous report, so reports chain.
4. **`review.run`** runs a brief with a clean-context agent. It writes the
   report and a `runs/run/v1` for the run. Its timeout and the variables it
   may read come from the manifest (build step 1). Decision 9 names the
   agent it starts.
5. **A review carries over a rebase** when the patch ids of the covered
   commits match the new commits.
6. **Anyone may post a report.** A person's GitHub review counts as review
   evidence and is recorded as a report with `source` set to its URL.
7. **Posting is an effect.** `review.post` asks `board decide --op
   review.comment --path <pull request URL>`. With no rule the answer is
   ask (plan/board-ui.md decision 9).
8. **The coverage check** is `review.covered --base <sha> --head <sha>`,
   which CI runs on each pull request.
   - It passes when reports cover every commit from the merge base to the
     head, by range or by patch id.
   - A report whose verdict is `changes` does not count.
   - It reads reports committed to the repository's own board.
   - It skips commits that only add review records, so committing a report
     does not make a new head that no report covers.
   - Each repository that uses review states this as a written control, so
     `controls.check` names it. A controls rule would fail every session
     start on an unreviewed branch.
9. **The agent command.** `review.run` starts the command named in the
   board's settings. The documented example is `claude -p`.
