# orders, formerly attention

A Board plugin for orders: instructions the owner signs with Touch ID, which
agents verify instead of going back to the owner's chat to check. It is
built into Board as `crates/board-plugin-orders`. It was named `attention`
before: `attention.*` calls and `attention` in a board's plugin list still
reach it in this release, each with a `board::plugin::former_name` warning
naming its `orders` replacement, and this skill keeps the name `attention`
until Board's next release drops the old name. Orders live in the
owner's personal board, the `personal` entry in
`~/.config/board/config.json`, and every command here names it with
`--personal`.

## How an order works

1. An agent writes a draft order, an `attention/order/v1` record at
   `/orders/<name>`, and a Markdown context file at `/orders/<name>.md`. The
   order is the binding instruction. The context is the non-binding
   background from the conversation, so a reader can tell what the owner
   meant.
2. The owner runs `board --personal orders sign --path
   /orders/<name>`, or presses **Review and sign** in the Claude Code mod.
   Board shows the order through its signer, which opens a window showing
   the order, its expiry, whether it is pinned, and the context as plain
   text. If the owner clicks **Sign with Touch ID**, Touch ID signs the
   statement, and `sign` writes the signature into the order with one put
   that expects the blob it read. An order edited while the owner reads it
   is refused with Board's `board::put::stale` and keeps its new text
   unsigned. If the owner clicks **Request changes…** instead, `sign`
   refuses with `orders::sign::declined`, and its `feedback` field holds
   what the owner wrote.
3. Any agent given the path runs
   `board --personal orders verify --path /orders/<name>`. It gets
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
order by editing it breaks the signature. `board --personal
orders pinned` lists each pinned order in the board's own tree that
verifies, by path, up to 32 KiB of orders and context, and lists the rest
with the code that says why each was left out. It does not read orders in
boards the board mounts.

## Setup

Install the signer from stemmacorp/board-app as Board's signing docs say
(docs/architecture/writes/signing.md). Board runs `~/.local/bin/board-sign`,
or `~/.local/bin/attention-sign` while board-sign is not installed. The
owner's key must be in `~/.config/board/allowed_signers` as the principal
`owner`, on a line whose `namespaces=` names `attention/order/v1` or that
has no `namespaces=`. The file must belong to the owner, and no one else may
write it. Then enable the plugin in the personal board:

```sh
board --personal plugin enable orders
```

### Moving from the attention allowed signers file

Orders used to verify against `~/.config/attention/allowed_signers`. In this
release, when no line of Board's file admits an order's key, Board checks
the attention file, keeps the order and warns `orders::signers::legacy`,
naming the command that prints the lines to add. Board's next release stops
reading the attention file, and those orders then fail with
`orders::order::bad_signature`.

`board --personal orders signers` prints those lines: Board's file with each
key of the attention file merged in. It writes neither file; the owner reads
the lines and writes Board's file. A key in both files keeps Board's line,
which gains `attention/order/v1` when its `namespaces=` lacks it; a key only
in the attention file gets `owner namespaces="attention/order/v1" <key>`;
and two lines of Board's file with one key are listed in `duplicates` and
printed as one line holding both lines' namespaces. `changes` gives each
printed line that differs, as `gained`, `added` or `merged`. A Board file that group or others may write
stops orders as it stops approvals, with `orders::signers::untrusted`.

`board skills install` installs `SKILL.md`, which tells an agent when and
how to read, check and draft orders.

### Moving from the Python plugin

While the Python plugin's link is still installed, `board call
attention.*` is refused with `board::plugin::ambiguous`, naming both it and
the orders plugin; `board call orders.*` answers.

1. Remove the link `~/.local/share/board/plugins/attention`.
2. Remove the link `~/.claude/skills/attention`, then run
   `board skills install`. Until the link is gone, install refuses with
   `board::skills::not_board`.
3. In Claude Code, run `claude plugin uninstall attention-approve@attention`
   and `claude plugin marketplace remove attention`, then install
   `board-orders@board` as below.
4. Delete `~/.config/attention/board`, which nothing reads now.
5. Run `board --personal plugin enable orders`. It writes nothing,
   because the schema's bytes are unchanged.

## The Claude Code mod

`plugins/orders/claude-code/` is the Claude Code mod `board-orders`. It
replaces `board-attention` in `claude-code/`, which stays silent while
`board-orders` is enabled and asks to be uninstalled.
docs/plugins/claude-code/attention.md says what each shows and runs.
Install the new one and remove the old one at a Claude Code terminal prompt:

```
claude plugin marketplace add crumbkit/board
claude plugin install board-orders@board
claude plugin uninstall board-attention@board
```

## Signing

- Board's signing owner builds the statement, runs the signer and checks the
  signature (STANDARDS.md 29). The plugin asks for both through
  `Host::sign_order` and `Host::verify_order`, and names no signing literal.
- An agent never signs an order, edits a `signature`, or runs
  `orders.sign` without the owner at the Mac.

## What it does not protect against

Any program running as the owner can edit `allowed_signers` and add its own
key, or replace the signer with one that shows one thing and signs another.
Install the signer from a reviewed commit, and check that the key in
`allowed_signers` is the one the signer prints. The Secure Enclave
guarantees only that the owner's key signs nothing without Touch ID.

## Codes

Each code below names the capabilities that refuse with it. `pinned`
refuses only with `orders::board::refused` and
`orders::call::bad_input`. It lists each order it leaves out in
`left_out`, with the code `verify` would refuse that order with.

- `orders::order::unsigned` (`verify`): the order has no signature yet.
- `orders::order::bad_signature` (`verify`): the order, its expiry or
  its context changed after signing, or a key neither allowed signers file
  admits signed it.
- `orders::order::expired` (`verify`, `sign`): the order's `expires`
  date has passed. `sign` refuses before the owner sees the order.
- `orders::order::not_an_order` (`verify`, `sign`): the path holds
  something else.
- `orders::order::invalid` (`verify`, `sign`): the order or its context
  cannot be read, or breaks a rule of the record above.
- `orders::order::context_too_large` (`verify`, `sign`): the context is
  over 64 KiB.
- `orders::signers::missing` (`verify`): neither
  `~/.config/board/allowed_signers` nor the attention file is there.
- `orders::signers::untrusted` (`verify`): Board's allowed signers file,
  or the attention file it fell back to, belongs to another user, or others
  may write it.
- `orders::verify::failed` (`verify`): the signature could not be
  checked.
- `orders::signers::missing`, `orders::signers::untrusted` and
  `orders::signers::failed` (`signers`): Board's allowed signers file is not
  named, a file is not the owner's alone, or the files could not be read.
- `orders::signers::legacy`, a warning beside a verified order: only the
  attention file admits its key.
- `orders::sign::declined` (`sign`): the owner asked for changes, given
  in `feedback`.
- `orders::sign::cancelled` (`sign`): the owner closed the window
  without deciding.
- `orders::sign::no_signer` (`sign`): this Board has no signer, as in a
  debug build.
- `orders::sign::failed` (`sign`): the signer did not sign, or its
  signature did not verify.
- `orders::pinned::over_budget` (in `pinned`'s `left_out`): the pinned
  orders before this one already hold 32 KiB.
- `orders::board::refused` (`verify`, `sign`, `pinned`): Board refused a
  read, the query or the put, with Board's code in the field `code`.
- `orders::call::bad_input` (`verify`, `sign`, `pinned`): the input does
  not match the capability's schema, such as a missing path or an unknown
  field.

## Gates

`crates/board-plugin-orders/src/tests.rs` tests the manifest, the schema
and each check and code mapping. `crates/board-cli/src/tests/built_in/attention.rs`
runs every capability through Board with a test key that signs synthetic
orders. Neither covers the window or Touch ID. The mod's tests stand in for
`board`, so they open no window and verify no signature; they prove the
context block, the band, the pane and the messages an agent gets.
