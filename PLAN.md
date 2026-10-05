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

### Controls (next; agreed with the owner 2026-10-05, not built)

The owner wants to adopt a code repository as a board and attach controls to
its files, with the controls stored in that repository. viitta is the first.
A control is a requirement that applies to files, with its purpose and,
optionally, a rule a program can check, such as a word budget.

**What works today.** A trial on a scratch clone of viitta found:

- `board check` indexes all 176 files. Three are degraded with
  `record::unknown_type`: the export golden record (`legacy/work/v1`) and the
  canonical-form fixture `nesting.in.json`/`.out.json` (`lr/task/v1`). The
  three `plugin.json` files check clean.
- A control can already be a plain record. A schema field of
  `board-ref` strings, without `x-board-type`, may reference any file.
  `check` reports `ref::dangling` when a referenced file is renamed or
  deleted, and `board refs /AGENTS.md` lists the controls that reach it.

**Owner decisions.**

1. Built: `board get <path>` prints any file's stored bytes
   (docs/architecture.md, Commands).
2. Board teaches types instead of hiding files. Adopting a repository must be
   gentle: after the owner follows what `check` says, `check` exits 0. Board
   knows its own `board/plugin-manifest/v1`, checked by Rust types as it
   checks `board/config/v1`. `record::unknown_type` names the schema file that
   would teach the type. A test fixture carries only a type Board or one of
   its tools supports:
   - The canonical-form fixture `nesting.in.json`/`.out.json` does not need a
     record type: canonical form applies to any JSON. Its `$type` key becomes
     `$id`, which still tests that `$` sorts before letters, so the fixture
     becomes a `json` entry.
   - The export golden record is `legacy/work/v1` because that is the type
     the M1 export writes. viitta commits the schema the export writes for
     it, `{"type": "object"}`, at
     `.board/types/legacy/work/v1.schema.json`, and deletes it with the
     export tool after M7.
3. Built: a plugin ships the schemas for its types, and `board plugin enable`
   writes them into `.board/types/` (docs/plugins.md). Not built yet:
   `plugin list` reports an enabled plugin whose schema is missing from the
   tree or differs from it.

**The `controls` plugin.** `controls/control/v1` has `requirement`, `purpose`,
`applies_to` (references to files), `paths` (SQLite `GLOB` patterns over
entry paths, for sets such as `docs/*.md`) and an optional `rule`: none, for
a written requirement, or a budget of `words` (whitespace-separated) or
`bytes` with a `max`, summed over every matched file. Built
(plugins/controls/README.md): the plugin extracts nothing, because `board
query` already holds every control's body. Two capabilities:

- `for-path` returns the controls that apply to a path, by reference or
  pattern, so an agent can ask before it edits a file.
- `check` evaluates each budget, reading files through `board query` and
  `board get`. A violation is the error `controls::budget::exceeded`, naming
  each failing control with its usage and maximum, so `board call` exits 1
  in CI. A control Board marks degraded fails it with
  `controls::control::degraded`. `board check` runs no plugin code and does
  not report controls.

Left out until a real need: ControlTree groups, assessments, paged reports
and control status from the old Board.

**viitta adopts itself.** `.board/board.json` enables `controls`.
`controls/` holds the nine rules from AGENTS.md as written requirements and
the `lint.toml` budgets as budget controls. `bin/lint` stays the gate; CI
also runs `board call controls.check`.

**Pull requests, in order.**

1. Built: `board get` prints any file's bytes.
2. Board knows `board/plugin-manifest/v1`; `record::unknown_type` names the
   missing schema file.
3. `board init` writes a minimal `.board/board.json`. Built: the manifest's
   `types` and `board plugin enable`.
4. Built: the `controls` plugin in `plugins/controls/`, with pytest against
   fixture boards through the real `board`. Next after it: `tasks` and
   `controls` repeat the protocol module, so it moves into a shared Python
   library.
5. Built: viitta adopts itself: `board.json`, the two fixture changes above,
   the control records in `controls/` and the CI step. `lint.toml`'s line and
   `#[expect]` budgets stay in `bin/lint` only: a budget control sums words or
   bytes over files, and neither counts lines per file.

Done: in a fresh clone of viitta with `board` and `controls` installed,
`board check` and `board call controls.check` exit 0, and lowering one
budget below its usage makes the call exit 1 with
`controls::budget::exceeded`.

Limits and controls (owner's decision): a limit that refuses a write, such as
Adoption's file-size limit below, is a setting that `put` and `check` apply. A
requirement on files that is checked on demand, such as a word budget, is a
control.

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
  `board::put::stale`.
- **Push after write.** A write pushes its commits; one that cannot leaves
  them for the next `sync` and says so. Mounted repositories push innermost
  first, so no pushed pin names a commit others cannot fetch.
- **Conflicts.** When the remote moved, Board replays its own commits on top
  only if they touch none of the same files; any overlap stops for the owner
  to resolve in git. Board never merges file contents.
- **Batches** write to one repository each.
- **Nesting** is the same step applied recursively, bottom up: each level
  commits, then the level above moves its pin. Repositories across boards,
  below, replaces this: only the root's pins move.

- **Transport runs the `git` executable** (owner's decision), so credentials,
  SSH and hosts work as they already do; gix keeps reading objects. The
  `git` child process gets one declared owner in `board-tree`.

- **Remote mounts change `board/mount/v1` in place** (owner's decision):
  Board is still being built and has no data to migrate, so `repo` gains its
  remote meaning in v1 with no new version. Repositories across boards,
  below, replaces this: each repository's remote is declared once.
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

- **Mechanics, from research.** A clone is `git init --bare` plus
  `git remote add`, never `--mirror`, which force-fetches over unpushed
  branches. Clones are full, not partial: partial clones reach the network on
  read and gix cannot read them. Each pin keeps a ref
  (`refs/board/keep/<commit>`) so garbage collection never drops a pinned
  commit after a remote force-push. Fetches pass `-c fetch.fsckObjects=true`;
  pushes use `--atomic --porcelain`. Board replays its own commits with gix's
  tree editor, not `git replay`, which merges contents. When two pins diverge,
  the one that descends from the other wins, as git does for submodules;
  otherwise it is a conflict. Repositories across boards, below, replaces
  this: the root's pin always wins.

- **Board detects a colocated jj repository automatically** (owner's
  decision, 2026-10-05). No setting chooses between git and jujutsu (jj).

  The jj design below was agreed with the owner on 2026-10-05 and is not
  built. Board's reads already work on colocated jj repositories, because jj
  writes ordinary git objects. A full version-control trait built on jj-lib
  would take 15 to 25 days and is not worth it: jj-lib breaks its API monthly
  and would pull in a second copy of gix. After M7, Board detects a colocated
  `.jj` directory and lets jj's snapshot record a put instead of committing
  onto HEAD (3 to 5 days). It refuses a conflicted jj commit, one whose
  header has `jj:trees`, with its own code. If jj users need a commit per
  put, Board will later run the `jj` CLI through the same child-process owner
  that M8 creates for `git`. Settled decision 4, under which `put` commits
  onto HEAD as `git commit <paths>` does (docs/architecture.md), is the one
  existing decision that conflicts with jj.

  M8 does the following now, so jj stays cheap to add:
  - Board's clones stay bare git repositories, whatever the user runs
    locally.
  - The single child-process owner lives in `board-tree`, and the
    child-process rule in `clippy.toml` gains it as a second owner.
  - Board never assumes HEAD is a branch.
  - `--at <rev>` takes git revision syntax only.

### Repositories across boards (sketch; agreed with the owner 2026-10-05, not built)

This refines M8 for references between repositories. Adoption, below, covers
importing records from other systems and plugin writes; this section does not
repeat it.

Board stores no large files. A record names its file's `s3://` address, and
the bucket new files go to is a setting the s3 plugin declares and the board
sets as a requirement (Settings, below), so everyone writing to a board
uploads to the same place unless the owner overrides it. An owner who does
uploads elsewhere; the record still names its own address, and `check` reports
it against the requirement. Getting the bytes behind an `s3://`, `op://` or
`widgets://` address is a *fetcher*: a plugin capability that someone calls on
purpose, such as `board call s3.fetch`. `query`, `refs` and `check` never
fetch, and reads never touch the network. The credentials a fetcher needs are
granted in the owner's machine config, and a secret's value reaches a plugin
only through its environment at start, never stored or printed (Adoption,
below).

- **A board declares each repository once**, in a `board/repo/v1` record at
  `/.board/repos/<name>.json`: its remote, its root commit, an optional
  `track: "<ref>"` and the commit it is pinned to. Names match
  `[a-z0-9-]{1,63}` and are local to the board that declares them. A
  repository need not be a board; a plain repository mounts and resolves
  today.
- **A link places a repository in the tree**: `{"path": "acme:/"}` at
  `/work/acme` makes the repository readable there. Several links may reach
  one repository.
- **References inside a repository stay paths.** A reference into another
  repository is written `name:/path`. A path reference that leaves its own
  repository through a mount is refused with `board::ref::crosses_repo`.
- **Board identifies a repository** by its normalised remote URL, checked
  against its root commit. That key is internal; records never contain it.
  `board://` addresses for other applications are later work and will carry
  this key.
- **Tracking and pinned** (owner agreed the name on 2026-10-05). A repository
  with `track` has its pin moved by `board sync` to that ref's commit. One
  field replaces `follow` and a float flag. A pinned repository stays at its
  commit, and everything read through it reads as recorded.
- **One copy per repository.** Every repository the root declares is read at
  the root's pin, wherever it appears. A repository that only a mounted board
  declares is read at that board's pin, and `check` warns when two pins
  differ. This replaces the descendant rule in M8.
- **`board sync`** moves only the root's pins, all in one commit, or writes
  nothing. It takes a per-clone `sync.lock` (`board::sync::locked`), writes
  pins with `--expect`, runs `git` with `GIT_TERMINAL_PROMPT=0`, and reports
  references that the new pins leave dangling. It runs once per call; a
  scheduler may run it, and no daemon is planned.
- **Writes.** A write through a tracking repository commits there and moves the
  root's pin. A write through a pinned repository is refused with
  `board::put::through_mount`. Writes refuse `--at`.
- **`board copy`** copies records into another location, rewrites references
  that resolve through mounts, and refuses one that would resolve to
  something else in the destination with `board::copy::unresolved`.
  `--allow-dangling` and a capped `--with-targets` are its only options. It
  never adds a mount, and it reports with `board/copy-report/v1`.
- **`board share` moves a record** into another repository, such as a
  shared one, and leaves a link at its old path, so local references keep
  working. `--snapshot` copies instead, and records the copy in a
  plugin-owned `share/snapshot/v1` record. It is the sharing that replaces
  widgets's team store (Adoption, below).
- **Time travel**: `--at <rev>` reads the root at a revision, and
  `--at name=<rev>` reads one repository at a revision. Both read local
  clones only.
- **Indexes.** Each board keeps its own index through M8. A cache shared by
  repository key comes only after measurement shows a need.

Decided with the owner on 2026-10-05:
- Proposals against other repositories, and coherence commits, come in a later
  milestone. A proposal is as Adoption, below, defines it. One may target a
  path in another repository, holding a patch and the base blob. It is settled
  by `put --expect` against that base, or applied in a coherence commit: every
  chosen proposal applied at the pin, in path order, with `git merge-file`,
  derived and never stored. An agent works on top of the coherence commit and
  ships with `git rebase --onto <pin> <coherence> <branch>`. Board owns the
  concept; domain plugins, such as a widgets plugin preparing a workspace, choose
  which proposals to apply.
- Sharing a record whose references the destination cannot resolve:
  `board share` takes the same options as `board copy`, plus `--with-repos`.
  `--with-targets` moves the records it refers to as well, up to the cap,
  leaving a link at each old path, so references inside the shared set keep
  resolving. `--with-repos` writes the declarations of the repositories it
  refers to into the destination. A declaration holds only a remote URL, a
  root commit and a pin, but it tells every reader of the destination that
  the repository exists. Without either option, `board share` refuses
  with `board::copy::unresolved`. With `--allow-dangling`, the reference is
  written unchanged and starts resolving once the destination declares the
  same name.
- M10 in #55 (content-addressed records) is dropped. Adoption turns each widgets
  relation into a path reference to `/widgets/<kind>/<id>` and keeps the `widgets://`
  address only as provenance; content hashing stays as Adoption has it.
- The owner's machine config names the owner's one personal board, which
  holds the preferences Settings reads. That is how any root board reaches
  it: M8 fetches the personal board as well as the root's remotes. A shared
  board can never name it, so the network rule's purpose holds. Settings,
  below, lists this entry.
- Whether `docs/architecture.md` keeps saying a board at a commit fully
  determines its mounts. Under tracking it does for reads at the pin; revise
  the sentence when this lands.

### M9 SHA-256 repositories (sketch; after M7)

This milestone lets a board, or any repository it mounts, use git's SHA-256
object format. The model already holds both formats: `ObjectId` has `Sha1` and
`Sha256` (`board-model/src/location.rs`), and a mount pin may name a 64-hex
commit. Hashing a blob already asks the repository for its format. The rest of
the git layer assumes SHA-1:

- gix is built with only its `sha1` feature (`Cargo.toml`).
- `model_id` in `board-tree/src/git.rs` reads any id that is not 32 bytes as
  SHA-1.
- `board-tree/src/write.rs` fills a new file's entry and a staged commit with
  SHA-1 zero ids before the real ids are known.
- `board-scenario` creates only SHA-1 repositories and reads every id back as
  SHA-1, and the working-tree hash test computes a SHA-1 id.

The work enables gix's SHA-256 support, reads each repository's object format
when it is opened, and removes the SHA-1 placeholders. The scenario and
concurrency tests then run once for each format.

Git cannot push or fetch between a SHA-1 and a SHA-256 repository. A board
whose mounts use different formats therefore keeps both kinds of pin, and each
clone that M8 keeps uses its remote's format.

A standing rule keeps this milestone cheap (owner's decision, 2026-10-05):
new code never assumes SHA-1. It asks the repository for its object format,
as `write.rs` and `worktree.rs` already do when they hash.

Open: how mature gix's SHA-256 support is for reading, writing commits and
updating the index; how far GitHub supports SHA-256 repositories (the owner
reports a beta; this is not yet verified).

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
- Bindings copy A2UI's semantics without adopting it (pre-1.0): the
  component catalogue is a JSON Schema, bindings are JSON Pointers, lists come
  from templates and actions are named events. Forms use JSON Forms' UI-schema
  semantics without function-based conditions.
- The sandbox copies MCP Apps' rules: an iframe with `allow-scripts` and
  without `allow-same-origin`, a host-set CSP with `connect-src 'none'`, and
  postMessage messages checked against a schema. Charts are Vega-Lite inside it.
- Acceptance, set by the owner: the UI can express widgets's UI. The component
  vocabulary is derived from it, which needs read access to example-org/widgets.

### Serving (sketch)

`board serve` is the one long-lived process allowed. It may keep the index
warm for an agent loop or a web view; no answer may depend on it, and every
answer equals a cold run's. It is a small synchronous server; because a local
server that writes is reachable through DNS rebinding, it checks the Host
header and requires a token. The measured warm-check speedups (HANDOFF item 2)
land after M7 gives real numbers.
A `board mcp` adapter can serve each capability as the MCP tool
`<plugin>.<capability>` with no translation, since capabilities are Tool objects.

### Adoption (sketch; agreed with the owner 2026-10-05, not built)

Board should be the one place to address things from other systems: every widgets
record, every JIRA ticket, and how to run a command against each. The other
system keeps its own records unchanged; Board adopts them. widgets comes first, and
JIRA is the second user that principle 8 requires.

**Two ways in.**

- *Mount* a git repository to see its files as they are, with no copy: the
  board declares it (Repositories across boards, above) and a link places
  it in the tree. A link pointer can name one file inside it. Mounting
  already exists; the files are untyped unless they carry `$type`.
- *Import* to make another system's data into typed Board records. A plugin
  reads the source, types each item from the source's own type field and
  writes it with `put`. Use it for anything not in git (JIRA) and anything
  that needs a type or references (widgets). The record keeps the source's
  original JSON unchanged under one field, `source`, beside `$type` and
  Board's references, so the source's keys never collide with Board's.

**widgets.** widgets's team store (example-org/changelog) holds `<kind>/<id>.json` with
`kind`, a content-hash `id` and `relations` such as `widgets://result/<id>`. The widgets
plugin imports each as `widgets/<kind>/v1` at `/widgets/<kind>/<id>` and turns each
relation into a reference to `/widgets/<kind>/<id>`, so `refs` and `query` work
across widgets records. The schemas live in the importing board. Machine-local
state with an expiry (observations) is not imported.

**Writes go to the source.** A plugin capability such as `widgets.up` runs the
source's own CLI; it never edits the source's store. The record says what to
act on and installed plugin code acts, so data never becomes code (principle
6). After the source changes, the import runs again.

**Staying true to the source.**

- The source is the truth for its data; Board's copy is an import, and
  Board's answers are always true of Board's tree (principle 1).
- Only the plugins the board grants may write an imported path: the
  importer, and the reconciler below.
- Each record keeps where it came from: the source address and the source's
  revision (for widgets, the id itself).
- widgets ids are content hashes, so a copy is never wrong, only missing newer
  records. For a source whose items change (JIRA), the import replaces a
  record with `put --expect`, so two writers cannot overwrite each other.
- An unchanged source imports to identical bytes, so re-importing changes
  nothing and commits nothing.
- Imports run when asked (`board call widgets.import`) or on a schedule. Reads
  never import and never touch the network.
- After M8, the widgets plugin can import into a team board that the owner's
  board declares with `track` (a write through a pinned repository is
  refused), so Board provides the sharing and widgets's own team store can
  retire.

**Reconciling the delta.** An import does not decide what to do when the
source and Board's copy differ. It splits into three steps:

1. The importer reads the source and produces the records it would write. It
   decides nothing.
2. The delta is computed against the tree: for each item, added at source,
   changed at source, removed at source, or changed on both sides (Board's
   copy no longer matches the revision it was imported from). The delta is a
   typed document, `board/delta/v1`, so any reconciler can read any
   importer's delta. The importer states the scope it covered (all of widgets's
   team store, or these 40 tickets); only an item inside that scope can be
   removed at source. Otherwise a ticket leaving a JIRA filter, or a fetch
   that failed halfway, would look like a deletion.
3. A reconciler, which is an ordinary plugin capability, receives the whole
   delta at once, so it can see patterns such as a mass removal or a moved
   ticket. It writes the outcome back item by item with `put --expect`,
   through the same gate as any plugin, so a run that stops halfway leaves
   correct records.

A reconciler gets the same grants as any plugin, and no more. Reacting to a
change elsewhere, such as closing a task when its ticket closes, is a
separate later feature called a watch.

Reconcilers are plugins so that people can add new ways to settle a delta.
Likely first ones: *mirror* (the source wins; the default for content-addressed
sources such as widgets), *keep* (mark records removed at source rather than
delete them), *propose* (write each disagreement as a proposal record for the
owner to settle), and *agent* (hand the delta and a prompt to an agent, which
writes the outcome or proposals). The importer's manifest names a default;
the board's settings name the reconciler per path or type, and the owner can
refine that (see Settings below). An agent reconciler's credentials are
granted in machine config like any other need.

A proposal is a record holding a pending change and the base it was written
against, so it is queried and settled like any other.
Accepting one is `put --expect` against the version the proposal was made
from, so a stale proposal is refused. Whether a reconciler applies or
proposes, and when the owner hears of it, are settings (below). A change
applied without review says so in the record.

The outcome is commits, so the result is in the tree and every reader sees
the same answer; reads never reconcile. The import and the delta are
deterministic. An agent reconciler is not, so the `Board-Plugin:` trailer on
its commits records which reconciler made each decision. Computing the delta
starts in the shared Python plugin library; it moves into the core only if a
plugin in another language needs it (principle 8).

**Plugins use Board through one gate.** A plugin may run `board put`, as it
already runs `board query` and `board call`. Board starts it with
`BOARD_PLUGIN=<name>@<version>`; the CLI reads it once into a principal (the
owner or a plugin), every operation takes the principal, and one function
decides access, refusing with a stable code such as `board::access::denied`.
Each commit from a plugin's put carries a `Board-Plugin:` trailer, so git
history is the audit log. These checks catch mistakes and make access
visible. They do not stop a hostile plugin, which runs as the owner; that
would need an OS sandbox.

**What a plugin needs, and who grants it.** The manifest declares needs by
name, never value: environment variables (required or optional, each with a
reason) and a longer time limit per capability. Grants come from two places:

- Read and write scope (which paths and types a plugin may read or put) is
  about the board's data, so it is a board setting, `access`, decided by the
  same function as every other setting (Settings, below).
- Environment variables concern the owner's machine and secrets, so they are
  granted in the owner's machine config, read once at startup. A shared board
  must never be able to hand someone's credentials to a plugin.
- A longer time limit is a setting like any other (see Settings below).

Values come from the owner's environment when the plugin starts and are never
stored or printed (STANDARDS.md 16). An ungranted need refuses before the
plugin starts and names the config line to add; a granted required variable
that is unset refuses and names it. A program that needs an interactive login
cannot run in a plugin; the plugin returns a typed error naming the command
to run. The manifest's needs fold into the `board/plugin-manifest/v2` that
Views already plans.

**Limits.** A limit such as "no adopted file over 1 MB" is a setting like any
other (below). `put` applies the setting in force for whoever writes. `check`
applies only what the board's own tree says, on every machine, so it fails CI
through `board check` when the board makes the limit a requirement, and
never because of a default or anyone's personal refinement. An owner who
overrides the board's requirement can still write the file; `check` keeps
reporting it.

A limit on files, not records, is declared the same way: a Board setting
with a path glob, which records the control and why it exists.
`.gitattributes` rules and lints enforce limits too. They complement the
setting and neither replaces the other. Board may later check that they
agree.

**Next steps.**

1. Spike: a plain script imports the team store into a scratch board, read
   only on the source. It shows what `check`, `refs` and `query` answer over
   real widgets data and whether every kind is content-addressed.
2. Build the principal and `board decide` with `put --or-propose`
   (Settings, below), then the machine config and `board/plugin-manifest/v2`
   with their codes.
   Design `board/delta/v1` alongside, using the spike's real deltas.
3. Build the widgets plugin on the Python library that a second plugin starts.
4. JIRA second, which proves the design is not widgets-shaped.

Open:
- Which widgets kinds change after they are written (workspaces?). The spike in
  next step 1 answers this.

### Settings and the gentle slope (sketch; agreed with the owner 2026-10-05, not built)

Board is where the complexity of settings lives, so that plugins stay simple.
A plugin asks one question, "for this operation on this path, may I, do I act
or propose, and how is the owner told?", and does what the answer says. Most
plugins need not even ask: they write with `put --or-propose` and Board
decides.

**The gentle slope.** Settings, like every concept, reach the owner only
when they are needed. The rules, from the owner's earlier designs:

- A concept appears only after its absence was felt. If that never happens,
  the owner never meets it. There is no tour, tutorial or settings page.
- Each introduction is one sentence and one act. The first time a setting
  matters, Board says what it is, where it came from (this board, the
  owner, a mandate) and what choosing would change.
- The owner can wave an offer away. *Later* brings it back at the next
  separate run of a command that needs it, never twice in one run; *No*
  never brings it back unasked unless the setting changes, and the ability
  stays findable.
- Only the owner accepts. Agents and plugins suggest; a suggestion applies
  nothing until accepted, and one left unjudged expires.
- The everyday surface does not grow. Power views are visited and left.
- The order concepts are introduced in is the order they are built.

**Layers.** Three sources, highest first:

1. A *mandate* from an authority above the owner, such as an enterprise
   policy. It cannot be refined: an owner working within an organisation is
   bound by it, and how strict it is stays the organisation's call. It
   cannot live in anything the owner edits, so on a machine it comes from
   managed config the owner cannot change; real enforcement is on the server
   (a required CI check, a protected branch), because whoever controls a
   machine can bypass Board. A preference that a mandate outranks is still
   admitted, since the personal board travels to machines without the
   mandate: `put` warns once, naming the authority, and every decision lists
   the preference as outranked.
2. The owner's *preferences*, as records in the owner's personal board, so
   they follow the owner between machines and keep their history. Machine
   config holds only secrets, environment grants and the name of the
   owner's personal board (Repositories across boards, above).
3. The *board's settings*, in its tree. The owner has the final say over
   all of them. A *default* the owner refines freely. A *requirement* the
   owner may also override, but only after a warning and an explicit
   decision; the preference records the hash of the setting it overrides,
   so a changed requirement asks again. `check` still reports against the
   requirement, so the board sees the override in CI. Changing a setting
   for everyone is a proposal to the board.

The first time a board's setting matters, the owner can keep it, refine it
for themselves, propose a change for everyone, or say Later or No. The
choice is recorded in the personal board, so it is not asked twice.

**Two settings per decision.** *Authority*: act, or propose. *Attention*:
mark it and say nothing; tell the owner when they next look; tell them when
a plugin reads the record (`query` and `refs` report an open proposal with a
stable code, derived from the tree); or tell them at once.

**Records** (agreed 2026-10-05 after four agents argued rival shapes).

- The core and each plugin's manifest declare named settings, each with a
  value schema and a default. A plugin's names start with its own name. A
  released name never changes meaning; a new meaning is a new name.
- One record per preference, so each has a path, a history and its own
  `put --expect`. The owner's are `board/preference/v1` in the personal board; a
  board's are at `/.board/settings/<name>.json` with the same shape, where
  `<name>` is any name the board's authors choose for that scope, as a file
  name; a mandate's are in the managed file. The layer comes from where a
  record was read, never from a field in it. `.board/board.json` keeps Board's
  own configuration (`commit_on_put`, enabled plugins) until an owner needs to
  refine one of those; then it moves to a setting in a new config version.
- A record is one scope and the values that apply inside it, each value a
  constraint naming a declared setting:

  ```json
  {"$type": "board/preference/v1",
   "where": {"board": "acme", "path": "/jira/**",
             "sql": "status = 'Closed'"},
   "set": {"authority": "act", "attention": "silent"},
   "because": ["acme:/proposals/p1"]}
  ```

  `where` may name a board, a path glob with git's pattern rules, a type, an
  operation, and an SQL condition run against the one record the decision is
  about, under `query`'s guards. The owner rarely writes `sql`; Board drafts
  it from a correction and shows it as a sentence. A limit's value is a plain
  number.
- `board` appears only in the owner's records. It is a repository name the
  personal board declares in its own `/.board/repos/<name>.json`, as are the
  names in references such as `acme:/proposals/p1`; an undeclared name is
  `ref::dangling` like any other. Without `board`, a preference applies in
  every board the owner works in.
- Three optional fields: `refines` names another record in the same layer that
  this one takes precedence over, and `required: true`, in a board's records
  only, makes its values requirements; without it they are defaults.
  `overrides` in an owner's record holds the blob hash of the board
  requirement it overrides (Resolution, below).
- The owner's answers to offers are separate `board/seen/v1` records holding
  the hash of the setting record seen and the answer (keep, later, no), so
  a decision's reasons cite only real preferences, and a changed setting is
  introduced again.

**Resolution.** For each setting:

1. A matching mandate is final. An owner's preference it outranks is still
   admitted, and the decision lists it with `board::settings::outranked`.
2. Otherwise the highest layer with a matching record wins, in the order
   owner, board, manifest default; a higher layer replaces a lower one
   rather than combining with it.
3. An owner's record that replaces a board requirement counts only if its
   `overrides` holds that requirement's current blob hash. `put` refuses
   such a record without it, with `board::settings::overrides_requirement`,
   naming the requirement; `put --override` is the owner's explicit decision
   and writes the hash. When the requirement changes, the hash no longer
   matches, the requirement applies again, and Board asks again.
4. Specificity and arrival time never pick a winner. Two matching records in
   one layer that disagree are `board::settings::conflict` unless one names
   the other in `refines`: Board uses the safer value (propose, tell at
   once, the smaller limit), names both records, and asks the owner once
   which refines which. A condition can make two records overlap only for
   some records, so some conflicts are found when they first happen.

Each value is reported with its kind, its layer, and the record and blob
behind it. The kinds are `mandated` (a mandate), `required` (a board
requirement), `chosen` (the owner), `default` (a board value without
`required`) and `declared` (the manifest's default, step 2's last layer).

`check` reads only the board's own settings and reports a value that breaks a
board requirement, so a requirement fails CI and a default never does. A
board that wants CI to enforce a limit marks it required.

**The call and the gate.** `board decide` reads lines of `{"op", "path"}` and
answers each with `access` and every setting the manifest declares for that
operation, with its reasons. It takes a list so an import of 300 items starts
one process. The plugin is known from `BOARD_PLUGIN`, never stated by the
plugin. `decide` is advice. The same function runs inside every `put` from a
plugin, which refuses what access, limits or authority forbid, so a plugin
that never asks still cannot write what it may not. `put --or-propose` writes
the record when authority is act and a proposal (Adoption, above) when it is
propose. Attention is Board's job: Board tells the owner, the plugin never
does. A plugin cannot show an offer either; Board answers with the current
value and shows the offer when the command ends, carried back from `decide`
through a per-run scratch directory named in the plugin's environment.

**Reads.** A fetch from another system is an operation like any other:
`jira.fetch` on `/jira/X/**` is decided before Board starts the plugin. A
fetch writes no record, so when authority is propose Board asks the owner
before it starts the plugin ("Fetch JIRA project X? Once, always for X, or
no"). The answer is an ordinary preference. A scheduled fetch that
would have to ask is skipped, and the owner is told as attention says. Board
can stop the call; only the source's own token scopes can stop a plugin that
reaches further once it holds the token. A plugin's reads of the board
(`query`, and the records sent to it) are gated by `access` too, through
views filtered to the paths it may read. That is designed in now and built
when a second plugin exists.

**Learning from corrections.** Both kinds of mistake are evidence: Board
asked when the owner wanted it to act, or acted when it should have asked.
Evidence never becomes a preference silently, and silence proves nothing.
Three kinds, all seen inside Board's own commands, so nothing runs in the
background:

- An explicit correction (`board correct <record>`, or "should have just done
  it" on an offer). One is enough.
- A proposal accepted unchanged: Board counts these as it settles them, and
  five in a row for one setting and scope, none rejected or edited, suggest
  acting. The count is itself a setting.
- An autonomous change the owner reverts or edits: Board sees the owner
  write a record whose last commit was a plugin's unreviewed change. One is
  enough, so tightening is easier than widening.

Time to approve is not evidence; nothing is keyed on time. The suggestion is
an offer naming the narrowest scope that covers the evidence, with wider ones
(this board, all my boards) to choose from. Accepting writes a preference
whose `because` lists the evidence. No is recorded and holds until evidence
of the opposite kind arrives.

The old Board needed a skill head, a declaration hash, an item id and a scope
to set one value; refining a setting here takes one answer to one question.

Open:
- Where a mandate's managed config lives and how it names its authority.
  This is deferred until an organisation needs a mandate. `board decide` is
  built first without the mandate layer.

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
   default-loaded files together. The docs budget was lowered from 8,000 to
   5,000 words after the altitude pass (#60), owner's decision of 2026-10-04.
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
