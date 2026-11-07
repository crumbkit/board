# attention

A Board plugin for orders: instructions the owner signs with Touch ID, which
agents verify instead of going back to the owner's chat to check. It is
built into Board as `crates/board-plugin-attention`. Orders live in the
owner's personal board, the `personal` entry in
`~/.config/board/config.json`, and every command here names it with
`--personal`.

## How an order works

1. An agent writes a draft order, an `attention/order/v1` record at
   `/orders/<name>`, and a Markdown context file at `/orders/<name>.md`. The
   order is the binding instruction. The context is the non-binding
   background from the conversation, so a reader can tell what the owner
   meant.
2. The owner runs `board --personal call attention.sign --path
   /orders/<name>`, or presses **Review and sign** in the Claude Code mod.
   Board shows the order through its signer, which opens a window showing
   the order, its expiry, whether it is pinned, and the context as plain
   text. If the owner clicks **Sign with Touch ID**, Touch ID signs the
   statement, and `sign` writes the signature into the order with one put
   that expects the blob it read. An order edited while the owner reads it
   is refused with Board's `board::put::stale` and keeps its new text
   unsigned. If the owner clicks **Request changes…** instead, `sign`
   refuses with `attention::sign::declined`, and its `feedback` field holds
   what the owner wrote.
3. Any agent given the path runs
   `board --personal call attention.verify --path /orders/<name>`. It gets
   the order and its context back, or a code saying why it cannot be
   trusted.

## The record

- `order`: the binding instruction, at least one character. No line of it
  may start with `Context (non-binding, from `, because the statement uses
  that line to start the context.
- `context`: the path of the context file, without a line break. The file
  holds at most 64 KiB of UTF-8, which the owner reads whole before signing.
- `expires`: the last day the order binds, as `YYYY-MM-DD`. It must be a
  real day. The order binds through that day in UTC.
- `pinned` (optional): whether every agent session starts with the order.
- `signature` (optional): the owner's SSH signature of the statement. A
  draft has none. Only `sign` writes it.

## The statement

The owner signs a statement built from the order, its expiry, whether it is
pinned, and the context's text, in the `attention/order/v1` namespace.
Board's signing owner builds it and checks it, and the plugin never builds
it itself. docs/architecture/writes/signing.md gives its layout, and the
vectors in `vectors/signing/` fix its bytes. Every order the Python plugin
signed still verifies, because the statement's bytes, the namespace and the
keys are unchanged.

## Pinning

An order with `"pinned": true` is shown to every agent session as it starts.
`pinned` is part of what the owner signs, so an agent that pins or unpins an
order by editing it breaks the signature. `board --personal call
attention.pinned` lists each pinned order in the board's own tree that
verifies, by path, up to 32 KiB of orders and context, and lists the rest
with the code that says why each was left out. It does not read orders in
boards the board mounts.

## Setup

Install the signer from stemmacorp/board-app as Board's signing docs say
(docs/architecture/writes/signing.md), then put the owner's key in
`~/.config/attention/allowed_signers` as the principal `owner`. The file must
belong to the owner, and no one else may write it. Then enable the plugin in
the personal board:

```sh
board --personal plugin enable attention
```

`board skills install` installs `SKILL.md`, which tells an agent when and
how to read, check and draft orders.

### Moving from the Python plugin

A Board that has attention built in refuses `board call attention.*` and
`board plugin enable attention` with `board::plugin::duplicate` while the old
plugin is still installed, and the message names the link to remove.

1. Remove the link `~/.local/share/board/plugins/attention`.
2. Remove the link `~/.claude/skills/attention`, then run
   `board skills install`. Until the link is gone, install refuses with
   `board::skills::not_board`.
3. In Claude Code, run `claude plugin uninstall attention-approve@attention`
   and `claude plugin marketplace remove attention`, then install
   `board-attention@board` as below.
4. Delete `~/.config/attention/board`, which nothing reads now.
5. Run `board --personal plugin enable attention`. It writes nothing,
   because the schema's bytes are unchanged.

## The Claude Code mod

`claude-code/` is a Claude Code mod, `board-attention`.
docs/plugins/claude-code/attention.md says what it shows and runs. Install it
at a Claude Code terminal prompt:

```
claude plugin marketplace add crumbkit/board
claude plugin install board-attention@board
```

## Signing

- Board's signing owner builds the statement, runs the signer and checks the
  signature (STANDARDS.md 29). The plugin asks for both through
  `Host::sign_order` and `Host::verify_order`, and names no signing literal.
- An agent never signs an order, edits a `signature`, or runs
  `attention.sign` without the owner at the Mac.

## What it does not protect against

Any program running as the owner can edit `allowed_signers` and add its own
key, or replace the signer with one that shows one thing and signs another.
Install the signer from a reviewed commit, and check that the key in
`allowed_signers` is the one the signer prints. The Secure Enclave
guarantees only that the owner's key signs nothing without Touch ID.

## Codes

`verify` and `sign` refuse an order with:

- `attention::order::unsigned`: the order has no signature yet (`verify`).
- `attention::order::bad_signature`: the order, its expiry or its context
  changed after signing, or a key not in `allowed_signers` signed it.
- `attention::order::expired`: the order's `expires` date has passed.
- `attention::order::not_an_order`: the path holds something else.
- `attention::order::invalid`: the order or its context cannot be read, or
  breaks a rule of the record above.
- `attention::order::context_too_large`: the context is over 64 KiB.
- `attention::signers::missing`: there is no
  `~/.config/attention/allowed_signers`.
- `attention::signers::untrusted`: the allowed signers file belongs to
  another user, or others may write it.
- `attention::verify::failed`: the signature could not be checked.
- `attention::board::refused`: Board refused a read or the put, with
  Board's code in the field `code`.

`sign` also refuses with:

- `attention::sign::declined`: the owner asked for changes, given in
  `feedback`.
- `attention::sign::cancelled`: the owner closed the window without
  deciding.
- `attention::sign::no_signer`: this Board has no signer, as in a debug
  build.
- `attention::sign::failed`: the signer did not sign, or its signature did
  not verify.

`pinned` lists an order it leaves out with its code, and with
`attention::pinned::over_budget` when the pinned orders before it already
hold 32 KiB.

## Gates

`crates/board-plugin-attention/src/tests.rs` tests the manifest, the schema
and each check and code mapping. `crates/board-cli/src/tests/built_in/attention.rs`
runs every capability through Board with a test key that signs synthetic
orders. Neither covers the window or Touch ID. The mod's tests stand in for
`board`, so they open no window and verify no signature; they prove the
context block, the band, the pane and the messages an agent gets.
