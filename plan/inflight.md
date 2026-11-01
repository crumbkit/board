# Inflight

One read-only query answers "what is open or running right now?", and
rescuing work after a session stops means running it and reading what it
prints. The records it reads come from the work records
(plugins/changes/README.md) and from the `runs` mod, which records each
subagent when it starts as well as when it stops.

## Built: the runs mod

The first pull request built the records the query reads.
docs/plugins/claude-code/runs.md describes them, and
plugins/runs/README.md the `runs/agent/v1` record.

- Each subagent gets an agent record at `/agents/<id>.json`, with
  `session`, `cwd`, `started` and `works_on`, and its prompt as
  `/agents/<id>.md`, when the `agent.spawn` hook runs. The engine's agent
  id is the id SubagentStart and SubagentStop carry, so every record pairs
  with its run, and the run names it in `agent`. Several agents started by
  one message pair as surely as one.
- An agent record that no run names is running. After a usage stop it is
  stalled: nothing will ever write its run. Lead sessions are not recorded
  this way; a change's newest claim says which session held it and where.
- `works_on` comes from the prompt's `Board: /changes/<name>` line, else
  the session's one held change. Without it the agent is unattached.
- The gate refuses `gh pr create` for a repository and branch no open
  change names, and the mod writes the `opened` record. The decision stub,
  its migration script and `plans/decision/v1` are gone.
- `changes.open` gives each change's `holder` and `worktree` and takes
  `--holder`, `--remote` and `--branch`. `artifacts/version/v1` has `for`,
  and `artifacts.list` takes `--for`.

## The query

A new `inflight` plugin with one capability. It reads the records of the
changes, runs, plans, evals and artifacts plugins and writes nothing.
Putting it in `changes` would make that plugin read evals and artifacts,
which a change does not need.

    board --personal call inflight.now [--holder <session>] [--stopped-at <time>] [--limit <n>]

It returns, each part bounded and saying what it left out:

- **Changes** from `changes.open`, each with its step derivation from
  `changes.next` through `board call`, so the rules live in one plugin. Each
  shows its
  repository, holder, worktree, next step and actor, its subagents, its pull request
  URL when one is open, and the artifacts whose `for` names it.
- **Agents** that are running, or stalled when `--stopped-at` is given and
  they started before that time, grouped under their change. Unattached
  agents come last.
- **Questions** waiting on the owner, from `plans.open`. A decision awaiting
  review appears here once, through its question.
- **Evals** planned or running, from `evals.open`.

`--holder` keeps only the changes that session holds and their agents. The
query makes no network call, so it cannot say that a pull request merged. A
change with an `opened` and no `landed` record shows the command that
checks it, `gh pr view <url> --json state`.

## Rescue

After a usage stop, a new session runs:

    board --personal call inflight.now --stopped-at <time>

For each change it shows, the session goes to the change's worktree,
writes a claim from there, runs `changes.brief`, and restarts the next
step. Uncommitted work in that worktree is still there, since a usage stop
ends the session and leaves its files. A stalled agent's prompt is in
its `.md`, so the session can start a replacement with the same brief. The
session reads nothing outside the board to learn what was in flight.

## Build

The second pull request builds the `inflight` plugin. Its fixture is a
board holding four changes stopped at different steps, with running
subagents, a waiting question and an open pull request. The test asserts
that `inflight.now --stopped-at` lists each one with the right next step
and actor.

Done when the owner, after a usage stop, rescues the work in flight with
this query and nothing else.
