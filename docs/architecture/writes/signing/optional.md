# Optional signing

This file holds detail for docs/architecture/writes/signing.md.

`signing` in the personal board's `.board/board.json` says which acts need
a signature:

```json
"signing": {"require": ["order", "effect", "owner-write"]}
```

- The acts are `order`, `effect` (approving a plugin's pending effect),
  `owner-write` (a put under `effects/` or removing a board's config),
  `plan`, `data` and `mockup` (a change's sign-offs). With `require` left
  out, the list is `order`, `effect` and `owner-write`.
- Signing is on when Board's allowed signers file, or the attention one,
  has a line whose principals name `owner`. Board reads the files on each
  call (`board_plugin::owner_listed`); a file it does not trust reads as
  on. A path whose presence Board cannot tell, such as a link to nothing or
  a directory it cannot search, refuses each listed act with
  `board::signing::file_missing`.
- With signing on, a listed act needs Touch ID and an unlisted act takes an
  answer. With signing off, every act takes an answer.
- Signing is configured once the personal board holds a signed record: a
  seen record under `effects/seen/` or an order under `orders/` with a
  `signature`. Board reads that only when no `owner` line is found, and
  writes nothing. `signing.configured` overrides it: true or false, which
  only a signed edit sets. While configured, a missing allowed signers file
  refuses each listed act with `board::signing::file_missing` and never
  falls back to an answer.
- An edit of `signing` needs a signature whenever signing is on, whatever
  `require` lists. A plugin never writes `.board/`.
- `board orders pinned` and `board orders signers` give the state in their
  `signing` line, such as `Signing is off: every act takes an answer in
  chat.` No band shows it.

## Effects and owner's writes

- `board answer <id> approve --approved <answer>` names the owner's
  `plans/answer/v1` in the personal board. Board checks that it approves the
  effect and writes the seen record with `approved` and no signature. A
  decide reads that seen record as an approval only while signing takes an
  answer for effects.
- With signing off, an approval without `--approved` or `--signature` is
  refused with `board::signing::not_approved`. With signing on and effects
  listed, `--approved` is refused the same way.
- Once the personal board holds a signed record, a missing allowed signers
  file refuses an approval with `board::signing::file_missing`.
- A put of a board's config that changes `signing` is an edit of it.
- With signing off, no owner's write asks for a signature.

## Approving with an answer

An answer approves an act when it is a `plans/answer/v1` that names
`by.person`, chooses the option `approve` of the `plans/question/v1` it
answers, and was recorded after that question was asked, and the
question's text names what is approved: the pending effect's id, or the
order's `Order::digest`, `sha256:` and the hex SHA-256 of its statement, so
an edit of the order voids the answer (`board_model::approving`).

- A pending effect approved so has a seen record with `approved`, the
  answer's path, and no signature.
- An order approved so holds `approved` and no signature, and verifies as
  `approved in chat, not signed` where a signed one says `signed by owner`.
  `board orders sign --path <order> --approved <answer>` writes it.
- `plans.answer --session` records `heard`: whether the answer's words are
  a prompt the owner typed in that session, or a line of one. The
  board-plans mod logs the SHA-256 of each prompt the owner types at the
  terminal or through Remote Control, and of each of its lines, in
  `~/.local/state/board/prompts/<session>`, kept 30 days and never synced.
  Words of one word are never heard, nor a digest dated after the check.
  Board records `heard` and does not yet refuse an answer by it.
- A check reads `heard` from the prompt log of the answer's `by.session`
  when it runs (`Approving::heard`), never from the answer's `heard`
  field, which whoever wrote the answer chose.

## Limits

- An agent with a shell can write an answer with `--person`, append a
  digest to the prompt log, or pass `--written_by plans/pane-answer`, so an
  answer is a guardrail. With signing on, the listed acts stay behind Touch
  ID, which no agent can supply.
- Codex and other harnesses have no prompt hook, so their answers are never
  heard; the owner answers through the questions pane, Board.app or /ask in
  Claude Code.
- A prompt the owner pasted from an agent counts as typed.
- Board reads `signing` from the working tree's `.board/board.json`, so an
  agent with a shell that writes the file directly changes `require` or
  `configured` without Touch ID. The guard covers `board put` only.
