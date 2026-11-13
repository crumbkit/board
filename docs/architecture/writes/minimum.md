# Minimum version

`min_board` in `.board/board.json` names the oldest Board that may write the
board. A Board older than it refuses every write (put, rm, batch, a write
through a mount to this board) and every sync with
`board::config::board_too_old`, whose message names what the board needs and
what is running. Reads and `check` still work.

`min_board` is one of:

- a version, `"0.4.0"`: `major.minor.patch`, with a `-` or `+` suffix ignored;
- a UTC timestamp, `"2026-11-13T10:00:00Z"` (any RFC 3339 timestamp);
- an object, `{"version": "0.4.0", "built": "2026-11-13T10:00:00Z"}`, with
  either key or both.

Every build embeds the commit date of its source: `SOURCE_DATE_EPOCH` when
set, otherwise git's HEAD committer date, otherwise none. `board --version`
prints it, as `0.0.0+a598bc545d84 (built 2026-11-13T00:00:00Z)`. A build is
refused when it is older than the version named, or when its date is before
the timestamp named. A build with no date is judged by the version alone.
Source builds are numbered `0.0.0`, so a `min_board` version above `0.0.0`
refuses them; use a timestamp to gate them by date.

A `min_board` that is none of these makes the config `board::config::invalid`.
A Board from before this field cannot open the board at all, since it reads
the field as an unknown one.
