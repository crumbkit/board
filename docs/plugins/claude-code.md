# Claude Code mods

This file holds detail for docs/plugins.md. It says what a plugin's Claude
Code mod is and what `board-runs` writes.

A plugin may ship a mod in `plugins/<name>/claude-code/`. A mod is a Claude
Code plugin of function hooks: `.claude-plugin/plugin.json`,
`hooks/hooks.json` naming one TypeScript module, and the module itself. It
runs inside a Claude Code session and writes records only through the `board`
command, as an agent would. Board itself never runs a mod, and a mod has no
process of its own, so mods add no trigger or daemon to Board. Mods share data
only through Board records.

Claude Code loads a mod with `claude --plugin-dir plugins/<name>/claude-code`,
or from the folders `CLAUDE_CODE_PLUGIN_DIRS` names. `claude plugin validate`
checks it and `claude plugin test` runs its `tests/*.test.ts`, which stand in
for `board` and `gh`. The `mods` workflow runs each mod's tests and
validates the marketplace below.
The release archive of a plugin does not include its mod.

The repository is also a Claude Code marketplace named `board`, listed in
`.claude-plugin/marketplace.json`. To install a mod, add the marketplace once
with `claude plugin marketplace add crumbkit/viitta`, then run
`claude plugin install <name>@board`, where `<name>` is the mod's name in its
`plugin.json`, such as `board-runs`, `board-controls` or `board-plans`. The `mods` workflow
fails when a mod has no entry in the marketplace.

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
task id. Its `by` is the agent type or the teammate, `measures.duration_s` the
seconds since it started, and `summary` the subagent's last line of text or
the task's subject. A stop that another hook blocks writes nothing, because
the subagent goes on. When the mod did not see the unit start, as when it
loaded after the unit started or the session was resumed, the run has no
duration.

After a Bash call that runs `gh pr create` succeeds, the mod reads the last
pull request URL in its output and asks `gh pr view` for the title. It writes
a `plans/decision/v1` stub at `<directory>/<time>-<repo>-<number>`. The
directory is `decisions_template` with the repository's name in place of
`{repo}`. The stub holds the title as `decision`, the URL in `refs`,
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
