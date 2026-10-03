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

### M3 Records and validation: done

`board-validate`, `board-ops`, `board-cli` and the strict parse in
`board-model`: kinds, schemas, typed references, `check`, `get` and `ls`
(#17, #18, #20, #22). The first full fuzz run, after #23 and #24, passed.

### M4 Index and query: done

`board-index`: the tables, refresh by tree diff, the working-tree overlay,
mounts, and the commands `query` and `refs` (#27, #29 to #33). Budgets are
measured and set in docs/architecture.md. Moved on: timing the M1 export,
which the owner runs locally with `cargo xtask bench --board` once the export
exists.

### M5 Writes: done

Canonical form with golden bytes and the lossless number check, `put` with
`--expect`, the lock, optional commit and `--batch`, and `mount update` (#35 to
#40). Each done item has its test: one write and one `put::stale` from two
concurrent puts, a killed put leaving the old or new file, an invalid line
refusing a batch, and writes through a link and a mount.

### M6 Plugins: done

`board-plugin`, `plugin list` and `find`, `call` with its limits and codes,
extraction with `query` attaching each plugin's database, and the `tasks`
plugin in `plugins/tasks/` with its Python gates in CI (#42 to #46). Each done
item has its test: `tasks` against fixture boards through the real `board`,
`query` joining `p_tasks.tasks` with `entries`, the `digest` fixture calling
`due` by capability, and every `board::plugin::` code. No Python library yet:
there is one plugin.

### M7 Switch

The owner uses the new Board for daily work and archives the old one.

Done: the owner says so.

### M8 Sharing (sketch; after M7)

A shared project is its own repository, mounted into each person's board. This
milestone lets a `put` write through a mount and keeps every mounted copy in
step. The owner's decisions so far:

- **Board owns its copies.** A mount names a remote; Board keeps a clone of
  each under `$XDG_DATA_HOME/board/repos/`, with no checkout to fall out of
  step. `board sync` fetches every clone, pushes pending commits and moves
  each pin, so keeping up to date is one command a scheduler can run. Reads
  never touch the network.
- **A write through a mount** commits in the mounted clone, moves its branch
  only if it still holds the pinned commit, then moves the pin with an
  ordinary `put`. A kill between the two leaves only a stale pin, which
  `mount update` repairs. A pin behind its branch refuses the write with
  `put::stale`.
- **Push after write.** A write pushes its commits; one that cannot leaves
  them for the next `sync` and says so. Mounted repositories push innermost
  first, so no pushed pin names a commit others cannot fetch.
- **Conflicts.** When the remote moved, Board replays its own commits on top
  only if they touch none of the same files; any overlap stops for the owner
  to resolve in git. Board never merges file contents.
- **Batches** write to one repository each.
- **Nesting** is the same step applied recursively, bottom up: each level
  commits, then the level above moves its pin.

- **Transport runs the `git` executable** (owner's decision), so credentials,
  SSH and hosts work as they already do; gix keeps reading objects. The
  `git` child process gets one declared owner in `board-tree`.

- **Remote mounts change `board/mount/v1` in place** (owner's decision):
  Board is still being built and has no data to migrate, so `repo` gains its
  remote meaning in v1 with no new version.
- **Network safety.** A mount that names a remote lets its author choose
  what the owner's machine fetches, with the owner's credentials. So Board
  fetches only remotes the root board names in one of its own mount pointers.
  To let a mounted board's nested remote be fetched, the owner mounts that
  remote somewhere in the root board; until then it is listed, not fetched.
  Approval is the tree, with no state beside it. The `git` child allows only
  https and ssh (`-c protocol.allow=never -c protocol.https.allow=always
  -c protocol.ssh.allow=always`), so `ext::` and `file://` are never
  reachable from data. Every remote URL is passed after `--`, and a URL that
  starts with `-` is refused, so data can never become a `git` option.

Open:
- Whether jujutsu, with its first-class conflicts, sits behind the same seam
  later. Decide after M7 shows how sharing is actually used.

### Views (sketch; designed for, not built)

Beside M8. Land the design before a second plugin exists, because the second
plugin starts the Python library and views belong in it.

- Every record type gets a detail view and a form derived from its schema,
  with no plugin work.
- A plugin declares views as data in a new `board/plugin-manifest/v2`: a small
  component vocabulary and field bindings, so listing a thousand records spawns
  no process.
- A sandboxed component is a later escape hatch for charts and editors.
- Edits go through `put --expect`, which gives a form safe concurrent saves.
- Views from a mounted board are data, never code.
- Uploads are files in the tree; large binaries wait for a real one.
- Acceptance, set by the owner: the UI can express widgets's UI. The component
  vocabulary is derived from it, which needs read access to example-org/widgets.

### Serving (sketch)

`board serve` is the one long-lived process allowed. It may keep the index
warm for an agent loop or a web view; no answer may depend on it, and every
answer equals a cold run's. The measured warm-check speedups (HANDOFF item 2)
land after M7 gives real numbers.

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
8. A web UI and sharing are designed for, not built in v0. Their sketches are
   M8, Views and Serving above.

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
