# Area churn: can an area resolve to paths well enough to trigger an audit?

This document records a design test. It asks whether a conceptual area of a
code base can be written down as a list of paths, and whether a cheap git
command over those paths can say when an audit is due. The subject is this
repository at main `8ed8078f` (11 November 2026). No plugin code was written.

The control under test has the form "after N lines change in an area, run an
audit". It needs two things: the area must resolve to paths, and a program
must count the lines changed in those paths since the last audit.

## Method

Three areas were chosen:

1. **Controls crate**: one crate, `crates/board-plugin-controls`.
2. **Write locking**: a concept that spans crates. It covers the board's
   `board.lock`, the code that takes and holds it, and its refusal
   `board::put::locked`.
3. **Design documentation**: an area that is mostly documentation.

Each area has a strict list, holding only the files that clearly belong, and
a wide list, which adds the files that were ambiguous. Both were counted, so
the effect of the ambiguous files on the numbers is visible.

The cost of writing each list is given as tool calls and files read, since a
wall-clock time depends on the machine and the agent. "Files read in full"
means a file opened and read whole. "Files inspected" means a file whose
matching lines a search printed.

Churn is lines added plus lines deleted, as `git log --numstat` reports them,
summed over the commits on main's first-parent line. A merge counts as its
diff against the previous main commit, so a branch's commits are not counted
twice. Weeks run Monday to Monday in UTC. The last week holds only
9 to 11 November.

## The path lists

### 1. Controls crate

Strict list (23 files):

```
crates/board-plugin-controls/
```

Wide list adds:

```
plugins/controls/                      # skill, README and Claude Code mod
docs/plugins/controls-claude-code.md   # the mod's design page
```

Cost: 2 tool calls (a directory listing and one `git grep` for the crate's
name), 0 files read in full, 10 files inspected.

Confidence: high for the strict list. A crate is its directory.

Ambiguous files:

- `plugins/controls/**` is the agent-facing half of the same plugin. It
  changes whenever the crate's commands change, but it is not the crate.
- `docs/plugins/controls-claude-code.md` and the controls sections of
  `docs/plugins.md` and `docs/plugins/built-in.md` describe the plugin. The
  last two are shared with other plugins, so whole-file counting would
  charge their other sections to this area. They were left out of both lists.
- `crates/board-plugins/src/lib.rs` registers the plugin, along with every
  other built-in plugin. It was left out.
- `controls/**` holds this repository's own control records. They are data
  that the plugin reads, and they were left out.

### 2. Write locking

Strict list (21 files):

```
crates/board-tree/src/write.rs              # Lock, Repositories::lock
crates/board-tree/src/write/sync.rs
crates/board-tree/src/repositories.rs
crates/board-ops/src/put.rs
crates/board-ops/src/put/batch.rs
crates/board-ops/src/put/file.rs
crates/board-ops/src/put/remove.rs
crates/board-ops/src/put/through.rs         # the mounted board's own lock
crates/board-ops/src/enable.rs
crates/board-ops/src/effects/decide.rs
crates/board-ops/src/mount.rs
crates/board-ops/src/sync.rs
crates/board-ops/src/put/tests.rs
crates/board-ops/src/put/tests/waiting.rs
crates/board-ops/src/put/tests/batch.rs
crates/board-ops/src/put/tests/through.rs
crates/board-ops/src/host/tests/sync.rs
crates/board-ops/src/mount/tests.rs
crates/board-tree/src/tests/commits.rs
crates/board-tree/src/tests/syncing.rs
docs/architecture/writes.md
```

Wide list adds:

```
crates/board-index/src/store.rs             # the index's SQLite write lock
crates/board-index/src/open.rs
crates/board-model/src/code.rs              # the PutLocked error code
crates/board-ops/src/host.rs
crates/board-ops/src/types.rs
docs/plugins/share.md                       # recovery advice for board.lock
docs/plugins/built-in/elsewhere.md
plugins/share/SKILL.md
crates/board-cli/src/tests/recoveries/recoveries.txt
```

Cost: 5 tool calls (three `git grep` sweeps for lock words, the lock's
type and its callers, and one pass over the borderline files), 0 files read
in full, about 45 files inspected.

Confidence: medium. The strict list finds every caller of
`Repositories::lock` and every file that defines or tests the wait. A
search for the word "lock" also matches `Mutex::lock`, "block" and
"blocked", so the results have to be narrowed by hand, and a reader can
still miss a file whose locking role carries no lock word.

Ambiguous files:

- `crates/board-index/src/store.rs` holds a different lock, SQLite's write
  lock on the index. Whether "locking" means the board's lock or every lock
  is a definition question that the path list cannot answer.
- `crates/board-model/src/code.rs` holds every error code. One line of it
  belongs to this area.
- `put.rs`, `mount.rs`, `enable.rs` and `sync.rs` take the lock, but most of
  their lines do other work. Whole-file counting charges all their change to
  this area. In the week of 12 October, 803 of the area's 1,110 changed lines
  came from two new files for writing through a mount. Those files take a
  lock, but they are mostly about mounts.
- Eleven of the strict files were created after 12 October. A list of
  single files written on 12 October would have missed them until someone
  added them by hand.

### 3. Design documentation

Strict list (55 files):

```
docs/
DOCS.md
PLAN.md
plan/
```

Wide list adds (37 files):

```
AGENTS.md
CLAUDE.md
STANDARDS.md
README.md
:(glob)plugins/**/*.md        # each plugin's SKILL.md and README.md
```

Cost: 1 tool call (a listing of `docs/`), 0 files read in full. The strict
list follows the layout that `lint.toml` already budgets.

Confidence: high for the strict list, medium for the wide one.

Ambiguous files:

- `plugins/*/SKILL.md` files are documentation and also shipped behaviour.
  An agent follows them, so a change to one can change what agents do.
- Rust doc comments (`//!` and `///`) are documentation inside code files.
  A path list cannot separate them from the code.
- `lint.toml` holds the word budgets for the documentation, but it is
  configuration.

## Counting change since a commit

The one-liner counts lines changed in an area's paths since a commit, such
as the commit the last audit read:

```sh
git log --first-parent --diff-merges=first-parent --format= --numstat "$SINCE"..HEAD -- "${AREA[@]}" \
  | awk '{n+=$1+$2} END{print n+0}'
```

`AREA` is an array of git pathspecs. Binary files print `-` and count as 0.
Over the 274 main commits of the last 30 days, the widest area took
0.04 seconds.

`git diff --numstat "$SINCE" HEAD -- ...` gives the net change instead of
the sum of each commit's change. It is smaller when lines are rewritten more
than once. Over the 30 days the controls crate's net change was 5,679 lines
against a sum of 7,539. The sum is the better trigger, because rewriting the
same lines twice is two changes an auditor may want to see.

## Weekly numbers

Lines changed per week, with the number of main commits that touched the
area in brackets.

| Area | 12 Oct | 19 Oct | 26 Oct | 2 Nov | 9 to 11 Nov | 30 days |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Controls crate, strict | 0 (0) | 0 (0) | 2,469 (7) | 4,548 (21) | 522 (2) | 7,539 |
| Controls crate, wide | 1,309 (10) | 528 (5) | 5,725 (13) | 5,720 (26) | 565 (4) | 13,847 |
| Write locking, strict | 1,110 (3) | 4,356 (17) | 2,076 (23) | 885 (11) | 301 (4) | 8,728 |
| Write locking, wide | 1,938 (8) | 5,410 (21) | 3,388 (30) | 1,290 (24) | 423 (6) | 12,449 |
| Documentation, strict | 828 (17) | 2,415 (52) | 3,316 (65) | 2,087 (49) | 100 (7) | 8,746 |
| Documentation, wide | 1,455 (25) | 4,509 (59) | 4,640 (73) | 6,707 (76) | 447 (10) | 17,758 |

The controls crate shows no change before 1 November because the crate was
created that day (#48), when the plugin moved into Board as a built-in. The
creation alone added 1,401 lines.

## Proposed thresholds

The thresholds aim at about one audit every one to two weeks at the pace of
the last month. The table gives how many audits each threshold would have
started in the 30 days. The counter resets to zero after each audit.

| Area | 1,000 | 2,000 | 3,000 | 5,000 | 8,000 |
| --- | ---: | ---: | ---: | ---: | ---: |
| Controls crate, strict | 5 | 3 | 2 | 1 | 0 |
| Controls crate, wide | 10 | 5 | 3 | 2 | 1 |
| Write locking, strict | 7 | 4 | 2 | 1 | 1 |
| Write locking, wide | 10 | 5 | 3 | 2 | 1 |
| Documentation, strict | 8 | 4 | 2 | 1 | 1 |
| Documentation, wide | 16 | 8 | 5 | 3 | 2 |

Proposals:

- **Controls crate: 3,000 lines on the strict list.** It would have fired
  twice, at `793a3a5e` (2 November) and `7e0eace3` (6 November). Both fall
  in the weeks the crate was being built, which is when an audit is useful.
- **Write locking: 3,000 lines on the strict list.** It would have fired
  twice, at `a3fb28a5` (22 October, batch puts through a mount) and
  `0afd3319` (27 October, recursive removal). Both changed what a put does
  while it holds the lock.
- **Documentation: 3,000 lines on the strict list.** It would have fired
  twice, at `bea6652b` (25 October) and `e71def99` (1 November). With the
  wide list, 5,000 lines gives three audits.

A line count treats a new file the same as an edit. The controls crate's
creation reached almost half of its threshold in one commit. That is
acceptable for a trigger, because a new crate is worth auditing. A control
that should ignore new files would count only modifications, by adding
`--diff-filter=M` to the one-liner.

## Keeping the path list current

Three ways were considered.

1. **By hand.** The control record holds the pathspecs, and a person edits
   them. This costs nothing to build. Directory pathspecs, as in areas 1 and
   3, stay correct as files are added. Lists of single files, as in area 2,
   go stale when code moves.
2. **A tag in the code**, such as a `//! area: locking` line. A tag moves
   with its code, so a split file keeps its area if the author copies the
   tag. But Markdown, JSON and test fixtures have no shared comment syntax,
   a tag says nothing about the files that lack it, and finding tags needs
   a scan of every file's contents rather than a pathspec.
3. **A check that flags files matching no area**, or a pattern that matches
   no file. This keeps hand-written lists honest without tagging code. It
   only works where every file is meant to belong to some area. A finer
   form flags a new file that names one of the area's terms, such as
   `Repositories::lock`, while matching none of its pathspecs.

## Recommendation

Declare an area in the control record as a list of git pathspecs with
optional exclusions, using git's own pathspec syntax (`dir/`, `:(glob)`,
`:(exclude)`). Prefer directories and globs, and use single files only for
a concept that spans crates. Count churn with the one-liner above from the
commit the last audit read, which the audit's run record already holds.

Keep the list current by hand, backed by a check that reports two things:
a pathspec in an area that matches no file, and a new file that names one
of a concept area's listed terms while matching none of its pathspecs.
Do not use tags in code.

The test supports this for a crate and for documentation, where the paths
are the area. It supports it with a caveat for a concept such as locking.
The list takes more work, the boundary needs a decision that a path cannot
make (the board's lock or every lock), and whole-file counting overstates
the change. For a trigger, an overstated count only starts an audit early,
so the caveat does not block the control.
