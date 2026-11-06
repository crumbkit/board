---
name: plans
description: 'The plans plugin records the decisions a session makes, the questions only the owner can answer, and the owner''s answers, as records in the board. Use it when you choose between options that a later session or reviewer needs to know about, and when work waits on the owner. Write a decision with `board -C <board> put --new --var project=<project>` and a `plans/decision/v1` on stdin; `board -C <board> call plans.open` lists the questions still waiting for an answer. SKILL.md and README.md in the plugin''s directory say more.'
---

# plans

Record decisions, questions and answers in the board, so the next session
finds them.

## Which board

Your instructions name the board. If they name none, use the board you are
in. `<project>` names what the record concerns, such as the repository
`board`. The records go in the board, never in a repository's files.

## Decisions

1. Before you decide, read the decisions already recorded:

       board -C <board> ls /projects/<project>/decisions

2. When you or the owner choose between options, write a
   `plans/decision/v1`. Write it as its own record, even while you write a
   change's plan; the plan can name it.

       echo '{"$type": "plans/decision/v1", "decision": "<what was chosen>", "why": "<the reason>", "alternatives": ["<what was not chosen>"], "by": {"program": "claude-code"}, "decided": "2026-11-06T12:00:00Z"}' | board -C <board> put --new --var project=<project>

   It prints the path it wrote, `/projects/<project>/decisions/<id>`.
   `decided` is the UTC time. `by` names who decided, with any of
   `session` (a `local_` id), `program`, `person` and `agent`; the owner's
   decision names the owner as `person`. Add `refs` for the board paths the
   decision concerns, such as a plan or an answer, `urls` for web pages such
   as a pull request, and `"review": "wanted"` when the owner should look
   at it.
3. When the decision belongs to a change, attach it:

       board -C <board> call changes.attach --path /changes/<name> --ref /projects/<project>/decisions/<id>

## Questions and answers

1. When only the owner can answer and the work cannot go on without the
   answer, write a `plans/question/v1` with the options the owner may
   choose:

       echo '{"$type": "plans/question/v1", "question": "<what the owner is asked>", "options": [{"id": "a", "text": "<option a>"}, {"id": "b", "text": "<option b>"}], "recommendation": "a", "why": "<why you recommend it>", "blocks": "<what waits on it>", "by": {"session": "<your local_ id>"}, "asked": "2026-11-06T12:00:00Z", "status": "open"}' | board -C <board> put --new --var project=<project>

   It writes `/projects/<project>/questions/<id>`. `by` must name your
   session's `local_` id. Never put a secret in a question.
2. List the questions still waiting for an answer:

       board -C <board> call plans.open --path /projects/<project>

3. When the owner answers in chat, record the answer with the option they
   chose and their words verbatim:

       board -C <board> call plans.answer --path /projects/<project>/questions/<id> --option <id> --words "<the owner's words>"

   It writes the `plans/answer/v1` at `/projects/<project>/answers/<id>`,
   with the question's id. `--option none` with `--words` records an answer
   that chooses no option.
4. Withdraw a question that no longer matters:

       board -C <board> call plans.withdraw --path /projects/<project>/questions/<id> --reason '<why>'

The plugin's README.md gives every field of the three record types.
