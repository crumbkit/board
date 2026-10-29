# Signing vectors

Fixed statements, signatures and test keys for the bytes viitta, board-app
and board-attention build, sign and verify. viitta's tests check them; the
other two repositories are to test against them.
docs/architecture/writes/signing.md says how they are used.

- The files live in `python/board-plugin/src/board_plugin/vectors/`, so the
  `board-plugin` package built from a checkout holds them; the root
  `vectors` is a link to that directory. Python reads them with
  `importlib.resources.files("board_plugin") / "vectors/signing/v1"`.
  `bin/package-plugin` leaves them out of plugin archives.
- A version is never edited: `released.json` lists every file, and the
  `released` test refuses a change. A changed layout or namespace is a new
  directory, `v2/`, with a new first line.
- `manifest.json` names each layout's namespace and owner, each signature's
  statement, namespace and signer, and the SHA-256 of every other file.

## Layouts and owners

| Statement | Owner | Built by | Namespace |
| --- | --- | --- | --- |
| `approval.statement` | viitta | `board_model::Effect::approval` of `approval.effect.json` | `board/approve/v1` |
| `write.statement` | viitta | `board_model::Effect::owner_write` of `approval.effect.json` | `board/write/v1` |
| `order.statement` | board-attention | `board_attention.order.statement` of `order.record` and `order.context.md` | `attention/order/v1` |

The owner of a layout changes it, and opens the viitta pull request that adds
the version. `order.record` is the order's JSON without a `.json` extension,
because viitta is a board and has no schema for `attention/order/v1`.

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
