# board-runs

This file holds detail for docs/plugins/claude-code.md. It says what the
runs plugin's mod, `plugins/runs/claude-code/`, writes and refuses.
plugins/runs/README.md describes the records.

The runs plugin's mod, `plugins/runs/claude-code/`, records each subagent a
session starts and each unit it delegates, gives a subagent working on a
change its brief, and gates `gh pr create`. Every call runs as
`board --personal`, so the records, the changes and the brief come from the
personal board whatever the session's directory. It opens no dialog. Each
record it writes adds a dim row to the transcript and counts on the status
line, such as `board: 2 records this session`.

Its one option, `suite`, set in the `/config` menu or in settings under
`pluginConfigs`, is the suite of the runs, `/suites/claude-code` by default.
The mod writes a `runs/suite/v1` record there when none exists.

## Subagents

When the Agent tool starts a subagent, the `agent.spawn` hook writes, in one
`board put --batch`, a `runs/agent/v1` at `/agents/<id>` and the prompt the
tool call gave as `/agents/<id>.md`. `<id>` is the agent id the engine
returns, which SubagentStart and SubagentStop carry as `agent_id`, so the
record and the run pair by that id. The record holds `session`, the
session's id; `cwd`, the subagent's directory; `started`; and `works_on`.
A resumed run writes nothing, since its record was written when it first
started, and a refused spawn writes nothing.

`works_on` is the change the prompt's last line names as
`Board: /changes/<name>`. Without that line it is the one open change whose
newest claim names this session, from
`changes.open --holder <session>`; with none or several it is left out.
When `works_on` is set, the mod runs `changes.brief` for it and starts the
subagent with that output, as JSON, before its prompt. The record keeps the
prompt as the tool call gave it.

When a subagent stops, or a task is completed, the mod writes a
`runs/run/v1` at `<suite>/runs/<started>-<id>`, where `<id>` is the agent or
task id. Its `by` is the agent type or the teammate, `measures.duration_s` the
seconds since it started, `summary` the subagent's last line of text or the
task's subject, and `agent` the subagent's agent record when the mod wrote
one. A stop that another hook blocks writes nothing, because the subagent
goes on. When the mod did not see the unit start, as when it loaded after the
unit started or the session was resumed, the run has no duration and no
`agent`. An agent record that no run names is running, or stalled after a
usage stop.

## The pull request gate

Until this gate every hook only recorded. It is the one hook that blocks
what it watches: before a Bash call that runs `gh pr create`, it finds the
repository and branch the command targets and refuses the call when no
open change names them, because such a pull request would leave the owner
no record of its plan, checks and review. It reads local records only and
makes no network call. It has no exemptions.

- The repository is the command's `-R` or `--repo`, else the origin of its
  directory. `HOST/OWNER/REPO` is read as `https://HOST/OWNER/REPO`.
- The branch is its `-H` or `--head`, without an `owner:` prefix, else the
  branch checked out in its directory.
- The directory is the one a leading `cd <dir> &&` names, else the
  session's. Any other `cd`, `pushd` or `popd` before `gh` makes it unknown,
  and the call is refused, saying to give `-R` and `--head`. So is a
  directory with no origin or no branch checked out.

The mod then runs `changes.open --remote <repository> --branch <branch>`.
With no change, the refusal says to run `board --personal call
changes.start` from the clone on that branch, which writes a repo record
too when the board has none. A failed `changes.open` is refused with its
code. After the pull request is created, the mod writes the change's
`opened` record, `changes/opened/v1` with the URL the output printed last.
When two open changes name the branch, it writes none and says so.

When `board` refuses a record, the transcript row gives its code and
message. Two refusals go only to the debug log, because they mean the board
does not take the record at all: `board::config::not_found`, when there is
no personal board, and `board::record::unknown_type`, when it lacks the
plugin.
