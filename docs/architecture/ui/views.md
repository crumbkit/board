# Views and bindings

This file holds detail for docs/architecture.md. It says how an agent puts
records on a page for the owner. [serve.md](serve.md) says how the page is
served.

`ui` is a reserved type namespace beside `board`: a board cannot define a
schema in either. Board knows three `ui` types.

| Type | Usual path | Holds |
| --- | --- | --- |
| `ui/view/v1` | `/views/<name>` | `slots`: a list of `{name, kind}`; names are unique |
| `ui/binding/v1` | `/bindings/<name>` | `view`, the view's logical path, and `slots`, an object filling each slot with `{"ref": <path>}` or `{"data": …}` |
| `ui/derived/v1` | anywhere | content written for a page: `from` (one or more logical paths), `agent`, `model` and `markdown` |

A slot's kind comes from a fixed catalogue in `board-model`. Each kind has a
JSON Schema and the record types a `ref` may name (`*` is any).

| Kind | Draws | Schema |
| --- | --- | --- |
| `markdown` | `ui/derived/v1` | `{markdown: string}` |
| `record` | any record | any object |
| `diff` | nothing; data only | `{patch: string}` |

## Checks

- `check` and `put` validate each `ui` record's shape. A view naming a kind
  outside the catalogue is `board::ui::unknown_kind`, at the kind's pointer.
- `check` and `put` fit a binding to its view by one rule. Each reads the
  view and every `ref` in the binding's own board. A binding that does not
  fit is written and reported degraded, never refused:
  - a view that is missing is `ref::dangling`; one that is not a
    `ui/view/v1` record is `ref::wrong_type`, at `/view`;
  - slots that differ from the view's are `board::ui::slot_mismatch`, at
    `/slots`;
  - a `ref` that does not resolve, or names a type the kind does not draw, is
    `ref::dangling` or `ref::wrong_type`, at `/slots/<name>/ref`;
  - data, or a referenced record, that fails the kind's schema is
    `board::ui::slot_invalid`, at `/slots/<name>/data` or `/ref`.
- `check` fits each binding again on every run, so it reports a binding
  whose view or records changed after the put. The index does not record a
  binding's `ref`s as references.
- A binding and its view can go in one batch, in either order: the fit runs
  after the batch writes.
- The `record` kind links only strings that start with `/bindings/`, which
  open that binding's page; other paths show as text.
- The `diff` kind's `patch` is a unified diff, as `git diff` or `diff -u`
  prints it.
  - Like `git apply`, it skips lines outside a file diff, such as a commit
    message.
  - `check` and `put` report the binding degraded, with
    `board::ui::slot_invalid` at `/slots/<name>/data/patch`, for:
    - a patch of more than 20,000 lines;
    - a patch with no file diff, or a hunk before any file header;
    - a malformed `@@` header;
    - a hunk that ends before its header's counts, a hunk line that starts
      with none of ` `, `+`, `-` or `\`, or a line past one side's count
      while the other side still has lines left.
  - A page shows such a slot as failed, with the same code.
  - Each line keeps a `\r` before its `\n`, so a change to line endings
    shows. Quoted git paths are unquoted. A combined diff of a merge
    (`diff --cc`) is not read.
  - The server sends the app the patch read into files, each with its old
    and new path, change counts, a binary flag, a copy flag, old and new
    modes, and hunks of numbered lines.
  - The server escapes hidden characters in paths, hunk headers and line
    text with `board_model::escaped`, as `\u{202e}`. Tabs in line text are
    kept. A line that held one carries `hidden`, and the app marks it.
  - The app shows a file list with change counts, then each file's
    unified hunks. Each line number links to its line.

## Commands

- `board ui kinds` lists the catalogue; `--long` adds each schema, also
  with `--json`.
- `board ui template <kind>` prints a slot's fill to edit.
- `board ui open /bindings/<name>` refuses a path outside `/bindings/`, or one
  that holds no binding, with `board::ui::not_a_binding`, then opens the page as [serve.md](serve.md)
  says.
