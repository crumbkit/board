# Board next: project plan

Proposed 2026-10-03, in crumbkit/board; moved to crumbkit/viitta the same
day. Nothing in it is installed yet.

## Why start again

The current Board is about 285,000 lines of Rust, 1,500 core tests, 103,000
words of documentation and 76,000 words of agent instructions, serving one
owner's 8,000 records. Its store is already git, but used as a block device:
records are JSON blobs named by hash, so nothing is legible without `board`,
every write rewrites 15 trees, and activating a new binary reads every object
(about four minutes). On 2026-09-15 the owner asked for "a way to enforce
types, and store them … in like a day"; that became a 6,181-word plan with
four revisions and nine reviews.

The cause is the central abstraction, not the code quality. Board rebuilt git's
job (immutable history, content addressing, parents, merging) one level down,
then put governance (standing, authority, freshness, settlement) into the
kernel's invariants. Refactoring cannot replace a central abstraction, so we
start again and take only what survives the new model.

## What Board becomes

A fast, validated view of a git tree. A board is a directory of files under git.
Board indexes every file, validates the JSON records it understands, reports
everything else honestly (at worst a path and an error code), resolves typed
references through links and mounts, and gives agents an easy way to read and
write records. Plugins add higher-level constructs as separate programs with
their own disposable SQLite. Git does history, branching, merging and
transport; a board on a branch is a branch, and joining two boards is a merge.

docs/architecture.md and docs/plugins.md hold the design. STANDARDS.md holds
the code standards and the gates that enforce them.

## Principles

1. The tree is the truth. Indexes and caches are disposable, and every query
   answers the same from a cold rebuild as from a warm index.
2. Everything derived is keyed by content (blob or tree hash), never by commit,
   time or arrival.
3. Validate on the way in, report on the way out: `put` refuses an invalid
   record; `check` and the index never refuse a file.
4. Paths are identity. A record's type lives in the record.
5. One resolver: only `board-tree` knows that links and mounts exist.
6. Mounting brings data and schemas, never code.
7. Copy existing semantics (git, POSIX symlinks, JSON Schema, SQL) rather than
   inventing them. No query language beyond SQL.
8. Nothing enters the core until two plugins need it.

## Scope of v0

In: `check`, `get`, `ls`, `refs`, `put` (single and batch), `mount update`,
`query`, `plugin list` and `find`, `call`, plugin extraction, macOS and Linux.

Out, each until a real need has its own plan: authority and external effects;
triggers and scheduling; review accounting; writes through mounts; pinned or
relative references; remote mounts (a mount names a local clone); any query
language beyond SQL; daemons and watchers; Markdown as a record format
(Markdown is a file a record points at); sync beyond git; Windows CI (the code
stays portable).

## Milestones

Every milestone ends with `bin/lint` and `bin/test` green on macOS and Linux
and its done evidence in the merged change. Each lands as small pull requests;
the list is an order, not a batch.

### M0 Seed

Workspace, standards, gates, the `xtask` budget lint, CI, this plan and the
design documents.

Done: `bin/lint` and `bin/test` pass with `xtask` as the only member, and CI
runs them in this repository.

### M1 Export the old corpus

A throwaway script, run by the owner against the installed old `board`, that
writes every current object head as a canonical JSON record:
`work:board/core/x` becomes the file `work/board/core/x.json` (logical path
`/work/board/core/x`) with `$type` `legacy/work/v1`, and each old type gets a permissive schema
(`{"type": "object"}`) to tighten later. History is not carried; the old store
stays as a read-only archive. M1 can run alongside M2.

Done: the export, committed in its own repository; a record count equal to
`board objects list`; a list of objects it could not map. Working in the export
with `rg` and agents during M2 and M3 is free evidence about what the core
really needs.

### M2 Paths and resolution: done

`board-model` and `board-tree`: logical paths, Locations, codes, commits and the
working tree, resolution, walking, links and mounts (#3, #7, #9, #12 to #14).
Moved on: the display-path round trip to M3, where `get` first prints one;
walks through mounts to M4; the two write scenarios to M5.

### M3 Records and validation

The duplicate-refusing parse, `$type`, schemas from the tree, typed references,
canonical output, `board-validate`, and the commands `check`, `get` and `ls`.

Done: a fixture board that produces every code in the code table; golden bytes
for canonical output; bolero targets for the record and path parsers, run in
the gate and fuzzed by a scheduled job; walks mark dangling references; the
property that resolving a Location's display path returns that Location.

### M4 Index and query

`board-index`: the tables, refresh by tree diff, the working-tree overlay, and
the commands `query` and `refs`.

Done: walks and the index descend mounts, each mounted commit indexed once;
the warm-equals-cold property for every query; "one changed record
parses one blob"; the timing job on the M1 export and a generated
10,000-record board, with the budgets in docs/architecture.md replaced by
measured ones.

### M5 Writes

`put` with compare-and-swap, canonical output, the lock, optional commit and
batches; `mount update`.

Done: two concurrent puts with the same `--expect` produce exactly one write
and one `board::put::stale`; a kill at any point leaves the old file or the new
one, never a partial file; a batch with one invalid record writes nothing; a
write through a link lands at its target and one through a mount is refused.

### M6 Plugins

`board-plugin` and one real plugin end to end (suggested: `tasks`, extracting
status and due date from task records, with a `due` capability). Plugins are
written in Python directly against the JSON contract, each keeping its protocol
code in one module. A Python library is extracted from the plugins once two
exist and their shared code is visible, not designed ahead of them.

Done: the plugin's tests pass against a real fixture board; `board query` joins
its table with `entries`; a second plugin calls the first by capability through
`board call`; every `board::plugin::` code has a test; CI runs the Python gates
(STANDARDS.md P2).

### M7 Switch

The owner uses the new Board for daily work and archives the old one.

Done: the owner says so.

## Taking from crumbkit/board

Take ideas and tests, not files. Each piece is rewritten under the new
standards and reviewed like new code.

- `crates/board-core/src/canonical.rs`: the canonical JSON rules, checked
  against serde_json's sorted output.
- `crates/board-core/src/objects.rs` (the ObjectSubject grammar): its byte and
  character limits, for logical path segments.
- `crates/board-core/src/json_schema.rs`: only if the `jsonschema` crate cannot
  meet the untrusted-schema rules in docs/architecture.md.
- The habits: every refusal paired with an admitted twin; deterministic
  work-count tests; golden vectors.

Leave everything else: activation, authority, review accounting, coordination,
triggers, skill projection, memory search, writer release, git transport and
the Python plugin experiments.

## Settled decisions

The owner settled these on 2026-10-03.

1. The name stays `board`: the binary, the `board-*` crates, `board::` codes and
   the reserved `board` namespace. `viitta` is only the repository's name.
2. The code is proprietary and unpublished.
3. The record tag is `"$type": "<namespace>/<kind>/v<n>"`.
4. `put` commits by default in a board's own repository, set by `commit_on_put`
   in `.board/board.json`.
5. Plugins are installed in `~/.local/share/board/plugins/<name>/`.
6. The budget values in `lint.toml` stand; `context.max_words` counts the
   default-loaded files together.
7. proptest states invariants over generated structures; bolero fuzzes parsers
   of untrusted bytes.
8. A web UI and sharing are designed for, not built in v0. The positions are in
   HANDOFF.md until they get a plan.

## Risks

- **gix coverage.** Reading trees, diffs and status is mature; writing commits
  is less used. If `put --commit` is awkward in gix, that is the one place to
  reconsider, and shelling out to `git` would need a reviewed child-process
  owner.
- **Untrusted schemas from mounts.** Regex blow-up and remote `$ref`; covered
  by the rules in docs/architecture.md and tested in M3.
- **Line-based merges of JSON.** One key per line makes most merges clean; a
  conflict leaves markers, which `check` names as `board::json::merge_conflict`.
  A merge driver can come later as a plugin.
- **Growth.** The budgets, the file and function caps, the `#[expect]` ratchet,
  rule 13 and principle 8 exist because the old Board grew past what anyone
  could read. Raising a limit is allowed; doing it silently is not.
