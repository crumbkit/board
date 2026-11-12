# Signing

This file holds detail for docs/architecture/writes/effects.md. It says what
Board signs, how it checks a signature, and what crumbkit/board and
board-app, the two repositories that build, sign or verify the bytes, agree
on.

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
  - It leaves out the effect's `asked` time, so the same request signs the
    same statement whenever it was asked.
- An owner's write signs `Effect::owner_write`. It starts `board/write/v1`
  and shows the op, path and board, then the content as an approval shows
  it.
- An attention order signs `Order::statement`, in
  `board_model::effect::order`. It starts `attention/order/v1`, then gives
  the expiry as `YYYY-MM-DD`, `pinned: yes` or `pinned: no` (`no` when the
  record does not say), the order under `Order (binding):` and the context
  file's path and text under `Context (non-binding, from <path>):`. The
  order and context text are copied as stored, so the statement ends with
  the context file's last byte.
- Each statement is signed in the namespace its first line names. No
  signature verifies in another namespace, so a key limited to approvals
  cannot sign an owner's write or an order.
- The three layouts are frozen by the vectors below: a change needs a new
  first line and a new vectors version.

## The signer

- `board answer <id> approve` and an owner's write write the statement to a
  0600 file in the cache and run `<signer> approve <file>`.
- A built-in plugin signs an order with `Host::sign_order`, which writes the
  statement to a 0600 file in the cache, runs `<signer> sign <file>`,
  removes the file and verifies the signature before returning it. It says
  why there is none: the owner asked for changes, with their words, or
  cancelled; there is no signer; or the signer failed.
- The signer is `.local/bin/attention-sign` in the home that the passwd
  entry of the running user names. `HOME`, `PATH` and the XDG variables do
  not move it. A debug build, which the tests run, has no signer. It sees
  only `PATH` and `HOME`, and has 20 seconds for an approval or an owner's
  write. The check has 3 seconds, so an answer fits a 30-second caller such
  as the effects mod. It has 10 minutes for an order, whose context of up
  to 64 KiB the owner reads before touching the sensor.
- attention-sign shows the statement in a native window and signs with the
  attention plugin's Secure Enclave key after Touch ID. Exit 3 means the
  owner asked for changes, with their words on stdout. Every other failure
  exits 1. When the owner presses Cancel or closes the window, the last line
  of its stderr is
  `attention-sign: the owner cancelled, so nothing was signed`, which Board
  reports as a cancel; AppKit may write other lines before it.
- When the owner cancels at the Touch ID sheet instead, attention-sign
  exits 1 with the LAError text on stderr and no cancel line, so Board
  reports a failed signer.
- Under `approve` attention-sign signs only a statement whose first
  line is `board/approve/v1` or `board/write/v1`; under `sign` it refuses
  those and signs in `attention/order/v1`.

## Verifying

- `/usr/bin/ssh-keygen -Y verify`, at a fixed path so `PATH` cannot supply
  another verifier, checks the signature against
  `.config/board/allowed_signers` in the same passwd home for the principal
  `owner`. It runs before an answer is written, whenever a decide reads an
  approval, and as an owner's write runs.
- `Host::verify_order` checks an order's signature the same way, in
  `attention/order/v1`, against `.config/attention/allowed_signers` in the
  same passwd home. It returns `owner`, or why not: no file is named or
  there, the file is untrusted, the signature is bad, or the verifier
  failed. board-app's pairing writes the file; Board never writes it.
- The signer and both allowed signers files must be regular files that the
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

STANDARDS.md 29 states the rule; `rules/signing-one-owner.json`
records it.

- `board_model::effect` builds the statements and names the namespaces.
- `board_plugin::sign` runs the signer and `ssh-keygen` and checks the
  allowed signers file.
- Only board-ops's effects module calls them. Clippy's `disallowed-methods`
  refuses any other caller of `sign`, `verify`, `Effect::approval`,
  `Effect::owner_write` and `Order::statement`; each caller carries an
  `#[expect]` that names it.
- A built-in plugin signs and verifies an order only through
  `Host::sign_order` and `Host::verify_order`. Their provided bodies refuse,
  and only `Hosted`, Board's own host, overrides them, by calling the
  effects module's `sign_order` and `verify_order`. Clippy refuses any other
  caller of those two functions.
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
| Approval | board `Effect::approval` | board-app `attention-sign approve` | board `board_plugin::verify` | `board/approve/v1` |
| Owner's write | board `Effect::owner_write` | board-app `attention-sign approve` | board `board_plugin::verify` | `board/write/v1` |
| Order | board `Order::statement` | board-app `attention-sign sign` | board `Host::verify_order` | `attention/order/v1` |

- Board reads `~/.config/board/allowed_signers` for approvals and owner's
  writes, and `~/.config/attention/allowed_signers` for orders. board-app's
  pairing writes both.
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

## What an agent never does

- Add a caller of a signing function, an owner to `signing.owners`, or an
  `#[expect]` for either gate, without the owner's approval.
- Write `~/.config/board/allowed_signers`,
  `~/.config/attention/allowed_signers` or `~/.local/bin/attention-sign`.
- Run `ssh-keygen -Y sign` with any key but a test key.
- Change a statement's layout without a new first line and a new vectors
  version.
