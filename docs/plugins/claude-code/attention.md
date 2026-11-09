# board-attention

This file holds detail for docs/plugins/claude-code.md. It says what the
attention plugin's mod, `plugins/attention/claude-code/`, shows and runs.
plugins/attention/README.md describes orders and their codes.

The mod reads and signs orders in the personal board through `board
--personal`. When there is no personal board, Board refuses with
`board::config::not_found`, and the mod shows nothing and adds nothing to
the session. It reads the pinned orders once per session start either way.

## Pinned orders

As a session starts, the mod runs `board --personal --json call
attention.pinned`, which verifies each pinned order. It adds the verified
orders, and the path and code of each one left out, to the session's first
message as one context block, `attentionOrders`. Claude Code sends that
block once and again after a compaction. The band above the prompt shows
one Orders button with the count of verified orders; [band.md](band.md) says
what it shows. The button opens a pane that lists the orders, and each one
opens its expiry, the signer that verified it and its context. A left-out
order or a personal board that could not be read turns the button red; the
pane gives the code and message of each.

## Asking the owner to sign

The mod gives agents the tool `mcp__board-attention__request_signature`,
which takes an order's path in the personal board. A line appears above the
owner's prompt saying the order is waiting, with **Review and sign** and
**Dismiss**. **Review and sign** runs `board --personal --json call
attention.sign --path <path>` and waits up to 10 minutes, the longest a
mod's process may run. The window and Touch ID do the signing. The mod reads
the outcome from Board's answer:

- Success sends the agent a message saying the owner signed.
- `attention::sign::declined` sends the agent the owner's requested changes.
- `attention::sign::cancelled` keeps the request waiting.
- Any other code sends the agent the failure with its code and message.
- A sign killed before Board answered, or still running after 10 minutes,
  keeps the request waiting and tells the owner the outcome is unknown. The
  owner checks it with `board --personal call attention.verify --path
  <path>`.

**Dismiss** tells the agent the owner dismissed the request without signing.
