# export-old-board

A throwaway script for PLAN.md milestone M1. It reads every object the old
`board objects list` returns, plus the drops that list hides, and writes each one as a canonical record in a new
board. Delete this directory once the export repository exists.

## Run it

```sh
mkdir ~/src/board-export && git -C ~/src/board-export init
uv run tools/export-old-board/export_old_board.py ~/src/board-export
```

`python3` 3.12 or newer works in place of `uv run`. Options:

- `--board PATH`: the old `board` executable. Default: `board` on `PATH`.
- `--store PATH`: passed to the old `board` as `--store`. Default: its own.
- `--jobs N`: concurrent `objects show` calls. Default 8. Use 1 if
  `show_failed` entries name a lock or busy store.

The output directory must exist and hold nothing but `.git`. The script reads
the old board only through `objects list` and `objects show`. It never runs a
writer.

## What it writes

- `<type>/<a>/<b>.json` for each subject `<type>:<a>/<b>`: the `object` from
  `objects show`, canonical, with `"$type": "legacy/<type>/v1"`.
- `.board/types/legacy/<type>/v1.schema.json`: `{"type": "object"}`.
- `.board/board.json`: `board/config/v1`.
- `export-report.json`: the listed counts (default list and `--type drop`),
  the distinct subject count, the record count, counts per type and per
  unmappable reason, every unmappable object with its reason, and each `record_set_sha256` the
  `show` calls saw. More than one means the store changed during the run; run
  it again into a fresh repository.

The record count always equals the distinct subjects minus the unmappable ones.
The script exits 1 if it does not. Unmappable reasons:

| Reason | Meaning |
| --- | --- |
| `subject_malformed` | no `:` in the subject |
| `type_invalid` | the type is empty or not `[a-z0-9-]` |
| `path_malformed` | `/<type>/<name>` is not a logical path (docs/architecture.md) |
| `case_collision` | the record file, or a directory on its path, differs from another name only by ASCII case |
| `show_failed` | `objects show` failed; `detail` holds its error |
| `write_failed` | the file could not be written, such as a name over 255 bytes; `detail` holds the error |

A record `x.json` may sit beside a directory `x/`, so `project:board` and
`project:board/core` are both exported. Distinct subjects never map to one file.

## Tests

```sh
cd tools/export-old-board
uv run --with pytest pytest
uvx ruff format --check . && uvx ruff check .
```

The tests run the script against `tests/fake_board.py`, which prints the old
CLI's JSON shapes. CI does not run them.
