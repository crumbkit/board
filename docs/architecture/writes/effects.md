# Effects

This file holds detail for docs/architecture.md, Commands. It says what only
the owner may write, and how a plugin asks the owner before an op.

## Who is checked

- A Board run with `BOARD_PLUGIN` set is checked as that plugin. `board call`
  sets it to the plugin's name, or keeps the value it already has, so a
  plugin that calls another stays named.
- A run without `BOARD_PLUGIN` is the owner's.
- `board call` also passes `XDG_CONFIG_HOME`, so a nested run finds the
  same known boards file and personal board.
- A plugin's `decide`, and every `answer`, read the known boards file. A
  file that cannot be read, or a personal board that is not a board, refuses
  them with `board::config::invalid` or the open error.
- A `BOARD_PLUGIN` that is empty, not UTF-8 or not a plugin's name refuses
  the run with `board::config::invalid`.

## Records

Each is a Board type, checked by its Rust type, with no schema file.

- `board/effect/v1`: a pending effect at `/effects/pending/<id>` in the
  personal board. It names the board root, plugin, op and path.
  - It may also hold `record` or `text`, `digest` and `prior`. A decide
    sets none of them. Only an owner's signed write by hand can set them.
  - Board never sets `prior` yet.
- `board/seen/v1`: the owner's answer at `/effects/seen/<id>`: `approve` or
  `refuse`, with an optional `reason`. An approval holds the owner's
  `signature`.
- The id is the 64 hex digits of the SHA-256 of the canonical JSON of the
  board root, plugin, op, path, digest and prior blob.

## What only the owner writes

A plugin's put or `rm` is refused with `board::settings::from_plugin`,
whatever the record type, compared with ASCII case folded, when it:

- names a path that is not ASCII, because a case-insensitive file system
  folds characters such as `ſ` into ASCII letters;
- lands under `.board/` in any board, except a schema a batch line adds
  under `.board/types/`, which is refused as `type add` refuses if it changes
  a schema;
- lands under `effects/` in any board, because the known boards file that
  names the personal board is the caller's to choose;
- puts a `board/effect/v1` or `board/seen/v1` record in any board;
- puts or removes a link or mount record.

Every other put or `rm` by a plugin is written.

- The check runs after the record is validated and its target is known.
- A file put with a store is checked before the bytes are uploaded, by the
  mounted board when the path leads through a mount.
- A batch is refused with `board::put::invalid` when any line is refused;
  each refused line carries its own code.
- Under a plugin, `init`, `plugin enable` and `type add` are refused with
  `from_plugin`, because they write without a put.

## `board decide`

`board decide --op <plugin>.<capability> --path <path>` asks whether the
plugin `BOARD_PLUGIN` names may run its op at the path.

- It resolves the path as a put does: after links, and through a mount in
  the mounted board. The effect names that board's root and the path there.
- The owner's approval admits the op, and the report names the effect.
- The owner's refusal refuses it with `board::settings::refused`, whose
  `effect` field is the id.
- With no answer it writes the pending effect and refuses with
  `board::settings::asked`, whose `effect` field is the id. Asking again
  writes nothing new.
- With no personal board named it refuses with `board::settings::no_personal`.
- The owner's own decide is admitted and writes nothing.

## `board answer`

`board answer <id> approve|refuse [--reason <text>]` writes the seen record
and removes the pending effect, in one commit.

- The records under `/effects/pending/` are the open effects. Git history
  keeps an answered one.
- A repeated decide finds the seen record by the id.
- Answering the same id again is refused with `not_found`, whose hint says
  the effect was answered or never asked. The owner changes an answer by
  deleting its seen record, or editing it with a new signature.
- The seen record is written only where none is, and the pending effect is
  removed only if it is unchanged. Of two answers at once, the second is
  refused with `board::put::stale`.
- A decide writes no pending effect if the owner answered after it read
  the answer.
- A plugin may not answer (`from_plugin`).
- An id that is not 64 lowercase hex digits, names no pending effect, or
  names one whose content does not hash to the id and digest is refused
  with `board::settings::not_found`.

## Signed approvals

Every approval carries the owner's Touch ID signature. An agent's shell has
no `BOARD_PLUGIN`, so it counts as the owner and could otherwise approve its
own effects.

- `board answer <id> approve` writes the statement `Effect::approval` to a
  0600 file in the cache and runs `<signer> approve <file>`.
  - The signer is `.local/bin/attention-sign` in the home that the passwd
    entry of the running user names. `HOME`, `PATH` and the XDG variables
    do not move it. It sees only `PATH` and `HOME`, and has 20 seconds.
    The check has 3 seconds, so an answer fits a 30-second caller such as
    the effects mod.
  - attention-sign shows the statement in a native window and signs with
    the attention plugin's Secure Enclave key after Touch ID. Exit 3 means
    the owner asked for changes.
- The statement starts `board/approve/v1` and names the effect id, which
  binds the content and prior blob. It shows the plugin, op, path and board,
  then the content's size in bytes and lines and the content split at each
  `\n`. Hidden characters, `\r` among them, are escaped as
  `board_model::escaped` does.
- Every stored signature is checked against the rebuilt statement, so its
  layout is frozen: a change needs a new first line.
- The signature is an SSH signature in the namespace the statement's
  first line names: `board/approve/v1` for an approval, `board/write/v1`
  for an owner's write.
  - `attention-sign approve` signs only a statement whose first line is
    one of the two.
  - A directive is signed in `attention/directive/v1`.
  - No signature verifies in another namespace, so a key limited to
    approvals cannot sign an owner's write.
- `/usr/bin/ssh-keygen -Y verify`, at a fixed path so `PATH` cannot supply
  another verifier, checks it against `.config/board/allowed_signers` in
  the same passwd home for the principal `owner`. The check runs before
  the answer is written and whenever a decide reads an approval.
- The signer and the allowed signers file must be regular files that the
  user or root owns and that neither group nor others may write.
- Board accepts any key that the file lists for a principal pattern
  matching `owner`, so the file should hold only attention-sign's Secure
  Enclave key, limited to the namespaces:

  ```sh
  echo "owner namespaces=\"board/approve/v1,board/write/v1\" $(attention-sign key)" > ~/.config/board/allowed_signers
  ```

  A file copied from git's allowed signers, with an ordinary key in
  `~/.ssh`, would let an agent sign approvals with `ssh-keygen` and no
  Touch ID.
- A missing, declined or bad signature is refused with
  `board::settings::unsigned`, whose `effect` field is the id. Nothing is
  written. A refusal is unsigned.

### The owner's own writes

`board put`, `put --file` and `put --batch` without `BOARD_PLUGIN` need the
owner's signature for a file under `effects/`, so an agent in the owner's
shell cannot write or remove a pending effect or an answer with a plain
put. The check applies in every board, because the known boards file is the
caller's to choose. It matches the working-tree path in any ASCII case. A
first directory that is not ASCII also needs a signature, because a file
system may fold it to `effects`. The owner's `rm` of `.board/board.json`
also needs a signature, because the board does not open without it. A put
of it does not.

- The put runs `<signer> approve <file>` on the statement
  `Effect::owner_write`. The statement starts `board/write/v1` and shows the
  op, path and board, then the content as an approval shows it.
- The signer runs while the put holds the board lock, so another write to
  that board waits and may time out. Each such line of a batch asks once.
- Board checks the signature as the put runs and keeps no copy, so the
  statement's layout is not frozen.
- A missing, declined or bad signature is refused with
  `board::settings::unsigned`, whose `path` field is the logical path, and
  nothing is written. A file put whose bytes are not UTF-8 cannot be shown,
  so it is refused the same way.
- Board's own writes of a pending effect and of a seen record do not ask.

## Limits

- A plugin's put never asks the owner. Only the plugin's own decide asks.
- A plugin that unsets `BOARD_PLUGIN` escapes the check. The check catches
  mistakes by a plugin that keeps it set.
- An approval names no content, so it admits that op at that path again
  until the owner removes the seen record.
- `board call` runs a plugin, which must ask with `decide` itself.
- A plugin can set `BOARD_PLUGIN` to another installed plugin's name.
- A plugin sets `XDG_CONFIG_HOME` for the Board it runs, so it picks the
  known boards file and so the personal board its decide asks in.
- A plugin may run `sync` and `mount update`, which the notes plugin
  needs. They push commits and move a pin without asking.
- A seen record over the record size limit is refused as
  `board::json::too_large`.
- Seen records stay in the personal board after use.
- The signatures stop an agent that runs as the owner from approving its
  own effect with a plain `board answer`, or from writing under `effects/`
  with a plain `board put`. They do not stop it from:
  - writing the file and committing it with git, outside Board;
  - replacing `~/.local/bin/attention-sign` or editing
    `~/.config/board/allowed_signers`, which the account may write;
  - asking the owner to sign a statement it wrote, in a window that looks
    like a real approval.

  Only the sandbox the agent runs in stops those.
