# Signing vectors

Fixed statements, signatures and test keys for the bytes board and
board-app build, sign and verify. crumbkit/board's tests check them, and
board-app is the only other repository that is to test against them.
docs/architecture/writes/signing.md says how they are used.

- The files live in `vectors/signing/` at the root of crumbkit/board.
  Another repository reads the version it tests against from a pinned
  commit of crumbkit/board.
- A version is never edited: `released.json` lists every file, and the
  `released` test refuses a change. A changed layout or namespace is a new
  directory, `v2/`, with a new first line.
- `manifest.json` names each layout's namespace and owner, each signature's
  statement, namespace and signer, and the SHA-256 of every other file.

## Layouts and owners

| Statement | Owner | Built by | Namespace |
| --- | --- | --- | --- |
| `approval.statement` | board | `board_model::Effect::approval` of `approval.effect.json` | `board/approve/v1` |
| `write.statement` | board | `board_model::Effect::owner_write` of `approval.effect.json` | `board/write/v1` |
| `order.statement` | board | `board_model::effect::order::Order::statement` of `order.record` and `order.context.md` | `attention/order/v1` |

`v1/manifest.json` records the owner of the first two layouts as `viitta`,
crumbkit/board's earlier name, and the owner of the order layout as
`board-attention`, the repository that held the attention plugin before
board built it in. A version is never edited, so these values stay until a
new version replaces them.

The owner of a layout changes it, and opens the crumbkit/board pull request that adds
the version. `order.record` is the order's JSON without a `.json` extension,
because crumbkit/board's own board does not enable the attention plugin.

## Keys

- `key`, `key.pub`: an ECDSA P-256 test key, principal `owner`. It signs
  nothing real.
- `other`, `other.pub`: a second test key, principal `other`, for the
  wrong-key case.
- `allowed_signers`: `owner` limited to the three namespaces, `other` limited
  to `board/approve/v1`.

Each key was made with `ssh-keygen -q -t ecdsa -b 256 -N '' -C 'vectors <principal>' -f <key>`.

## Signatures

Each `.sig` was made with
`ssh-keygen -Y sign -q -f <key> -n <namespace> < <statement> > <sig>`, with the
key, namespace and statement the manifest gives it:

- `approval.sig`, `write.sig`, `order.sig`: the owner's signature of each
  statement in its own namespace. Each verifies.
- `cross/approval-in-write.sig`, `cross/write-in-approval.sig`,
  `cross/order-in-directive.sig`: the owner's key in the wrong namespace.
  Each is refused.
- `cross/approval-by-other.sig`: `other`'s key in the right namespace. Board
  verifies as `owner`, so it is refused.

`approval.raw` is the 64 bytes r‖s of the ECDSA signature inside
`approval.sig`, each padded to 32 bytes, for a signer that builds the armor
itself.
