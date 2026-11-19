# Claude Code mods

This file holds detail for docs/plugins.md. It says what a plugin's Claude
Code mod is and what `board-runs` writes.

A plugin may ship a mod in `plugins/<name>/claude-code/`. A mod is a Claude
Code plugin of function hooks, and ships no skill:
`.claude-plugin/plugin.json`, `hooks/hooks.json` naming one TypeScript
module, and the module itself. It
runs inside a Claude Code session and writes records only through the `board`
command, as an agent would. Board itself never runs a mod, and a mod has no
process of its own, so mods add no trigger or daemon to Board. Mods share data
only through Board records.

Board's own effects have no plugin, so their mod, `board-effects`, lives in
`plugins/effects/claude-code/`, where the `mods` workflow and the marketplace
find it. `plugins/effects/` holds nothing else and is not a Board plugin.
[effects-claude-code.md](effects-claude-code.md) says what it shows and runs.
The artifacts mod, `board-artifacts`, records each artifact a session
publishes or reads; [claude-code/artifacts.md](claude-code/artifacts.md)
says what it writes.
The attention plugin's mod, `board-attention`, puts the pinned orders in
each session and opens the signing window for an order an agent drafted;
[claude-code/attention.md](claude-code/attention.md) says what it runs.
A change to a mod raises the `version` in its `plugin.json`, because
`claude plugin update` installs only a new version; the `mods` workflow fails a
pull request that changes a mod without raising it.
Four mods share one row of badges above the prompt;
[claude-code/band.md](claude-code/band.md) says what each shows.
The github plugin's mod, `board-github`, refuses raw `git push` and `gh`
writes to a destination a boundary covers; plugins/github/README.md says
when.

Claude Code loads a mod with `claude --plugin-dir plugins/<name>/claude-code`,
or from the folders `CLAUDE_CODE_PLUGIN_DIRS` names. `claude plugin validate`
checks it and `claude plugin test` runs its `tests/*.test.ts`, which stand in
for `board` and `gh`. The `mods` workflow runs each mod's tests and
validates the marketplace below.
The release archive of a plugin does not include its mod.

The repository is also a Claude Code marketplace named `board`, listed in
`.claude-plugin/marketplace.json`. To install a mod, add the marketplace once
with `claude plugin marketplace add crumbkit/board`, then run
`claude plugin install <name>@board`, where `<name>` is the mod's name in its
`plugin.json`, such as `board-runs` or `board-artifacts`. The `mods` workflow
fails when a mod has no entry in the marketplace.

## board-changes

`board-changes`, in `plugins/changes/claude-code/`, is the write guard: it
refuses an Edit, Write or NotebookEdit of a mounted repository unless the
acting agent started from a change's brief. The changes plugin's README,
section "The write guard", says what it checks and what it cannot see.

## board-handoff

`board-handoff`, in `plugins/handoff/claude-code/`, tells a session to hand
off before it runs out of context or account usage. It is a mod and no Board
plugin, so it adds no capability.

After each turn Claude Code gives a mod the session's context share and the
account's rate-limit windows (`session.measure`). When the context share
reaches `handoff-context-share`, or a window reaches `handoff-usage-share`,
the mod submits one prompt that tells the session to follow the handoff skill
now. Both values are read once per session with `board --personal call
params.get`; without a value the mod uses 75 and 85. For a usage window the mod
also writes `trigger.json` in `board-handoff/` under the account's Claude
config directory. Every session on that account checks the file every 30
seconds and gets the same prompt, once, until the window resets.

When the told turn ends, the session shows `handoff: cd <dir> && claude
--resume <id>  # <its handoff line>` and keeps it in `sessions/<id>.json`.
`/handoff-lines` prints every session's line. The mod does not end a session
and cannot make the model obey the prompt.

## written_by

A record a hook writes without an agent carries `written_by`, the plugin and
hook that wrote it, such as `runs/subagent-stop`. It keeps the difference
between what an agent wrote and what was recorded for it. `runs/run/v1`,
`evals/eval/v1`, `plans/decision/v1` and `plans/answer/v1` take the field, and
a board takes it once `board plugin enable` has installed the plugin's schemas
again. A board whose schema lacks the field refuses the record with
`board::record::invalid`.

## board-runs

The runs plugin's mod, `plugins/runs/claude-code/`, records what a session
delegates and the pull requests it opens. It opens no dialog. Each run and
decision stub it writes adds a dim row to the transcript and counts on the status line, such as
`board: 2 records this session`.

Three options, set in the `/config` menu or in settings under
`pluginConfigs`, say where it writes:

- `board`: the board's directory. When empty, `board` runs in the session's
  working directory and finds the board there.
- `suite`: the suite of the runs, `/suites/claude-code` by default. The mod
  writes a `runs/suite/v1` record there when none exists.
- `decisions_template`: the directory of the decision stubs, where `{repo}`
  stands for the name of the pull request's repository, lowercased.
  `/projects/{repo}/decisions` by default.

When a subagent stops, or a task is completed, the mod writes a
`runs/run/v1` at `<suite>/runs/<started>-<id>`, where `<id>` is the agent or
task id. Its `by` is `{"program": "claude-code"}` with the subagent type or
teammate as `agent`, and the session's `local_` id in the desktop app.
`measures.duration_s` is the seconds since it started, and `summary` the subagent's last line of text or
the task's subject. A stop that another hook blocks writes nothing, because
the subagent goes on. When the mod did not see the unit start, as when it
loaded after the unit started or the session was resumed, the run has no
duration.

After a Bash call that runs `gh pr create` succeeds, the mod reads the last
pull request URL in its output and asks `gh pr view` for the title. It writes
a `plans/decision/v1` stub at `<directory>/<time>-<repo>-<number>`. The
directory is `decisions_template` with the repository's name in place of
`{repo}`. The stub holds the title as `decision`, the URL in `urls`,
`review: "wanted"`, the session id, and an empty `why`. It then appends a row
the person does not see to the conversation of the agent that ran `gh`,
asking it to fill in `why` and `alternatives`.

When the output holds no URL, as for `gh pr create --help`, the mod writes
nothing and says so in the debug log. When `gh` gives no title, it writes
nothing and logs why. When `board` refuses a record, the transcript row gives its code
and message. Two refusals go only to the debug log, because they mean the
board does not take the record at all: `board::config::not_found`, when the
directory holds no board, and `board::record::unknown_type`, when the board
has no plans plugin.
