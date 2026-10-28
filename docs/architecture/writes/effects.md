# Effects

This file holds detail for docs/architecture.md, Commands. It says how the
owner decides what a plugin may do, and how a plugin asks.

## Who is checked

- A Board run with `BOARD_PLUGIN` set is checked as that plugin. `board call`
  sets it to the plugin's name, or keeps the value it already has, so a
  plugin that calls another stays named.
- A run without `BOARD_PLUGIN` is the owner's. Its puts are not checked.
- `board call` also passes `XDG_CONFIG_HOME`, so a nested run finds the
  same known boards file and personal board.
- `put` and `decide` read the known boards file. Under a plugin, a file
  that cannot be read or a personal board that is not a board refuses them
  with `board::config::invalid` or the open error.
- A `BOARD_PLUGIN` that is empty, not UTF-8 or not a plugin's name refuses
  the run with `board::config::invalid`.

## Records

Each is a Board type, checked by its Rust type, with no schema file.

- `board/preference/v1`: a rule.
  - `op` is `put`, `rm` or `<plugin>.<capability>`.
  - `path` is a pattern that starts with `/`. `*` matches any run of
    characters, `/` included, and `?` matches one. Matching folds ASCII case.
    A pattern with an empty, `.` or `..` segment, or a trailing `/`, is
    not a preference.
  - `authority` is `act`, `propose`, `ask` or `refuse`.
  - `plugin`, when set, limits the rule to one plugin. `board`, when set,
    limits it to the board with that root directory, compared after both
    resolve symlinks.
  - `tell` is `silent`, `summary` or `now`.
- `board/effect/v1`: a pending effect at `/effects/pending/<id>` in the
  personal board. It names the board root, plugin, op, path and authority.
  A put's effect also holds the `record` or the file's `text`, its `digest`,
  and `prior`, the blob the put would replace.
- `board/seen/v1`: the owner's answer at `/effects/seen/<id>`: `approve` or
  `refuse`, with an optional `reason`. An approval holds the owner's
  `signature` (see Signed approvals).
- The id is the 64 hex digits of the SHA-256 of the canonical JSON of the
  board root, plugin, op, path, digest and prior blob. The same put over the
  same file asks with the same id.

## Rules and resolution

- The owner layer is every preference under `/preferences/` in the personal
  board. The board layer is every preference under `.board/settings/` in the
  board where the put lands. A put through a mount meets the mounted
  board's settings, and the root's settings do not apply to it.
- Both layers are read from the working tree, once per put or batch.
- A `.json` file in either layer that is not a valid preference, including
  one of another `$type`, is no rule. A plugin's put and decide are then
  refused with `board::settings::bad_rule`, which names the file. `check`
  reports the file degraded, and the owner's puts are not checked.
- A rule matches the landed path: the path inside the board where the put
  lands, after links and mounts resolve, without `.json` for a record. Its
  existing directories are spelled as the file system spells them.
- The narrowest authority among the matching rules wins, in the order act,
  propose, ask, refuse. A rule therefore narrows another and never widens
  it. Of equal rules the owner's comes first.
- With no matching rule, `put` and `rm` act and any other op asks.
- `rm` is checked as a put is, at the file it removes. Its effect holds
  `prior` and no content. A plugin's removal of a link or mount record is
  refused, as its put of one is.

## A plugin's put

- The check runs after the record is validated and its target is known. An
  invalid record is refused as invalid and writes no pending effect.
- These are refused with `board::settings::from_plugin`, whatever the record
  type, compared with ASCII case folded:
  - a put whose path is not ASCII, because a case-insensitive file system
    folds characters such as `ſ` into ASCII letters;
  - a put whose landed file is under `.board/` in any board, except a
    schema a batch line adds under `.board/types/`. That line is refused,
    as `type add` refuses, if it changes a schema, and meets the rules at
    its file's path;
  - a put under `preferences/` or `effects/` in any board;
  - a put of a `board/preference/v1`, `board/effect/v1` or `board/seen/v1`
    record in any board, because a clone of the personal board could carry
    it there;
  - a put of a `board/mount/v1` record.
- Rules live in files a plugin could otherwise overwrite, and an
  overwritten rule stops applying. These refusals are the whole defence of
  the rules.
- Under a plugin, `init`, `plugin enable` and `type add` are refused with
  `from_plugin`, because they write without a put.
- `act` writes. `refuse` is refused with `board::settings::refused`.
- `ask` and `propose` read the owner's answer to the put's effect. Approve
  writes and refuse is `refused`, with the id in the `effect` field.
- With no answer, `ask` writes the pending effect and is refused with
  `board::settings::asked`, whose `effect` field is the id.
- With no answer, `propose` is refused with `board::settings::propose_only`.
  With `--or-propose` it writes the pending effect and reports its id as
  `proposed`.
- An ask or propose whose effect cannot hold the content, a file that is
  not UTF-8 or an effect over the record size limit, is refused with
  `board::settings::cannot_hold`.
- A file put with a store is checked before the bytes are uploaded, by the
  mounted board when the path leads through a mount.
- A batch is refused with `board::put::invalid` when any line is refused;
  each refused line carries its own code, and the pending effects of the
  lines that ask are written. `--or-propose` does not combine with `--batch`.
- Pending effects are written after the board's lock is released, so a
  plugin's put into the personal board can hold its effect there.
- An ask or propose with no personal board named is refused with
  `board::settings::no_personal`.

## Commands

- `board decide --op <op> --path <path>` prints the authority, the
  matching rules and the deciding one, for the plugin `BOARD_PLUGIN` names or
  for the owner.
  - It resolves the path as a put does, and through a mount decides with
    the mounted board's settings.
  - It lists 20 rules and the deciding one, and notes those left out and
    when no personal board was read.
  - For `put` and `rm`, it reports the rules' authority and writes nothing,
    because their effect holds the record or the prior blob.
  - For another op under a plugin, the owner's answer replaces the
    authority, and an ask with no answer writes a pending effect.
- `board answer <id> approve|refuse [--reason <text>] [--save-rule]` writes
  the seen record and removes the pending effect, in one commit.
  - The records under `/effects/pending/` are the open effects. Git history
    keeps an answered one.
  - A repeated put or decide finds the seen record by the id and writes no
    new pending effect. A put with other content or over another blob has
    another id and asks again.
  - Answering the same id again is refused with `not_found`, whose hint
    says the effect was answered or never asked. A stale client therefore
    cannot turn a refusal into an approval. The owner changes an answer by
    deleting its seen record, or editing it with a new signature.
  - The seen record is written only where none is, and the pending effect
    is removed only if it is unchanged. Of two answers at once, the second
    is refused with `board::put::stale`, as is an answer to a pending
    effect whose seen record exists.
  - A put holds its pending effect after it releases the lock, and writes
    none if the owner answered in between.
  - `--save-rule` also writes `/preferences/<id>` for the effect's op,
    exact path, plugin and board, in the same commit. A path holding `*`,
    `?` or `[` is refused with `board::settings::bad_pattern`.
  - The owner answers only approve or refuse. There is no answer that
    turns an ask into a proposal.
  - Approving a put or rm with `--save-rule` is refused with
    `board::settings::cannot_widen`. They ask only where a rule says so,
    and a saved `act` cannot widen that rule.
  - A plugin may not answer (`from_plugin`).
  - An id that is not 64 lowercase hex digits, names no pending effect, or
    names one whose content does not hash to the id and digest is refused
    with `board::settings::not_found`.

## Signed approvals

Every approval, with or without `--save-rule`, carries the owner's Touch ID
signature. An agent's shell has no `BOARD_PLUGIN`, so it counts as the owner
and could otherwise approve its own effects.

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
  binds the content and prior blob. It shows the plugin, op, path and
  board, a line when the approval also saves a rule that acts, and the
  content's size in bytes and lines followed by the content split at each
  `\n`. Hidden characters, `\r` among them, are escaped as
  `board_model::escaped` does.
- The seen record holds the path of the rule it saved in `rule`, so a later
  check rebuilds the same statement. Every stored signature is checked
  against the rebuilt text, so its layout is frozen: a change needs a new
  first line.
- The signature is an SSH signature in the namespace the statement's
  first line names: `board/approve/v1` for an approval, `board/write/v1`
  for an owner's put.
  - `attention-sign approve` signs only a statement whose first line is
    one of the two.
  - A directive is signed in `attention/directive/v1`.
  - No signature verifies in another namespace, so a key limited to
    approvals cannot change a rule.
- `/usr/bin/ssh-keygen -Y verify`, at a fixed path so `PATH` cannot supply
  another verifier, checks it against `.config/board/allowed_signers` in
  the same passwd home for the principal `owner`. The check runs before
  the answer is written and whenever a put or decide reads an approval.
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

### The owner's own puts to rules and effects

`board put`, `put --file` and `put --batch` without `BOARD_PLUGIN` need the
owner's signature for a file under `.board/settings/`, `preferences/` or
`effects/`. The check applies in every board, because the known boards file
that names the personal board is the caller's to choose. It matches the
working-tree path in any ASCII case. A first directory that is not ASCII,
or a second one under `.board/`, also needs a signature, because a file
system may fold it to one of those names. The owner's `rm` of
`.board/board.json` also needs a signature, because the board does not open
without it. A put of it does not.

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
- A plugin's put under `preferences/` or `effects/` is refused in every
  board for the same reason, as one under `.board/` always was.

## Limits

- A plugin that unsets `BOARD_PLUGIN` escapes the check. The check catches
  mistakes by a plugin that keeps it set.
- Paths are not NFC normalised, so a rule and a path spelled with other
  Unicode forms do not match.
- An approval of an op other than put names no content, so it admits that
  op at that path again until the owner removes the seen record.
- Only `board decide` checks an op other than put. `board call` runs a
  plugin, which must ask with `decide` itself.
- A plugin can set `BOARD_PLUGIN` to another installed plugin's name.
- A plugin sets `XDG_CONFIG_HOME` for the Board it runs, so it picks the
  known boards file and so the personal board whose rules apply. A file
  that names no personal board drops the owner's rules.
- An approval is keyed on the content and the blob it replaces. If the
  file returns to that blob, the plugin can write the approved content
  again without asking.
- The root board's rules do not apply to a path inside a mount. Only the
  mounted board's settings and the owner's rules do.
- A plugin may run `sync` and `mount update`, which the notes plugin
  needs. They push commits and move a pin without asking.
- A preference with `board` set stops applying from another clone or after
  the board moves.
- A seen record over the record size limit is refused as
  `board::json::too_large`.
- Seen records stay in the personal board after use.
- The signatures stop an agent that runs as the owner from approving its
  own effect with a plain `board answer`, or from changing a rule with a
  plain `board put`. They do not stop it from:
  - writing the rule's file and committing it with git, outside Board;
  - replacing `~/.local/bin/attention-sign` or editing
    `~/.config/board/allowed_signers`, which the account may write;
  - asking the owner to sign a statement it wrote, in a window that looks
    like a real approval.

  Only the sandbox the agent runs in stops those.
