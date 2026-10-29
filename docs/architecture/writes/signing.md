# Signing

This file holds detail for docs/architecture/writes/effects.md. It says what
Board signs, how it checks a signature, and what the three repositories that
sign agree on.

## What is signed

Every approval carries the owner's Touch ID signature. An agent's shell has
no `BOARD_PLUGIN`, so it counts as the owner and could otherwise approve its
own effects.

- An approval signs `Effect::approval`.
  - It starts `board/approve/v1` and names the effect id, which binds the
    content and prior blob.
  - It shows the plugin, op, path and board, then the content's size in
    bytes and lines and the content split at each `\n`. Hidden characters,
    `\r` among them, are escaped as `board_model::escaped` does.
- An owner's write signs `Effect::owner_write`. It starts `board/write/v1`
  and shows the op, path and board, then the content as an approval shows
  it.
- Each statement is signed in the namespace its first line names. No
  signature verifies in another namespace, so a key limited to approvals
  cannot sign an owner's write.
- Both layouts are frozen by the vectors below: a change needs a new first
  line and a new vectors version.

## The signer

- `board answer <id> approve` and an owner's write write the statement to a
  0600 file in the cache and run `<signer> approve <file>`.
- The signer is `.local/bin/attention-sign` in the home that the passwd
  entry of the running user names. `HOME`, `PATH` and the XDG variables do
  not move it. It sees only `PATH` and `HOME`, and has 20 seconds. The check
  has 3 seconds, so an answer fits a 30-second caller such as the effects
  mod.
- attention-sign shows the statement in a native window and signs with the
  attention plugin's Secure Enclave key after Touch ID. Exit 3 means the
  owner asked for changes. It signs only a statement whose first line is
  `board/approve/v1` or `board/write/v1`.

## Verifying

- `/usr/bin/ssh-keygen -Y verify`, at a fixed path so `PATH` cannot supply
  another verifier, checks the signature against
  `.config/board/allowed_signers` in the same passwd home for the principal
  `owner`. It runs before an answer is written, whenever a decide reads an
  approval, and as an owner's write runs.
- The signer and the allowed signers file must be regular files that the
  user or root owns and that neither group nor others may write.
- Board accepts any key that the file lists for a principal pattern matching
  `owner`, so the file should hold only attention-sign's Secure Enclave key,
  limited to the namespaces:

  ```sh
  echo "owner namespaces=\"board/approve/v1,board/write/v1\" $(attention-sign key)" > ~/.config/board/allowed_signers
  ```

  A file copied from git's allowed signers, with an ordinary key in
  `~/.ssh`, would let an agent sign approvals with `ssh-keygen` and no Touch
  ID.

## Owners and gates

STANDARDS.md 29 states the rule; `controls/rules/signing-one-owner.json`
records it.

- `board_model::effect` builds the statements and names the namespaces.
- `board_plugin::sign` runs the signer and `ssh-keygen` and checks the
  allowed signers file.
- Only board-ops's effects module calls them. Clippy's `disallowed-methods`
  refuses any other caller of `sign`, `verify`, `Effect::approval` and
  `Effect::owner_write`; each caller carries an `#[expect]` that names it.
- `cargo xtask lint` fails with `xtask::signing::outside_owner`, naming the
  file, line and literal, when source code outside `signing.owners` in
  `lint.toml` names a namespace, `ssh-keygen`, `attention-sign`, the allowed
  signers path or the signature armor. It reads `.rs`, `.py`, `.sh`, `.ts`
  and `.tsx` files, and scripts without an extension under `bin/` and
  `plugins/`.
- `deny.toml` bans `ssh-key`, `ssh-encoding`, `p256`, `ecdsa` and
  `openssl`, so no crate signs or verifies in process.

## Across repositories

| Statement | Built by | Signed by | Verified by | Namespace |
| --- | --- | --- | --- | --- |
| Approval | viitta `Effect::approval` | board-app `attention-sign approve` | viitta `board_plugin::verify` | `board/approve/v1` |
| Owner's write | viitta `Effect::owner_write` | board-app `attention-sign approve` | viitta `board_plugin::verify` | `board/write/v1` |
| Order | board-attention `order.statement` | board-app `attention-sign sign` | board-attention `verified_signer` | `attention/order/v1` |

- Board reads `~/.config/board/allowed_signers`; board-attention reads
  `~/.config/attention/allowed_signers`. board-app's pairing writes both.
- Both verifiers run `ssh-keygen`. board-app builds the SSHSIG armor itself.

## Vectors

- `vectors/signing/v1/` holds the statements, signatures, a test key and an
  allowed signers file; its README says how each file was made and which
  repository owns each layout. The `board-plugin` Python package ships them
  in a checkout, so board-attention can read the version its pin names.
  Plugin archives leave them out.
- `crates/board-ops/tests/signing_vectors.rs` checks that Board builds both
  statements byte for byte, maps each to its namespace, admits the owner's
  signatures and refuses those in another namespace or by another key.
- `released.json` lists every vector file, so the `released` test refuses an
  edit in place.
- A changed layout or namespace adds `vectors/signing/v2/`. The pull
  request names the board-app and board-attention pull requests that take
  it, and all three merge the same day.

## What an agent never does

- Add a caller of a signing function, an owner to `signing.owners`, or an
  `#[expect]` for either gate, without the owner's approval.
- Write `~/.config/board/allowed_signers` or `~/.local/bin/attention-sign`.
- Run `ssh-keygen -Y sign` with any key but a test key.
- Change a statement's layout without a new first line and a new vectors
  version.
