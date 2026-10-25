# The effects mod

This file holds detail for docs/plugins/claude-code.md. It says what the
`board-effects` mod in `plugins/effects/claude-code/` shows the owner and
which commands it runs. docs/architecture/writes/effects.md describes the
effects themselves.

The mod lets the owner answer a plugin's pending effects without leaving
Claude Code. Board makes every decision and checks every answer. The mod only
lists effects and runs `board answer` for the owner, so it holds no rule of
its own.

- It lists the pending effects with one `board --personal query` over
  `/effects/pending/`, leaving out each effect with a seen record at the same
  id under `/effects/seen/`. It reads them when the session starts and every
  60 seconds, and again after each answer.
- When any are pending, the band above the prompt says how many, such as
  `2 effects wait for your answer`, with a Show button. When none are, or
  `board` fails, the band draws nothing of the mod's, and the failure goes to
  the debug log.
- The pane lists up to 100 effects. Each shows the plugin, the op, the path,
  the board's directory name and the first lines of the record or text a put
  would write, and says when the put replaces a file. Its Approve and Refuse
  buttons run `board answer <id> approve` or `board answer <id> refuse`. A
  refused answer is shown in a toast, and the effect stays listed.
- After a Bash call whose output holds `board::settings::asked`, the mod
  reads each effect id the output names from the personal board and asks the
  owner in a dialog with Approve, Refuse and Later. The dialog shows the
  effect as Board recorded it, so output that names an effect Board does not
  hold pending opens no dialog. Approve or Refuse runs `board answer`, and
  the agent that ran the call then reads a line it alone sees: after an
  approval, that it may run the same put again; after a refusal, that the put
  is now refused. Later, or a dismissed dialog, answers nothing.

The mod passes each id to `board` as its own argument, and an id is 64
lowercase hex digits before it reaches the query.

The mod offers no way to save a rule. `board answer --save-rule` asks for no
Touch ID today, and saving a rule must be signed with Touch ID
(plan/board-ui.md, decision 12). When Board signs saved rules, the dialog and
the pane can offer "Approve and save rule".
