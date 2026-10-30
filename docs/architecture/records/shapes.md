# Record shapes

This file holds detail for docs/architecture.md, under Typed references. It
lists the shape every record schema keeps, and what a writer of references
needs to know.

## Rules

1. Every record has a required `"$type": {"type": "string", "const": "<ns>/<kind>/v<n>"}`.
2. A field the writer may leave out is absent, never null, in records and
   capability inputs. In capability outputs every key is present and null
   means none.
3. A reference is `{"type": "string", "format": "board-ref"}`, plus
   `x-board-type`: a string for one type, a list for several, or nothing for
   any entry, Markdown files and mounts included.
4. A list of references puts the reference schema in `items`.
5. A reference sits only under `properties`, `patternProperties`,
   `additionalProperties`, `items`, `prefixItems`, `allOf` and local `$ref`.
   Board searches nothing else, and refuses a schema that marks a reference
   under any other keyword that applies subschemas, or behind a
   `$dynamicRef`, with `schema::invalid`.
6. References and URLs never share a field. URLs go in a field named `urls`,
   whose items match `^https?://`. A field that holds one URL is named for
   what it is, such as `pull_request`.
7. A field that names a repository is a reference to that repository's mount
   record at `/repos/<name>`, named for what it is, such as `mount` or
   `mounts`. A pinned version is a sibling field, such as `commit`. A field
   whose values are matched against GitHub remotes (`owner/name` or
   `owner/*`) is not a mount reference; it is named `remote`, or for what it
   matches, such as notes `covers`.
8. An enum is inline, `{"type": "string", "enum": [...]}`, in lowercase or
   kebab case. A union of scalars, such as a date or one of a few words, is
   allowed when it holds no reference.
9. Nested structs go in `$defs`.
10. Every record and nested object has `additionalProperties: false`, except a
    map whose values share one schema, and `tasks/task/v2`, which another
    tool co-writes.
11. Every property has a `description`. No property has a `title`.
12. No `$schema` key. Draft 2020-12, committed in canonical form.

`xtask/tests/shapes.rs` checks rules 1, 2, 5, 10, 11 and 12 on every committed
record schema. It lists the schemas that break a rule today, until their
plugins are ported.

## Writing references

- Find the records that refer to a path with `board refs`, never by
  comparing path text in SQL. A link or a mount alias reaches the same entry
  under another path.
- A record that fails its schema, for any reason, contributes no references.
- A reference into a mount is read at the commit the mount pins, so it
  dangles until `board mount update`.
- A record in a mounted board can refer only to its own board, never to the
  board that mounts it.
- A JSON record is named without `.json`; `/a.json` dangles where `/a`
  resolves. A trailing `/` is `ref::malformed`.
- `put` refuses only `ref::malformed`. It writes a dangling or wrong-type
  reference and reports it.
- In frontmatter, an empty YAML key is null. Leave the key out instead.
- A capability input marks a path with `format: board-path`, not
  `board-ref`, so `board call` reads it relative to the prefix.

## Generating schemas in Rust

The plans plugin generates its record schemas from structs with schemars,
through `board-plugin-plans/src/shape.rs`:

- `Tag<Self>` is the `$type` field; `Ref<T>` is a reference to a record type
  `T`, to a marker that lists several types, or to any entry when `T` is left
  out. Both are always inlined.
- `schema_of` removes null from every optional field and drops an integer's
  `format`.
- The committed schema files are goldens. A test fails when the generated
  schema differs by a byte; `SNAPSHOTS=overwrite` rewrites them.
