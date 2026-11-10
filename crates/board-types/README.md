# board-types

Types for programs that read the documents `board --json` prints, or write
records into a board. Board prints and parses through these types, so the
crate at a commit describes what the `board` built from that commit prints.

The crate is not published. Depend on it through git, pinned to a commit:

```toml
[dependencies]
board-types = { git = "https://github.com/crumbkit/board", rev = "<commit>" }
```

It depends on serde alone, so this builds nothing else of Board. Move the
pin to a newer commit for newer types; the compiler then shows what changed.

| Type | Reads or writes |
| --- | --- |
| `Report<D>`, `Printed<D>` | the envelope of every report, or a report or the failure document, told apart by `schema` |
| `Failure`, `Recovery`, `FieldValue` | `board/failure/v1` and each entry of a report's `failures` |
| `PutReport` | the data of `board/put-report/v2` |
| `File<R>` | each file in the data of `board/get-report/v2`; the default `R` skips records unread |
| `BatchLine<R>` | one line of `board put --batch` |
| `Run`, `Suite` | the records `runs/run/v1` and `runs/suite/v1` |

Each type names its id as a constant, such as `PutReport::SCHEMA` and
`Run::TYPE`. A report, a failure and a record ignore fields they do not
know, so a reader keeps working when Board adds one. A batch line refuses
them, as Board does.

```rust
use board_types::{Printed, PutReport};

let printed = r#"{"schema":"board/put-report/v2","data":{"commit":null,"writes":[]}}"#;
match serde_json::from_str::<Printed<PutReport>>(printed)? {
    Printed::Report(report) => assert!(report.data.writes.is_empty()),
    Printed::Failure(failure) => panic!("board refused: {}", failure.code),
}
# Ok::<(), serde_json::Error>(())
```

`Suite` leaves out inline `cases`, whose `input` and `expected` are any
JSON. Until a release, a type changes in place as Board's output does; a
new version of a document, such as a `put-report/v3`, gets a type beside
the old one.
