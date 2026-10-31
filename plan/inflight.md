# Inflight

One read-only query answers "what is open or running right now?", and
rescuing work after a session stops means running it and reading what it
prints. The records it reads come from the work records
(plan/work-records.md) and from the `runs` mod, which records each subagent
when it starts as well as when it stops.

## Recording subagents

The runs mod writes two records per subagent, each once:

- When the Agent tool call that starts it runs, a `runs/agent/v1` at
  `/agents/<id>.json`, with `session`, `cwd`, `started` and `works_on`, and
  the prompt from that tool call as `/agents/<id>.md`.
- At SubagentStop, the `runs/run/v1` it writes today, which gains `agent`,
  the reference to that record.

The mod already pairs SubagentStart with SubagentStop by agent id. The
build confirms which id the Agent tool call shares with them; if none does,
the mod pairs the call with the next SubagentStart in the same session.
Tasks from TaskCreated are recorded as today, with no agent record.

An agent record that no run names is running. After a usage stop it is
stalled: nothing will ever write its run. Lead sessions are not recorded
this way, because a session that a usage stop ends gets no stop hook. A
change's claims say which session held it.

`works_on` comes from the prompt's last line, `Board: /changes/<name>`. When
that line is missing and the parent session holds exactly one change, the
mod uses that change. Otherwise `works_on` is left out, and the agent is
listed as unattached.

When `works_on` is set, the mod adds the output of `changes.brief` for that
change to the subagent's context, so an agent starts with the plan, the
summaries of what the plan uses and the command that checks the controls.

## The pull request gate

The mod refuses `gh pr create` when no open change names the current
repository and branch. A repository with no repo record has no change, so
it is refused too; the refusal says to run `changes.start`, which creates
both. The gate has no exemptions, as the owner asked. It reads local
records only and makes no network call.

This is the mod's first hook that blocks what it watches. Until now every
hook only recorded, and its header comment says so; this pull request
changes that comment and gives the reason.

After a pull request is created, the mod writes the change's `opened` record
with its URL, and stops writing the decision stub it writes today
(plan/decisions.md).

## The query

A new `inflight` plugin with one capability. It reads the records of the
changes, runs, plans, evals and artifacts plugins and writes nothing.
Putting it in `changes` would make that plugin read evals and artifacts,
which a change does not need.

    board call inflight.now [--holder <session>] [--stopped-at <time>] [--limit <n>]

It returns, each part bounded and saying what it left out:

- **Changes** that have no `landed` or `dropped` record. Each shows its
  repository, holder, next step and actor, its subagents, its pull request
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

    board call inflight.now --stopped-at <time>

For each change it shows, the session writes a claim, runs
`changes.brief`, and restarts the next step. A stalled agent's prompt is in
its `.md`, so the session can start a replacement with the same brief. The
session reads nothing outside the board to learn what was in flight.

## Build

Two pull requests.

1. The runs mod: agent records, `works_on`, brief injection, the pull
   request gate, the `opened` record and the end of the decision stub. Its
   tests cover each hook with a twin that is refused or left out.
2. The `inflight` plugin. Its fixture is a board holding four changes
   stopped at different steps, with running subagents, a waiting question
   and an open pull request. The test asserts that `inflight.now
   --stopped-at` lists each one with the right next step and actor.

Done when the owner, after a usage stop, rescues the work in flight with
this query and nothing else.
