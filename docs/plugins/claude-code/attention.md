# board-orders and board-attention

This file holds detail for docs/plugins/claude-code.md. It says what the
orders plugin's mod, `board-orders` in `plugins/orders/claude-code/`, shows
and runs, and how `board-attention` in `plugins/attention/claude-code/`, the
mod it replaces, steps aside. plugins/attention/README.md describes orders
and their codes.

The mod reads and signs orders in the personal board through `board
--personal`. When there is no personal board, Board refuses with
`board::config::not_found`, and the mod shows nothing and adds nothing to
the session. It reads the pinned orders once per session start either way.

## Pinned orders

As a session starts, the mod runs `board --personal --json call
orders.pinned`, which verifies each pinned order. It adds the verified
orders, and the path and code of each one left out, to the session's first
message as one context block, `pinnedOrders`. The block gives each order's
path, expiry and signed text, not its context; it tells the agent to read
the context with `board --personal orders verify --path <order>` when it
needs to interpret the order. Claude Code sends that
block once and again after a compaction. The band above the prompt shows
one Orders button with the count of verified orders; [band.md](band.md) says
what it shows. The button opens a pane that lists the orders, and each one
opens its expiry, the signer that verified it and its context. A left-out
order or a personal board that could not be read turns the button red; the
pane gives the code and message of each.

## Asking the owner to sign

The mod gives agents the tool `mcp__board-orders__request_signature`,
which takes an order's path in the personal board. A line appears above the
owner's prompt saying the order is waiting, with **Review and sign** and
**Dismiss**. **Review and sign** runs `board --personal --json call
orders.sign --path <path>` and waits up to 10 minutes, the longest a
mod's process may run. The window and Touch ID do the signing. The mod reads
the outcome from Board's answer:

- Success sends the agent a message saying the owner signed.
- `orders::sign::declined` sends the agent the owner's requested changes.
- `orders::sign::cancelled` keeps the request waiting.
- Any other code sends the agent the failure with its code and message.
- A sign killed before Board answered, or still running after 10 minutes,
  keeps the request waiting and tells the owner the outcome is unknown. The
  owner checks it with `board --personal call orders.verify --path
  <path>`.

**Dismiss** tells the agent the owner dismissed the request without signing.

## The mod board-orders replaces

`board-attention` does what `board-orders` does under the old names: it
calls `attention.*`, which Board answers as `orders.*` with a warning, adds
the block `attentionOrders` and gives agents
`mcp__board-attention__request_signature`. As a session starts it reads the
settings' `enabledPlugins`:

- While `board-orders` is enabled, it runs no board, registers no tool and
  adds no block, so a session sees each order once. Its band line asks the
  owner to run `claude plugin uninstall board-attention@board`.
- Otherwise it shows the orders as before, and its band line beside them
  names `claude plugin install board-orders@board`, then the uninstall
  command.

Board's next release drops the `attention` name, and `board-attention` with
it.
