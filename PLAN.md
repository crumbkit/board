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

M7 ends when the owner says so. Compiling manifest schemas once per run
would save `call` 3 to 4 ms.

## Next: the map of facts, rules and work

A session sees a view of its node chosen for its task, and the owner sees one
daily view of what is due, failing and waiting on him. "Control" becomes
"rule", and rules record their state. [plan/map.md](plan/map.md) holds the
design.

## Then: rules with a second shape

Rules gain subjects, triggers, checkers and failure actions, and a rule's
`on_failure` alone sets how strict Board is. Every board migrates in place,
and 0.1.0 is tagged when every kind is built. Flow checks stay in flows until
a later plan moves them.
[plan/rules.md](plan/rules.md) holds the design.

## Then: runs

Board records test, perf and eval results. A new `runs` plugin holds suites
and their runs, `evals` and `rules` refer to it, `cargo xtask bench
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
once, and each one's state is derived from which records exist. Build them in
this order.

1. [Decisions](plan/decisions.md).

2. [Work records](plan/work-records.md), then [inflight](plan/inflight.md).

## Out of scope

Each of these waits until a real need has its own plan: effects beyond the
first slice in plan/board-ui.md; a scheduler inside Board;
pinned or relative references (a review's `subject` pins only its own
field); remote mounts (a mount names a local clone);
any query language beyond SQL; daemons and watchers other than `board serve`;
sync beyond git; Windows CI; and the old Board's ControlTree groups, assessments
and paged reports.

## Deferred

These sketches are not built. Their full text is in PLAN.md at commit 226829b
on main (`git show 226829b:PLAN.md`), as is the deleted HANDOFF.md.

- **M8 Sharing.** The owner decided that transport runs
  the `git` executable, that remote mounts change `board/mount/v1` in place,
  that Board fetches only remotes the root board names, and that Board detects
  a colocated jj repository automatically with no setting. After M7, turn the
  sketch into design gaps for a builder.
- **Repositories across boards.** References into another board's
  repository read `name:/path`; repo records are the declaration
  (docs/architecture/paths/repos.md). The
  owner agreed it on 2026-10-05.
- **M9 SHA-256 repositories.** Board reads and writes repositories in git's
  SHA-256 object format. Its standing rule is settled decision 5 below.
- **Views from schemas.** Every record type gets a view and a form derived
  from its schema. Its acceptance test: views express the UI of
  an existing tool the owner names.
- **Serving beyond the UI.** `board serve` keeps the index warm for an agent
  loop, and a `board mcp` adapter serves each capability as an MCP tool.
- **Adoption.** Board imports records from other systems, such as JIRA,
  through plugins, and a reconciler plugin settles the difference between the
  source and Board's copy. The owner agreed it on 2026-10-05; its first step is
  a spike importing one existing store into a scratch board.
- **Later effects.** Offers, attention levels, read gating, learning from
  corrections and the mandate layer. A policy that makes a put ask or refuse
  is a rule (plan/rules.md). Design:
  `git show 5a34ac92:docs/architecture/writes/effects.md`.
- **A repository's own rules.** `rules.check` reads a mounted repository's
  own rules as data at its pinned commit, checks the caller's repository
  without a path, and takes a branch's files on stdin with `--limit`. Full
  text: `git show c48b2856:plan/rules.md`.
- **Docs as a tree.** Still open are how the review of each docs change runs
  and a tool check that each docs leaf is linked once.

## Settled decisions

1. The name is `board`: the repository (crumbkit/board), the binary and its
   help, the `board-*` crates, `board::` codes, record types, plugin ids,
   config and the reserved `board` namespace (2026-10-30).
2. The code is proprietary and unpublished.
3. A limit that refuses a write, such as a file-size limit, is a setting that
   `put` and `check` apply. A requirement on files is a rule (2026-10-05).
   A rule that code already enforces only records it: `lint.toml` sets the
   word budgets, `cargo xtask lint` enforces them, and it fails when a rule
   in `rules/budgets/` disagrees with `lint.toml` (2026-10-06).
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
