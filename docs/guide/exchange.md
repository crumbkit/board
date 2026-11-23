# Exchange messages between boards

A record never holds a path into another board. When work in one board needs
something from another board, such as an answer from that board's lead, the
sender writes a message in its own words and puts it in the receiving board.
This page describes the format of those messages. No Board command sends or
reads them yet, and the Flare inbox will list these same files, so the layout
below is the contract it reads.

## A request

A request is a Markdown file at `/exchange/<to-role>/<id>.md` in the receiving
board. `<to-role>` is the role that should act on it, such as `lead`. The name
`sent` is reserved and is never a role.

The id starts with the UTC minute and the sender's role, then a slug, such as
`20261122T0329Z-lead-scope-of-the-inbox`.

The frontmatter holds:

- `kind: request`.
- `from`: the sender's `board`, `machine`, `role` and `session`.
- `to`: the receiver's `board` and `role`, and its `machine` and `session`
  when the sender knows them.
- `subject`: one line.
- `expires`: a date, such as `2026-11-29`, after which the sender stops
  waiting.
- `reply_path`, which is optional: where in the receiving board the receiver
  writes what the request asks for.

The body is Markdown in the sender's own words. Write it for a reader who
cannot open the sending board. It names no path in the sending board, quotes
no record whole, and states the facts the receiver needs. Keep it under 800
words; send a longer message as a request and follow-up replies.

```markdown
---
kind: request
from: {board: personal, machine: studio, role: lead, session: local_0a1b}
to: {board: team, role: lead}
subject: Which list does the team keep for the inbox work?
expires: 2026-11-29
---

The inbox work needs one list of open questions. Two lists exist today ...
```

## A reply

A reply is a Markdown file at `/exchange/<request id>/replies/<reply id>.md`
in the board that holds the request. Its id starts with the UTC minute and
names the replying board and role, such as `20261122T0410Z-team-lead`. Its
frontmatter holds `kind: reply`, `from`, `to`, and `outcome`, which is
`accepted`, `declined` or `done`.

A request's state comes from its replies, and nothing stores it, so neither
side edits a file the other side wrote:

| Replies | State |
| --- | --- |
| None, and `expires` has not passed | waiting |
| None, and `expires` has passed | expired |
| One or more | the `outcome` of the latest reply |

## The boundary check on both sides

Before the sender writes anything, it checks the message against the
boundaries in the owner's personal board:

```sh
board --personal boundary covers --action exchange.request --to board:team --from /repos/notes
echo '{"action": "exchange.request", "to": "board:team", "from": "/repos/notes",
       "parts": {"subject": "<subject>", "body": "<body>"}}' | board --personal boundary check
```

A reply uses the action `exchange.reply`. When no boundary covers the action,
the sender writes nothing and records a `plans/question/v1` for the owner.
When the check fails, the sender rewrites the text and checks again.

The receiver checks a request with the action `exchange.receive` before it
acts on it. When that check fails, the receiver replies `declined`, gives the
refusal code, and quotes none of the request.

A sample boundary that covers every exchange action with the team board:

```json
{
  "$type": "boundary/boundary/v1",
  "to": "board:team",
  "actions": ["exchange.*"],
  "checks": [{"rule": "/rules/boundary/no-private-names", "parts": ["*"]}]
}
```

## The sender's copies

The sender keeps a copy of each request it sends in its own repository, at
`/exchange/sent/<id>.md`. When a reply arrives, the sender checks it with the
action `exchange.receive`, then copies it to
`/exchange/sent/<id>/replies/<reply id>.md`. A reply that fails the check is
not copied; the sender records a question for the owner that gives only the
code.

The order of the writes is: check, write the request in the receiving board,
then write the local copy. When the local copy fails after the remote write,
write the copy again with the same id.

## The task that waits

The task that waits on the crossing names the copies in its `refs`, which
name files in the task's own repository only:

```sh
board tasks ref --path /tasks/agree-the-inbox-list.md --add /exchange/sent/20261122T0329Z-lead-scope-of-the-inbox.md
```

When the reply arrives, add the reply's copy the same way. `board check`
marks a task degraded with `board::ref::through_mount` when one of its `refs`
resolves through a mount, and `board tasks ref` refuses such a path with
`tasks::ref::through_mount`.

When `expires` passes with no reply, the task still names the request, and the
sender records a question for the owner.
