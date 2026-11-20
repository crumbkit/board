---
name: places
description: 'Join an mxv workspace on this machine to the Board change it works on, and list every machine''s joins with each change''s phase and the workspace''s status. Use it when a workspace starts work on a change. Run `board --personal places attach --change /changes/<name>` from inside the workspace. SKILL.md and README.md in the plugin''s directory say more.'
---

# places

## Join a workspace to its change

1. Run `board --personal places attach --change /changes/<name>` from inside
   the mxv workspace. From anywhere else, add `--workspace <name>`.
2. Running it again is safe. When the join is already there, `written` is
   false.
3. When `replaced` is true, an earlier workspace of the same name had the
   join, and the join now names this one.

## Remove a join

- Run `board --personal places detach --change /changes/<name> --workspace <name>`.
- `removed` is false when no join of that workspace to that change was there.
- A `places::detach::other_change` warning means the join belongs to the
  change it names. Detach that change instead if it is the one meant.

## List the joins

- Run `board --personal places list`. It lists every machine's joins. Each
  row has its `machine`, the change's title and phase, and the workspace's
  `status`.
- Add `--machine <label>` for one machine's joins. A label with no joins
  lists none.
- A join of another machine is `unknown`, because only that machine's mxv
  knows its workspaces.
- `live`: the workspace exists with the join's id.
- `replaced`: a workspace of that name exists with another id.
- `archived`: the workspace is archived with the join's id.
- `gone`: no workspace of that name exists, live or archived.
- `unknown`: mxv prints no id for the workspace of that name, or only
  archived workspaces of that name with another id exist.

## Codes

- `places::mxv::no_id`: the installed mxv is older than 0.4.0. Tell the
  person to upgrade mxv.
- `places::workspace::no_id`: mxv prints no id for the workspace. Tell the
  person the message, which says what to fix.
- `places::workspace::outside`: pass `--workspace <name>`, or run from inside
  the workspace.
- `places::workspace::not_found`: run `mxv list` and pass a listed name.
- `places::machine::unnamed`: ask the person to set `BOARD_MACHINE` or to
  write the machine's name to `~/.config/board/machine`.
- `places::attach::taken`: another change with the same last segment holds
  the join. Ask the person which change the workspace works on.
- Any other code: tell the person the message.
