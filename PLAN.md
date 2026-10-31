# Board: plan

This file holds what comes next and the decisions that still constrain work.
docs/architecture.md and docs/plugins.md describe what is built, and
STANDARDS.md holds the code standards. A milestone whose design does not fit
here keeps it in one flat leaf, `plan/<milestone>.md`, linked once below. When
a milestone lands, its design moves into docs/ and leaves this file and its
leaf. `lint.toml` limits the length of this file and of each leaf.

The principles are the rules in AGENTS.md, plus one: a record's path is its
identity, and its type is the `$type` field inside it.

## Next: M7, the switch to daily use

The owner uses the new Board for daily work and archives the old one. The
steps left are the owner's:

1. Time the board in daily use with `cargo xtask bench --board ~/Code/board`.
   `cargo xtask bench` fails above three times a budget in
   docs/architecture/performance/budgets.md. The targets there are speeds to
   aim for, and nothing enforces them.
2. Enable `tasks` and use Board daily.
3. Route what daily use finds to builders as small fixes.

M7 is done when the owner says so.

The index keeps each mounted source's decision
([docs](docs/architecture/performance/decisions.md)). On a copy of the
owner's board with 24 mounts, a warm `call plans.open --limit 20` takes
about 47 ms, down from 96 ms, which meets the 50 ms target.

Two smaller items each get their own pull request. A scoped check walks
every mount; fixing that needs a design. Compiling manifest schemas once
per run saves `call` 3 to 4 ms.

## Then: runs

Board records test, perf and eval results. A new `runs` plugin holds suites
and their runs, `evals` and `controls` refer to it, `cargo xtask bench
--record` writes runs, and a `github` plugin imports CI timings across
repositories. [plan/runs.md](plan/runs.md) holds the decisions and the build
order.

## Then: board-ui and the effect gate

Agents show records through a component catalogue that `board serve` serves,
and plugins ask Board before they take an effect. The owner answers from the
command line or a gate window. No answer depends on `board serve`.
[plan/board-ui.md](plan/board-ui.md) holds the decisions and the build order.
The `review` plugin follows, designed in [plan/review.md](plan/review.md),
which brings review accounting into scope.

## Then: work records

Every change, decision and artifact is recorded as small records written
once, and each one's state is derived from which records exist. Build
[repos](plan/repos.md) first; then [decisions](plan/decisions.md),
[control scope](plan/control-scope.md) and [artifacts](plan/artifacts.md),
which are independent; then [work records](plan/work-records.md); then
[inflight](plan/inflight.md).

## Out of scope

Each of these waits until a real need has its own plan: effects beyond the
first slice in plan/board-ui.md; triggers and scheduling;
pinned or relative references; remote mounts (a mount names a local clone);
any query language beyond SQL; daemons and watchers other than `board serve`;
sync beyond git; Windows CI (the code
stays portable); and the old Board's ControlTree groups, assessments,
paged reports and control status.

## Deferred

These sketches are not built. Their full text is in PLAN.md at commit 226829b
on main (`git show 226829b:PLAN.md`), as is the deleted HANDOFF.md.

- **M8 Sharing.** A put of one record or a batch through a local mount and
  `board sync` of the root board are built (docs/architecture/writes.md). The owner decided that transport runs
  the `git` executable, that remote mounts change `board/mount/v1` in place,
  that Board fetches only remotes the root board names, and that Board detects
  a colocated jj repository automatically with no setting. After M7, turn the
  sketch into a list of design gaps for a builder.
- **Repositories across boards.** References into another board's
  repository read `name:/path`; plan/repos.md builds the declaration. The
  owner agreed it on 2026-10-05; it refines M8, and #55's content-addressed
  records are dropped. The `share` plugin, built in place of the `board copy`
  and `board share` it named, copies a record and what it needs into a
  mounted board (docs/plugins/share.md).
- **M9 SHA-256 repositories.** Board reads and writes repositories in git's
  SHA-256 object format. Its standing rule is settled decision 5 below.
- **Views from schemas.** Every record type gets a view and a form derived
  from its schema. The owner's acceptance test is that views can express the
  UI of an existing tool the owner will name.
- **Serving beyond the UI.** `board serve` keeps the index warm for an agent
  loop, and a `board mcp` adapter serves each capability as an MCP tool.
- **Adoption.** Board imports records from other systems, such as JIRA,
  through plugins, and a reconciler plugin settles the difference between the
  source and Board's copy. The owner agreed it on 2026-10-05; its first step is
  a spike that imports one existing store into a scratch board.
- **Owner rules for effects.** Rules that make a plugin's put ask or
  refuse, cut until two plugins need them (`git show
  5a34ac92:docs/architecture/writes/effects.md`), then offers, attention
  levels, read gating, learning from corrections and the mandate layer.
- **Docs as a tree.** The per-directory word budget is built (STANDARDS.md 22),
  and docs/architecture.md has split into docs/architecture/. Still open are
  how the review of each docs change runs and a tool check that each leaf is
  linked once.

## Settled decisions

1. The name is `board`: the repository (crumbkit/board), the binary and its
   help, the `board-*` crates, `board::` codes, record types, plugin ids,
   config and the reserved `board` namespace (2026-10-30).
2. The code is proprietary and unpublished.
3. A limit that refuses a write, such as a file-size limit, is a setting that
   `put` and `check` apply. A requirement on files is a control (2026-10-05).
   A control that code already enforces only records it: `lint.toml` sets the
   word budgets, `cargo xtask lint` enforces them, and it fails when a control
   in `controls/budgets/` disagrees with `lint.toml` (2026-10-06).
4. Adopting a repository is gentle: after the owner follows what `check` says,
   `check` exits 0. A test fixture carries only a type Board or one of its
   tools supports (2026-10-05).
5. New code never assumes SHA-1. It asks the repository for its object format,
   as `write.rs` and `worktree.rs` already do when they hash (2026-10-05).

## Risks

- **Line-based merges of JSON.** One key per line makes most merges clean. A
  conflict leaves markers, which `check` names as
  `board::json::merge_conflict`. A merge driver can come later as a plugin.
- **Growth.** The budgets, the file and function caps, the `#[expect]` ratchet,
  STANDARDS.md 13 and AGENTS.md rule 1 exist because the old Board grew past
  what anyone could read. A change that raises a limit says why.
