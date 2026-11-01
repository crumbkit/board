---
name: artifacts
description: 'The claude.ai artifacts the owner publishes, each with a copy of every version''s page, recorded by the board-artifacts Claude Code mod. Use it to find an artifact, or to sync the artifacts published outside Claude Code. Start with `board --personal call artifacts.list`, which lists the artifacts with their newest version, newest first. SKILL.md and README.md in the plugin''s directory say more.'
---

# artifacts

Find the owner's claude.ai artifacts in the personal board, and bring in the
ones published outside Claude Code.

The board-artifacts mod records every artifact this session publishes or
reads. You never write these records yourself.

## Find an artifact

    board --personal call artifacts.list

It lists each artifact's link, account and newest version with its title,
newest first. `--account <email>` keeps one account's artifacts, and
`--limit <n>` returns up to 200 (50 by default). The newest version's
`path` names its record; the page is beside it as `<path>.html` or
`<path>.md`, which `board --personal get` reads.

## artifacts sync

Run this step when the owner asks to sync artifacts, or to backfill an
account the owner has just signed in to.

1. Run the Artifact tool's `list` action with `limit` 200 and no `scope`
   or `type`. The mod passes the listing to `artifacts.import`, which
   writes an artifact record for each link the board does not have. The
   mod then tells you which artifacts are new.
2. Read each new artifact with the Artifact tool's `read` action and its
   `url`. The mod records the version that comes back. You need not read
   the page yourself.

Tell the owner how many artifacts were new, and pass on what the mod says
about a truncated listing.

## Limits

- The `list` action returns at most 200 artifacts, most recently updated
  first, and has no way to ask for more. A regular sync still catches every
  artifact updated since the last one, because those sort first. A backfill
  of an account with more than 200 artifacts records only the 200 most
  recent, and the mod says how many the account has.
- A `read` returns only an artifact's current version. Versions published
  before the mod was enabled are not copied, and the first version record
  of an imported artifact is its version at the import.
