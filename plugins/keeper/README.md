# keeper

The keeper plugin keeps a project tree of the owner's open work and writes it
as a status board page. SKILL.md says how an agent runs a pass.

## Capabilities

- `keeper.gather` reads the open changes, open tasks, waiting questions and
  stale work, and the latest map. It writes nothing.
- `keeper.map` checks a project tree against the open changes and writes it
  as `keeper/map/v1` at `/projects/keeper/map/<timestamp>` and at
  `/projects/keeper/map/latest` in one commit. It refuses a tree that leaves
  an open change out, holds one twice or names a path that does not exist.
- `keeper.render` writes one self-contained HTML page, by default at
  `/projects/keeper/work-view/index.html`, from the latest map and the live
  records. The page fetches nothing and needs no server.

## The page

Rust settles every fact and sorts every list, and the page's script builds
the views from three JSON blocks in the file. The page gives the same bytes
for the same facts, which `crates/board-plugin-keeper/src/page/tests.rs`
checks by reversing every input list and by comparing with a golden file.
`crates/board-plugin-keeper/page/check.mjs` renders every route of the golden
page in Node and checks that each realm's total is its rows plus its
questions tied to no project. `bin/ui-goldens` runs it.

The page sources are in `crates/board-plugin-keeper/page/`. After changing
them, run the keeper tests with `SNAPSHOTS=overwrite` to rewrite
`page/fixture/page.html`, and review the difference.

## Records

`keeper/map/v1` holds the date, the realms, the projects with their changes
and tasks, the findings, the links between projects, and optionally the
systems and the question rules. Its schema is
`crates/board-plugin-keeper/types/`.

## Limits

keeper.render reads Cargo workspaces only. It counts lines and public item
names in the mounted files and cannot read git history, so the page shows no
churn.
