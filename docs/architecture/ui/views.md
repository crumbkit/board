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

## Checks

- `check` and `put` validate each `ui` record's shape. A view naming a kind
  outside the catalogue is `board::ui::unknown_kind`, at the kind's pointer.
- `put` of a binding also fits it to its view, read from the working tree:
  - a view that is missing is `ref::dangling`; one that is not a
    `ui/view/v1` record is `ref::wrong_type`, at `/view`;
  - slots that differ from the view's are `board::ui::slot_mismatch`, at
    `/slots`;
  - a `ref` that does not resolve, or names a type the kind does not draw, is
    `ref::dangling` or `ref::wrong_type`, at `/slots/<name>/ref`;
  - data, or a referenced record, that fails the kind's schema is
    `board::ui::slot_invalid`, at `/slots/<name>/data` or `/ref`.
- `check` does not fit bindings to views, and the index does not record a
  binding's `ref`s as references.
- A binding put in the same batch as its view is refused, because the view
  is read from the working tree before the batch writes. The reason says to
  put the view in an earlier put.
- The `record` kind links only strings that start with `/bindings/`, which
  open that binding's page; other paths show as text.

## Commands

- `board ui kinds` lists the catalogue; `--long` adds each schema, also
  with `--json`.
- `board ui template <kind>` prints a slot's fill to edit.
- `board ui open /bindings/<name>` refuses a path outside `/bindings/`, or one
  that holds no binding, with `board::ui::not_a_binding`, then opens the page as [serve.md](serve.md)
  says.
