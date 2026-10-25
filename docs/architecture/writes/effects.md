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
  - `op` is `put` or `<plugin>.<capability>`.
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
  `refuse`, with an optional `reason`.
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
- With no matching rule, `put` acts and any other op asks.

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
  - a put under `preferences/` or `effects/` in the personal board;
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
  - For `put`, it reports the rules' authority and writes nothing, because a
    put's effect holds its record.
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
    editing or deleting its seen record.
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
  - Approving a put with `--save-rule` is refused with
    `board::settings::cannot_widen`. A put asks only where a rule says so,
    and a saved `act` cannot widen that rule.
  - A plugin may not answer (`from_plugin`).
  - An id that is not 64 lowercase hex digits, names no pending effect, or
    names one whose content does not hash to the id and digest is refused
    with `board::settings::not_found`.

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
