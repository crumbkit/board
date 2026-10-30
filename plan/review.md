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
   check, asking with `board decide`.
4. `review.covered`, which CI calls to check a pull request is covered by
   reports.

The trial runs on the owner's own repositories, board first. Other Rust
repositories join as it proves itself.

## Decided

1. **Names.** The plugin is `review`. The author writes a review brief; a run
   produces a review report.
2. **`review/brief/v1`**, written by the author of a change, at
   `/reviews/<id>/brief.json`:
   - `range`: the commits under review, as `base..head`. The schema allows
     only two full object ids joined by `..`, and every git call puts
     `--end-of-options` before revisions, so a crafted range cannot pass git
     an option such as `--output`.
   - `focus`: what matters in this change and what to check first.
   - `questions`: optional; what the author wants answered.
   - `follows`: optional; the report this brief continues from.
3. **`review/report/v1`**, written by `review.run` or a person, at
   `/reviews/<id>/report.json`:
   - `brief`: ref to the brief, absent for a human review.
   - `range` and `patch_ids`: the commits covered, and their `git patch-id`
     values, which the schema allows only as hex.
   - `findings`: each with severity, file, line and text.
   - `verdict`: `approve`, `changes` or `comment`.
   - `by`: the agent and model, or the person.
   - `cost`: duration, and tokens when a model ran.
   - `source`: optional URL, such as a GitHub review or the posted comment.
   - `follows`: optional ref to the previous report, so reports chain.
4. **`review.run`** runs a brief with a clean-context agent and writes the
   report, whose `by` and `cost` record the run. Its timeout and the variables it
   may read come from the manifest (build step 1). Decision 9 names the
   agent it starts.
5. **A review carries over a rebase** when the patch ids of the covered
   commits match the new commits. A patch id is `git diff-tree -p
   --no-renames --no-ext-diff --no-textconv --full-index <commit> | git
   patch-id --verbatim`, run without global or system git config, so the
   author's machine and CI agree and a whitespace-only change is a new
   patch. `review` runs `git` itself, as `notes` does.
6. **Anyone may post a report.** A person's GitHub review counts as review
   evidence and is recorded as a report with `source` set to its URL.
7. **Posting is an effect.** `review.post` asks `board decide --op
   review.post --path <report path>`, and the ask shows the whole comment,
   since an agent wrote it from an untrusted diff.
8. **The coverage check** is `review.covered --base <sha> --head <sha>`,
   which CI runs on each pull request.
   - It passes when reports cover every commit from the merge base to the
     head, by range or by patch id. A merge commit or an empty commit has no
     patch id, so only a report whose range contains it covers it.
   - A report whose verdict is `changes` does not count.
   - It reads reports committed to the repository's own board.
   - It skips commits that only add valid `review/*` records under
     `/reviews/`, so committing a report does not make a new head that no
     report covers. A merge, or a commit that changes or deletes a file, is
     never skipped.
   - It proves that reports cover the head. It does not prove a review
     happened: anyone who can add a commit to the pull request, from a fork
     too, can commit an approving report, and the workflow that runs the
     check is part of the pull request. The check is evidence for the owner,
     not a merge gate.
   - Each repository that uses review states this as a written control, so
     `controls.check` names it. A controls rule would fail every session
     start on an unreviewed branch.
9. **The agent command.** `review.run` starts the argv list in
   `BOARD_REVIEW_AGENT`, an environment variable the user sets and the
   manifest declares (build step 1), never a command from a board's tree,
   because mounting brings no code. No shell parses it. The agent reads an
   untrusted diff, so it starts in an empty temporary directory with the
   brief and diff on stdin, and loads no project settings, tools or MCP
   servers. The
   variable holds a JSON array of strings. The documented example is
   `["claude", "-p", "--output-format", "json", "--setting-sources", "user",
   "--tools", "", "--strict-mcp-config"]`, which loads no built-in tools and
   no MCP servers, and whose JSON gives the token counts; the implementer
   checks those flags against `claude --help`. A verdict is no stronger
   than the agent's resistance to text in the diff.
