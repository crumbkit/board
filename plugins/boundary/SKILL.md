---
name: boundary
description: 'The boundary plugin checks content, such as a pull request body, before it leaves a checkout, against the controls that boundaries in the personal board name for its destination. Use it before you send anything a boundary may cover. Run `board --personal call boundary.check` with the action, destination, checkout and parts on stdin; on `boundary::check::failed`, change what it names and check again. SKILL.md and README.md in the plugin''s directory say more.'
---

# boundary

Check content before it leaves the checkout you work in.

## When

Before you send text that a boundary may cover, check it: a commit message,
a diff, a pull request title or body, a comment or an issue. An action that
sends content, such as a capability of another plugin, may check it for you.
Then follow that action's refusal the same way.

## Steps

1. Check the content. Name the action that will send it, the destination,
   the checkout's path in the personal board, and each part:

       echo '{"action": "github.pr-create", "to": "github:octo/board", "from": "/repos/board", "parts": {"pr.title": "<title>", "pr.body": "<body>"}}' | board --personal call boundary.check

   Leave out `from` for a checkout no board mounts. The output names the
   boundaries that matched, every verdict and the crossing it wrote. A
   `note` says when nothing was checked.
2. On `boundary::check::failed`, read the message or `fields.failures`.
   Each failure names a control, a part, a line and the text at fault.
   Remove or reword quoted matches, shorten parts over budget, and follow
   each `rewrite`. Then check again. `fields.recovery.text` says the same.
   On `boundary::boundary::invalid`, an entry under `/boundaries/` in the
   personal board is broken. Run `board --personal check <path>` and fix it;
   a boundary is a record the owner keeps, so ask before you change one.
3. Send only content that passed. After sending, record what the
   destination returned:

       board --personal call boundary.record --digest <digest> --result <url>

4. To ask whether content passed, such as in a hook, run
   `board --personal call boundary.verify --digest <digest>`, or pass the
   action, destination and parts on stdin.

Never edit a crossing by hand. The plugin's README.md gives the records'
fields and how boundaries match.
