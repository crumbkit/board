---
name: understanding-check
description: 'Check that the owner understands what a change stores, where, who shares it and what its users see, before he signs off its data or agrees its UX, and record the check as a changes/understanding/v1. Use it when a change''s plan has a Data section that is not None, when the owner is about to agree a mockup, and again before the change moves to review.'
---

# Understanding check

The owner's approval alone proves nothing. This check finds out whether he
knows what the plan does, and records what he showed.

## Steps

1. **Find the facts first.** Read the plan, the schemas and the code. Ask
   the owner only what he alone can answer.
2. **Play back, then invite correction.** In a few sentences, say what will
   be stored, where, which services read and write it, and what its users
   see. Ask him to correct it. A correction is the most useful result of the
   check; record each one.
3. **Ask in rounds of at most three questions.** Wait for his answers before
   the next round. Hold back a question whose answer depends on an open
   question. Each question is one of four kinds:
   - *predict*: "A caller hangs up while the transcript is being written.
     What does the dashboard show?" Ask first; say what the design does
     only after he answers.
   - *choose*: two concrete shapes side by side, such as one record per call
     or one per utterance. He says which one the plan uses and whether he
     wants it.
   - *explain*: "In one sentence, which service owns the transcript and
     which only reads it?"
   - *edge*: "What deletes a transcript, and what happens to a reader that
     still holds its ID?"
4. **Give no clues.** A comprehension question carries no recommended
   answer, and the options of a choice have the same length. A decision
   question, such as "should transcripts expire after 30 days?", does carry
   your recommendation.
5. **Settle each mismatch.** When his answer differs from the plan, name the
   difference plainly and ask whether the plan or his expectation should
   change. "Sounds good" is not an answer.
6. **Finish when nothing is left.** The check is done when, for every stored
   shape, store and consumer, he has explained it and said he wants it, or
   has changed it. Name anything out of scope and park it.
7. **Record it.** Write one `changes/understanding/v1` at
   `/changes/<name>/understanding/<subject>` with `board put`: the change,
   the subject (`data` or `ux`), the play-back, his corrections, each
   question with its round, kind, his answer and whether it matched, each
   mismatch and how it was settled, and what was parked. Attach it with
   `board call changes.attach --path /changes/<name> --ref <record>`.
8. **Ask for the sign-off.** The owner signs a `plans/decision/v1` whose
   `refs` name the record. Attach that decision too. The plan gate refuses
   with `changes::data::not_signed` until both are attached.
9. **Check again before review.** An answer given minutes after reading the
   plan can be short-term recall. Before the change moves to `review`, ask
   two of the predict or edge questions again, worded differently, and add
   them to `rechecks`. A wrong answer reopens the check for that part.

The owner can take the check live in chat or as a written questionnaire:
the most important questions first, one idea per question, room for an
answer under each, and a closing "anything else?". When a question does not
land, ask it again in plain words, using the project's glossary.
