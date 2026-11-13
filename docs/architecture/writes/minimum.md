# Minimum version

`min_board` in `.board/board.json` (for example `"min_board": "0.4.0"`) names
the oldest Board that may write the board. A Board older than it refuses
every write (put, rm, batch, a write through a mount to this board) and
every sync with `board::config::board_too_old`, whose message names both
versions; reads and `check` still work. Versions are `major.minor.patch`; a
`-` or `+` suffix, such as the `+<commit>` that `board --version` prints, is
ignored. A build whose number is `0.0.0` is an unreleased build from source
and is never refused. A `min_board` that is not a version makes the config
`board::config::invalid`. A Board from before this field cannot open the
board at all, since it reads the field as an unknown one.
