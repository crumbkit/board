# Across repositories and vectors

This file holds detail for docs/architecture/writes/signing.md.

## Across repositories

| Statement | Built by | Signed by | Verified by | Namespace |
| --- | --- | --- | --- | --- |
| Approval | board `Effect::approval` | board-app `board-sign approve` | board `board_plugin::verify` | `board/approve/v1` |
| Owner's write | board `Effect::owner_write` | board-app `board-sign approve` | board `board_plugin::verify` | `board/write/v1` |
| Order | board `Order::statement` | board-app `board-sign sign` | board `Host::verify_order` | `attention/order/v1` |

- board-app's pairing writes both allowed signers files.
- Board verifies with `ssh-keygen`. board-app builds the SSHSIG armor
  itself.
- The vectors' v1 manifest still names board-attention as the order
  layout's owner, because a released version is never edited. Board owns
  the order layout now, because the attention plugin is built into Board.

## Vectors

- `vectors/signing/v1/` holds the statements, signatures, a test key and an
  allowed signers file. `vectors/signing/README.md` says how each file was
  made and which repository owns each layout. board-app is to test against
  them.
- `crates/board-ops/tests/signing_vectors.rs` checks that Board builds all
  three statements byte for byte, maps each to its namespace, admits the
  owner's signatures and refuses those in another namespace or by another
  key.
- `released.json` lists every vector file, so the `released` test refuses an
  edit in place.
- A changed layout or namespace adds `vectors/signing/v2/`. The pull
  request names the board-app pull request that takes it, and both merge
  the same day.
