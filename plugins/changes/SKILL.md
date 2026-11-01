---
name: changes
description: 'Record one change to a repository, from its plan to its merge, as records written once, and derive its next step from them. Use it when you start a change to a repository, and whenever you resume one. Start in the repository''s clone, on the work branch, with `board --personal call changes.start --title ''<title>'' --session <session>`, then call `changes.next --path /changes/<name>` for the next step. A subagent whose prompt ends with `Board: /changes/<name>` first runs `board --personal call changes.brief --path /changes/<name>`, unless that brief is already in its context. SKILL.md and README.md in the plugin''s directory say more.'
---

# changes

Each step of a change is done when its record exists. `next` derives the
steps from the records; nothing stores a phase.

Run every command as `board --personal call …`. In a checkout of a
repository that is itself a board, plain `board` runs as that board.
`--session` takes the desktop app's `local_` session id when there is one,
the value of `CLAUDE_CODE_HOST_SESSION_ID`, else the transcript's id; the
runs mod matches claims by the same id.

## Steps

1. In the repository's clone, on the work branch, run
   `board --personal call changes.start --title '<title>' --session <session>`.
   Add `--handle @<you>` so the route reflects who owns what. Read every
   path in `docs`, and treat each `ask_first` pattern as a file to ask about
   before changing.
2. Run `board --personal call changes.next --path /changes/<name>` whenever
   you start a step or lose context. Do the step `next` names; `actor` says
   who takes it.
3. When you resume a change, or work on it from another clone, run
   `changes.claim --path /changes/<name> --session <session>` there first.
4. End a change with `changes.landed --commit <sha>` once it merges, or
   `changes.drop --why '<text>'` to stop it.

## Each kind of step

- **plan**: interview the owner, then write the plan with `changes.plan`,
  giving `{path, text, uses}` on stdin. `uses` names the standards, controls,
  decisions and evals it relies on.
- **answer**: ask the owner a `plans/question/v1` whose `for` names the
  change. The step is done when the owner answers `approve`. On any other
  answer, plan again and ask again, or drop the change.
- **check**: run the change's suite on a clean tree, record the run, and
  then run `changes.check --run <run>`. The run must name the commit it
  tested in `versions` under the repo record's name. Then record the files
  the branch touches at that head:

      git diff --name-only origin/main... | jq -R . | jq -sc \
        '{path: "/changes/<name>", head: "<sha>", files: .}' \
        | board --personal call changes.files

- **review**: write a `review/brief/v1` at `/reviews/<id>/brief` with `for`
  (the change), `step` (the step's name), `repo` and the range ending at the
  head, then run `review.run` from the clone. Fix each high finding, check
  again and review again; a new head needs a new review.
- **landed**: run `changes.pr`, read `/changes/<name>/pr.md`, fix it, and run
  its `command` from the clone. Merging happens outside Board; then record
  `changes.landed`.

## Subagents

End every brief to a subagent with the line `Board: /changes/<name>`. The
runs mod records the subagent as working on that change and starts it with
the change's brief.

If you are a subagent and your prompt ends with `Board: /changes/<name>`,
first run `board --personal call changes.brief --path /changes/<name>`,
unless your context already holds that brief, which starts with "The runs
mod ran".

Work across two repositories is two changes, each plan naming the other in
`uses`.

## Refusals

- `changes::branch::default` or `changes::branch::detached`: switch to a
  work branch.
- `changes::workflow::unchosen`: link `/workflows/repos/<name>` or
  `/workflows/owners/<owner>` to a workflow, or pass `--workflow`. Run
  `changes.seed` for the two seed workflows.
- `changes::suite::unknown`: pass `--suite /suites/<name>`.
- `changes::repo::name_taken`: pass `--repo-name <name>`.
- `changes::change::exists`: pass another `--name`, or resume the change.
- `changes::change::ended`: the change landed or was dropped. Start a new
  one.
- The runs mod refuses `gh pr create` when no open change names the
  repository and branch it targets. Start the change, or give `-R` and
  `--head` when the command cannot name its directory.
