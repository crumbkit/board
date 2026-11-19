---
name: plans
description: 'The plans plugin records the decisions a session makes, the questions only the owner can answer, and the owner''s answers, as records in the board. Use it when you choose between options that a later session or reviewer needs to know about, and when work waits on the owner. Write a decision with `board -C <board> put --new --var project=<project>` and a `plans/decision/v1` on stdin; `board -C <board> plans open` lists the questions still waiting for an answer. SKILL.md and README.md in the plugin''s directory say more.'
---

# plans

The plans plugin keeps decisions, questions and answers in the board, so
the next session finds them.

## Which board

Your instructions name the board. If they name none, use the board you are
in. `<project>` names what the record concerns, such as the repository
`board`. The records go in the board, never in a repository's files.

## Decisions

1. Before you decide, list the decisions already recorded, and read each
   one that bears on yours with `board -C <board> get <path>`:

       board -C <board> ls /projects/<project>/decisions

   `board::path::not_found` means the project has no decision yet.

2. When you or the owner choose between options, write a
   `plans/decision/v1`. Write it as its own record, even while you write a
   change's plan; the plan can name it.

       echo '{"$type": "plans/decision/v1", "decision": "<what was chosen>", "why": "<the reason>", "alternatives": ["<what was not chosen>"], "by": {"program": "claude-code"}, "decided": "<now>"}' | board -C <board> put --new --var project=<project>

   It prints the path it wrote, `/projects/<project>/decisions/<id>`.
   `decided` is the UTC time, as `date -u +%Y-%m-%dT%H:%M:%SZ` prints it. `by` names who decided, with any of
   `session` (a `local_` id), `program`, `person` and `agent`; the owner's
   decision names the owner as `person`. Add `refs` for the board paths the
   decision concerns, such as a plan or an answer, `urls` for web pages such
   as a pull request, and `"review": "wanted"` when the owner should look
   at it.

   A change's owner checks count a decision only when its `by.person` is
   set and its `written_by` is empty. When the owner approves in chat,
   record his approval with his name as `person` and his words, quoted, in
   `why`:

       echo '{"$type": "plans/decision/v1", "decision": "<the owner> approves the plan <plan record>.", "why": "Asked whether he approves the plan, <the owner> answered \"<his words>\".", "refs": ["<plan record>"], "by": {"person": "<the owner>", "session": "<your local_ id>"}, "decided": "<now>"}' | board -C <board> put --new --var project=<project>

   Set `person` only for a decision the owner made. A decision you make
   yourself names your `session` or `program`.
3. When the decision belongs to a change, attach it:

       board -C <board> changes attach --path /changes/<name> --ref /projects/<project>/decisions/<id>

## Questions and answers

1. When only the owner can answer and the work cannot go on without the
   answer, ask it with `plans.ask` and the options the owner may choose:

       echo '{"project": "<project>", "question": {"question": "<what the owner is asked>", "options": [{"id": "a", "text": "<option a>"}, {"id": "b", "text": "<option b>"}], "recommendation": "a", "why": "<why you recommend it>", "blocked_work": "<what waits on it, in words>", "by": {"session": "<your local_ id>"}}}' | board -C <board> call plans.ask

   When `outcome` is `filed`, it wrote `/projects/<project>/questions/<id>`.
   When it is `merged`, an open question already asks the same words and
   you now wait on that one. When it is `answered`, the owner already
   answered: act on the `answer` it returns and ask nothing.
   `by` must name your session's `local_` id, so the answer can be sent
   back to it. Never put a secret in a question. When a change or task
   waits on the answer, add `"blocks": "/changes/<name>"` or the task's
   path; `blocks` takes a path and no words.
2. List the questions still waiting for an answer, the most urgent first:

       board -C <board> plans open --path /projects/<project>

   `board -C <board> plans waiting` gives their count and one line to end a
   reply with.

3. When the owner answers in chat, record the answer with the option they
   chose and their words verbatim:

       board -C <board> plans answer --path /projects/<project>/questions/<id> --option <id> --words "<the owner's words>"

   It writes the `plans/answer/v1` at `/projects/<project>/answers/<id>`,
   with the question's id. `--option none` with `--words` records an answer
   that chooses no option.
4. Withdraw the questions whose work ended, whose asking sessions are all archived, or
   whose words a question answered in the same project already asked, with `board -C <board> plans sweep --archived <local_ id>
   --apply true`; leave out `--apply` to see the list first.
5. Find work nobody tends and ask the owner about all of it at once with
   `board -C <board> plans stale --session <local_ id> --apply true`; leave
   out `--apply` to see the list first. It covers changes in plan with no
   owner and evals with no verdict (over 7 days without activity), and
   open tasks with no owner (over 30 days). Each item becomes one
   drop-or-keep question; nothing is dropped until the owner answers.
6. Withdraw a question that no longer matters:

       board -C <board> plans withdraw --path /projects/<project>/questions/<id> --reason '<why>'

The plugin's README.md gives every field of the three record types.
