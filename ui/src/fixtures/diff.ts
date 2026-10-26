import type { DiffData } from "../kinds/Diff";

/** One file with two hunks, as Board reads `git diff` for the app. */
export const change: DiffData = {
  files: [
    {
      old: "crates/board-ops/src/sync.rs",
      new: "crates/board-ops/src/sync.rs",
      added: 3,
      removed: 2,
      binary: false,
      hunks: [
        {
          header: "@@ -10,6 +10,7 @@ use crate::remote::Remote;",
          lines: [
            { change: "context", old: 10, new: 10, text: "" },
            { change: "removed", old: 11, text: "const RETRIES: u32 = 3;" },
            { change: "added", new: 11, text: "/// Measured on a remote that refuses twice before it answers." },
            { change: "added", new: 12, text: "const RETRIES: u32 = 5;" },
            { change: "context", old: 12, new: 13, text: "" },
            { change: "context", old: 13, new: 14, text: "pub(crate) fn sync(board: &Board) -> Result<Report, SyncError> {" },
            { change: "context", old: 14, new: 15, text: "    let remote = Remote::open(board)?;" },
          ],
        },
        {
          header: "@@ -40,4 +41,4 @@ pub(crate) fn sync(board: &Board) -> Result<Report, SyncError> {",
          lines: [
            { change: "context", old: 40, new: 41, text: "    for attempt in 0..RETRIES {" },
            { change: "removed", old: 41, text: "        if remote.push().is_ok() { break; }" },
            { change: "added", new: 42, text: "        if remote.push().is_ok() { return Ok(Report::pushed(attempt)); }" },
            { change: "context", old: 42, new: 43, text: "    }" },
            { change: "context", old: 43, new: 44, text: "}" },
          ],
        },
      ],
    },
  ],
};

/** A new, a renamed, a binary, a mode-only and a deleted file, and a file whose path
 * and one line held a bidi control, which Board sent escaped. */
export const files: DiffData = {
  files: [
    {
      new: "docs/sync.md",
      added: 2,
      removed: 0,
      binary: false,
      hunks: [
        {
          header: "@@ -0,0 +1,2 @@",
          lines: [
            { change: "added", new: 1, text: "# Sync" },
            { change: "added", new: 2, text: "A push is retried five times." },
          ],
        },
      ],
    },
    { old: "notes/retry.md", new: "notes/sync-retry.md", added: 0, removed: 0, binary: false, hunks: [] },
    { old: "ui/logo.png", new: "ui/logo.png", added: 0, removed: 0, binary: true, hunks: [] },
    {
      old: "bin/sync",
      new: "bin/sync",
      added: 0,
      removed: 0,
      binary: false,
      old_mode: "100644",
      new_mode: "100755",
      hunks: [],
    },
    {
      old: "scripts/old-sync.sh",
      added: 0,
      removed: 1,
      binary: false,
      hunks: [{ header: "@@ -1 +0,0 @@", lines: [{ change: "removed", old: 1, text: "board sync --retries 3" }] }],
    },
    {
      old: "src/admin\\u{202e}txt.rs",
      new: "src/admin\\u{202e}txt.rs",
      added: 1,
      removed: 1,
      binary: false,
      hunks: [
        {
          header: "@@ -1 +1 @@",
          lines: [
            { change: "removed", old: 1, text: "fn allowed() -> bool { false }" },
            { change: "added", new: 1, hidden: true, text: "fn allowed() -> bool { true } // \\u{202e} } false {" },
          ],
        },
      ],
    },
  ],
};
