# Inflight

One read-only query answers "what is open or running right now?", and
rescuing work after a session stops means running it and reading what it
prints. The records it reads come from the work records
(plan/work-records.md) and from the `runs` mod, which records each subagent
when it starts as well as when it stops.

## Recording subagents

The runs mod writes two records per subagent, each once. It runs Board as
`board --personal`, as every call these plans add does (plan/work-records.md), so
its records, its gate and its brief come from the personal board whatever
the session's directory.

- When the Agent tool call that starts it runs, a `runs/agent/v1` at
  `/agents/<id>.json`, with `session`, `cwd`, `started` and `works_on`, and
  the prompt from that tool call as `/agents/<id>.md`.
- At SubagentStop, the `runs/run/v1` it writes today, which gains `agent`,
  the reference to that record.

The mod already pairs SubagentStart with SubagentStop by agent id. The
build confirms which id the Agent tool call shares with them. If none does,
the mod pairs a SubagentStart with an Agent call only when exactly one call
in the session is still unpaired. When one message starts several agents,
it pairs none of them: their runs are recorded as today, without `agent`,
and the mod injects no brief.
Tasks from TaskCreated are recorded as today, with no agent record.

An agent record that no run names is running. After a usage stop it is
stalled: nothing will ever write its run. Lead sessions are not recorded
this way, because a session that a usage stop ends gets no stop hook. A
change's newest claim says which session held it and the worktree it worked
in (plan/work-records.md).

`works_on` comes from the prompt's last line, `Board: /changes/<name>`. When
that line is missing and the parent session holds exactly one change, the
mod uses that change. Otherwise `works_on` is left out, and the agent is
listed as unattached.

When `works_on` is set and the pairing is certain, the mod adds the output
of `changes.brief` for that change to the subagent's context, so an agent
starts with the plan, the summaries of what the plan uses and the command
that checks the controls. The changes skill also tells every subagent whose
prompt ends with a `Board:` line to run `changes.brief` for it first when
no brief is in its context, so an agent whose start could not be paired
still gets one.

## The pull request gate

The mod refuses `gh pr create` when no open change names the repository
and branch the command targets. It takes them from the command's `-R` and
`--head` when given, and otherwise from its directory: the session's
directory, or the one a leading `cd <dir> &&` names. When it cannot tell,
for example after another `cd`, it refuses and says to run `gh pr create`
with `-R` and `--head`. A repository with no repo record has no change, so
it is refused too; the refusal says to run `board --personal call
changes.start`, which creates both. The gate has no exemptions, as the owner asked. It reads local
records only and makes no network call.

This is the mod's first hook that blocks what it watches. Until now every
hook only recorded, and its header comment says so; the runs mod pull
request changes that comment and gives the reason.

After a pull request is created, the mod writes the change's `opened` record
with its URL, and stops writing the decision stub it writes today
(plugins/decisions/README.md).

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

Two pull requests.

1. The runs mod: agent records, `works_on`, brief injection, the pull
   request gate, the `opened` record, the end of the decision stub with the
   second migration run (plugins/decisions/README.md), and the artifacts mod's `for`
   (plan/artifacts.md). Its
   tests cover each hook with a twin that is refused or left out.
2. The `inflight` plugin. Its fixture is a board holding four changes
   stopped at different steps, with running subagents, a waiting question
   and an open pull request. The test asserts that `inflight.now
   --stopped-at` lists each one with the right next step and actor.

Done when the owner, after a usage stop, rescues the work in flight with
this query and nothing else.
