# board-attention

This file holds detail for docs/plugins/claude-code.md. It says what the
attention plugin's mod, `plugins/attention/claude-code/`, shows and runs.
plugins/attention/README.md describes orders and their codes.

The mod reads and signs orders in the personal board through `board
--personal`. When there is no personal board, Board refuses with
`board::config::not_found`, and the mod shows nothing and adds nothing to
the session.

## Pinned orders

As a session starts, the mod runs `board --personal --json call
attention.pinned`, which verifies each pinned order. It adds the verified
orders, and the path and code of each one left out, to the session's first
message as one context block, `attentionOrders`. Claude Code sends that
block once and again after a compaction. A line above the prompt says how
many signed orders bind the session, and says in red how many were left out
of context or that the personal board could not be read. Its **Show** button
opens a pane that lists each order with its expiry and the signer that
verified it, and gives the code and message for each one left out.

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

**Dismiss** tells the agent the owner dismissed the request without signing.
