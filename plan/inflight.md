# Inflight

One read-only query answers "what is open or running right now?", and
rescuing work after a session stops means running it and reading what it
prints. The records it reads come from the work records
(plan/work-records.md) and from the `runs` mod, which records each agent
when it starts as well as when it stops.

## Recording agents

The runs mod writes two records per agent, each once:

- At start, a `runs/agent/v1` at `/agents/<id>.json`, with `kind` (`session`
  or `subagent`), `session`, `parent`, `cwd`, `started` and `works_on`, and
  the prompt as `/agents/<id>.md`.
- At stop, the `runs/run/v1` it writes today, which gains `agent`, the
  reference to that record.

An agent record that no run names is running. After a usage stop it is
stalled: nothing will ever write its run.

`works_on` comes from the prompt's last line, `Board: /changes/<name>`. When
that line is missing and the parent session holds exactly one claim, the mod
uses that change. Otherwise `works_on` is left out, and the agent is listed
as unattached.

When `works_on` is set, the mod adds the output of `changes.brief` for that
change to the subagent's context, so an agent starts with the plan, the
summaries of what the plan uses and the command that checks the controls.

## The pull request gate

The mod refuses `gh pr create` when no open change names the current
repository and branch. The refusal says to run `changes.start`. It reads
local records only and makes no network call. After a pull request is
created, the mod writes the change's `opened` record with its URL. It no
longer writes a decision stub (plan/decisions.md).

## The query

A new `inflight` plugin with one capability:

    board call inflight.now [--owner <session>] [--stopped-at <time>] [--limit <n>]

It returns, each part bounded and saying what it left out:

- **Changes** that have no `landed` or `dropped` record. Each shows its
  repository, owner, next step and actor, the agents working on it, and its
  pull request URL when one is open.
- **Agents** that are running, or stalled when `--stopped-at` is given and
  they started before that time, grouped under their change. Unattached
  agents come last.
- **Questions** waiting on the owner, from `plans.open`.
- **Decisions** awaiting review, from `decisions.open`.
- **Evals** planned or running, from `evals.open`.
- **Artifacts** with a version in the last seven days (plan/artifacts.md).

`--owner` keeps only the changes that session owns and their agents.
`--json` prints all of it as one document. The query makes no network call,
so it cannot say that a pull request merged. A change with an `opened` and
no `landed` record shows the command that checks it,
`gh pr view <url> --json state`.

## Rescue

After a usage stop, a new session runs:

    board call inflight.now --stopped-at <time>

For each change it shows, the session writes a claim, runs
`changes.brief`, and restarts the next step. A stalled agent's prompt is in
its `.md`, so the session can start a replacement with the same brief. The
session reads nothing outside the board to learn what was in flight.

## Build

Two pull requests.

1. The runs mod: agent records, `works_on`, brief injection, the pull
   request gate and the `opened` record. Its tests cover each hook with a
   twin that is refused or left out.
2. The `inflight` plugin. Its fixture is a board holding four changes
   stopped at different steps, with running agents, a waiting question and
   an open pull request. The test asserts that `inflight.now --stopped-at`
   lists each one with the right next step and actor.

Done when the owner, after a usage stop, rescues the work in flight with
this query and nothing else.
