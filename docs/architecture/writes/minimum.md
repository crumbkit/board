# Minimum version

`min_board` in `.board/board.json` names the oldest Board that may write the
board. A Board older than it refuses every write (put, rm, batch) and every
sync with `board::config::board_too_old`. The message names what the board
needs, what is running (the version, the `+<commit>` of a source build, and
the build date), and the install step that satisfies the gate. Reads and
`check` still work. A write through a mount lands in the mounted board, so it
obeys that board's own `min_board`; it also obeys the root board's, because
the root board takes its write lock before it finds that the path crosses a
mount. A mounted board that sets none is therefore still gated by the root.

`min_board` is one of:

- a version, `"0.4.0"`: `major.minor.patch`, with a `+` suffix ignored. A `-`
  suffix, as in `1.2.0-rc1`, marks a pre-release, which is older than `1.2.0`;
- a UTC timestamp, `"2026-11-13T10:00:00Z"` (any RFC 3339 timestamp with a
  zone, which the board reads with jiff);
- an object, `{"version": "0.4.0", "built": "2026-11-13T10:00:00Z"}`, with
  either key or both.

## The build date

Every build embeds the commit date of its source: the committer date of the
commit `BOARD_BUILD_COMMIT` names when that is set, otherwise of git's HEAD,
otherwise `SOURCE_DATE_EPOCH` when it is set, otherwise none. `board
--version` prints it, as `0.0.0+a598bc545d84 (built 2026-11-13T00:00:00Z)`. A
build is refused when it is older than the version named, or when its date is
before the timestamp named. A build with no date is judged by the version
alone, and writes a board with a date gate anyway. It prints a warning on
stderr when it does, whichever board it wrote: the root, one through a mount,
the personal board or one a plugin opened. The warning says that it could not
check the date.

Source builds are numbered `0.0.0`, so a `min_board` version above `0.0.0`
refuses them; use a timestamp to gate them by date. A version needs a release
build to satisfy it, and a build date is what a source build can satisfy.

A date gate trusts the date of the commit the build was made from, and does
not check that the build contains any particular change. A branch cut before
the change, with one new commit made after the gate's date, builds a Board
dated after the gate that lacks the change. So do an amended commit, a
cherry-pick of old code onto an old base, and a rebase of an old branch onto
another old base. Decision /projects/board/decisions/20261113T1034Z-c504
chose the commit date, with this limit, because every Board on the owner's
machines is a source build and no other fact about a build is embedded. A
gate that must hold against an old branch needs a build that embeds whether a
named commit is an ancestor of its HEAD; no build does yet.

## The refusal's install step

The message ends with what to do, chosen from the build and the gate:

A published release is numbered above `0.0.0` and also prints
`+<commit>`; a build numbered `0.0.0`, with or without a commit, is a source
build.

- a release build older than a version: install a release of that version or
  newer (README.md, "Installing a release");
- a release build older than a date: install a release built from source
  committed at or after it;
- a source build older than a version: check out the tag `v<version>` or a
  later commit, then run `cargo install --path crates/board-cli --locked`;
- a source build older than a date: check out a commit dated at or after it,
  then run the same command. A `git pull` on an older branch is not enough.

## A `min_board` this Board cannot read

A `min_board` that this Board does not understand, such as an unknown key or a
string form a newer Board introduced, does not stop the board opening. The
board reads as usual, and every write and sync is refused with
`board::config::min_board_unreadable`, whose message names the `min_board`
field and says why it could not be read. `board::config::invalid` still
covers the rest of the config.

A Board from before this field cannot open the board at all, since it reads
the field as an unknown one, so it loses reads too. Run from a repository that
a board mounts, such a Board skips a known board it cannot open and may
report `board::config::not_found`. Before setting `min_board` in a board,
every Board on every machine that reads it must be at or after the merge that
added the field. This Board does not read a known board it cannot open as
no board: it goes on to the others, and when none mounts the repository it
refuses with the first such board's own code.

## Where the build comes from

The binary declares its build once, at the start of every run
(`board_ops::declare_build`), and every board opened after that gates its
writes as that build, including the personal board that an answer or a held
effect writes and a mounted board reached through a put. A process that never
declares runs as `board-ops`' own version with no date.
