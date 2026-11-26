---
name: plans
description: 'The plans plugin records the decisions a session makes, the questions only the owner can answer, and the owner''s answers, as records in the board. Use it when you choose between options that a later session or reviewer needs to know about, and when work waits on the owner. Record a decision with `board -C <board> plans decide --project <project> --decision ''<what was chosen>'' --why ''<the reason>'' --session <local_ id>`; `board -C <board> plans open` lists the questions still waiting for an answer. SKILL.md and README.md in the plugin''s directory say more.'
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

   `board::path::not_found` means the project has no decision yet. To find
   decisions on the subject in every project, run
   `board -C <board> search 'type:decision <words>'`; `type:question` and
   `type:answer` find questions and answers the same way, and
   `from:<person>` keeps the records that person wrote.

2. When you or the owner choose between options, record a decision with
   `plans.decide`. Record it as its own record, even while you write a
   change's plan; the plan can name it.

       board -C <board> plans decide --project <project> --decision '<what was chosen>' --why '<the reason>' --alternatives '<what was not chosen>' --session <your local_ id>

   It prints the path it wrote, `/projects/<project>/decisions/<id>`, and
   fills in `$type`, `decided` as now, and `by` from `--session`,
   `--person` and `--program`. A call that gives neither `--session` nor
   `--person` is refused with `plans::decide::no_author`. Repeat
   `--alternatives` for each option not chosen, and `--refs` for each
   board path the decision concerns, such as a plan or an answer, and
   `--urls` for each web page, such as a pull request. Add `--review` when
   the owner should look at it. JSON on stdin with the same names, such as
   `{"project": "<project>", "decision": "...", "alternatives": ["..."]}`,
   works too. When the project held nothing before, the decision is still
   written, and the output warns with the three nearest project names; on
   a typo, record it again under the right name and remove the first with
   `board rm`.

   A change's owner checks count a decision only when its `by.person` is
   set and its `written_by` is empty. They count an answer when its
   `by.person` or its `written_by` is set. When the owner approves in chat,
   record his approval with his name as `--person` and his words, quoted, in
   `--why`:

       board -C <board> plans decide --project <project> --decision '<the owner> approves the plan <plan record>.' --why 'Asked whether he approves the plan, <the owner> answered "<his words>".' --refs '<plan record>' --person '<the owner>' --session <your local_ id>

   Set `--person` only for a decision the owner made. A decision you make
   yourself names your `--session` or `--program`.
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

## A project's plan

A change's plan is a `changes/plan/v1` that `changes.plan` writes. A
project with no mounted repository has no change, so it has no such record.

- A project with no mounted repository keeps its plan in the Markdown body
  of its project task, a `tasks/task/v2` with `kind: project`.
- The body holds a `## Steps` list only. It lists no decision or question:
  each decision names the project task in `refs` and each question names
  it in `blocks`, so `board refs <project task>` lists them.
- Start the project with its first plan as the body:

      board -C <board> tasks add --title '<project title>' --kind project --text "$(cat plan.md)"

  It prints the project task's path.
- A decision names the project task in `refs`.
- A question names the project task in `blocks`.
- `board -C <board> refs <project task>` lists the decisions and questions
  that name it.
- To revise the plan, read the task into a file, edit the file and write it
  back:

      board -C <board> get <project task> > plan.md
      board -C <board> put <project task> --expect <blob> < plan.md

- plan.md holds the task's frontmatter and its body.
- Edit only the body, and leave the frontmatter as it is.
- This command prints the `blob`:

      board -C <board> --json get <project task> | jq -r '.data[0].blob'

- On `board::put::stale`, read the task again and redo the edit.
- Git keeps every earlier plan.
- A project task in a mounted notes repository refuses this `put` with
  `board::put::through_mount`. Edit it as the tasks skill's Edit section
  says.

## A question for another board

A record never names a path in another board. When the answer must come
from another board, such as a team board's lead:

1. Write a request in your own words, as docs/guide/exchange.md describes:
   one subject line, a body under 800 words, and no path from this board.
2. Check it with `board --personal boundary check`, action
   `exchange.request`. When no boundary covers it, write nothing and ask
   the owner a question here instead.
3. Write the request at `/exchange/<role>/<id>.md` in the other board, then
   your copy at `/exchange/sent/<id>.md` in the task's own repository.
4. Name the copy on the waiting task:

       board -C <board> tasks ref --path <task> --add /exchange/sent/<id>.md

   A project task is v2 and takes no refs until projects move to their own
   records, so `tasks ref` on it is refused with `tasks::ref::not_v3`. Add a
   v3 child task that names the copy instead, with JSON on stdin:

       echo '{"title": "<what waits>", "parent": "<project task>",
         "refs": ["/exchange/sent/<id>.md"]}' | board -C <board> tasks add

5. When a reply arrives, check it with action `exchange.receive`, copy it
   under `/exchange/sent/<id>/replies/`, and add that copy the same way.

The plugin's README.md gives every field of the three record types.
