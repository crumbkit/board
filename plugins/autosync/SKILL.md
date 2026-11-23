---
name: autosync
description: 'Keep each board the owner listed in step with its remote on an interval, pushing only commits a gitleaks scan passed. Use it to see why a board stopped syncing, with `board --personal autosync status`. Only the owner adds a board, with `board --personal autosync add`, which asks for Touch ID. SKILL.md and README.md in the plugin''s directory say more.'
---

# autosync

## See why a board stopped syncing

1. Run `board --personal autosync status`. Each board has its last
   `outcome`, `code` and `message`.
2. Tell the owner the message. It names what to fix. Do not fix a refused
   board yourself: each refusal waits for the owner.
3. A failing board also has an open task titled `Board sync stopped for the
   <name> board` in the personal board. It closes itself after the next
   clean sync.

## Add a board

- Only the owner adds a board. `board --personal autosync add --name <name>
  --path <directory> --origin '<url> <url>'` shows the owner the record and asks for
  Touch ID. Never write an `autosync/board/v1` record with `board put`:
  autosync refuses a record without the owner's signature.

## Codes

- `autosync::board::unsigned`: the record has no good signature. The owner
  runs `autosync add` again.
- `autosync::origin::not_allowed`: the board would push to a URL its record
  does not list. Tell the owner the URL.
- `autosync::board::nested`, `autosync::board::missing`: the record's path
  is not a checkout of its own. Tell the owner.
- `autosync::scan::found`: the scan flagged commits. Tell the owner the
  rule, file and line from `status`. Never print the secret.
- `autosync::scan::board_config`: the board holds `.gitleaks.toml` or
  `.gitleaksignore`. Tell the owner the hint.
- `autosync::scan::scanner_missing`: gitleaks 8.19.0 or later is not on the
  agent's PATH.
- `board::sync::*`: the guarded sync refused. Tell the owner the message.
- Any other code: tell the owner the message.
