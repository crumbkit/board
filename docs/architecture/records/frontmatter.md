# Frontmatter records

This file holds detail for docs/architecture.md, under Records. It says when
a Markdown file is a record, how Board reads its YAML, and how writes treat
it.

## When a Markdown file is a record

A Markdown file opens a frontmatter block when its first line is exactly
`---`, after one byte order mark if the file starts with one. The block ends
at the next line that is exactly `---` or `...`, and a file with no such line
has no frontmatter. Board reads the block only when one of its lines starts
with `$type:`, the key unquoted at the start of the line. It leaves any other block to its writer, such as a notes app
whose frontmatter need not be YAML Board reads, or a thematic break that
opens a document, and reads that file whole as a `file`. A block that
claims a type makes the file a record. Its kind is `record`, and it is
checked against `.board/types/<type>.schema.json` as a JSON record is. The
record's value is the frontmatter alone; the body, the text after the
closing line, is not part of it.

A Markdown record always names its `$type`. Board does not infer a type from
a file's directory, because the index stores each file's parse result by its
content and role, so the same bytes read the same way at any path. A plugin that wants the untyped Markdown files under a directory
finds them with SQL on `entries`.

A malformed `$type` is `record::bad_type`, as in JSON, and so is a claimed
block that is not a mapping with a top-level `$type` key. A `board/*` type in
frontmatter is `record::unknown_type`, because Board's own documents are JSON.

## How YAML becomes JSON

Board reads YAML 1.2 with the core schema (YAML 1.2.2, section 10.3.2),
through the saphyr-parser crate's event parser. Quoted and block scalars are
strings. A plain scalar is null (`~`, `null` or empty), a boolean (`true` or
`false`), an integer (decimal, `0o` octal or `0x` hexadecimal), a float, or
otherwise a string. YAML 1.1 forms such as `yes` and `off` are strings, and
`012` is the decimal 12. A date such as `2026-10-26` stays a string, because
YAML 1.2 has no timestamp type; a schema checks it with `"format": "date"`.

Frontmatter that JSON cannot hold is `board::frontmatter::unsupported`. That
covers an anchor or an alias to one, a tag, a key that is not a string, `.inf` or
`.nan`, a number that a double or a 64-bit integer cannot hold exactly, a
second document, and more than 127 nested mappings and sequences, which
`serde_json` cannot read back. YAML that does not
parse, a key repeated in one mapping, and flow nesting deeper than the
parser's own limit are `board::frontmatter::parse`.
Each message names the file's line. Both codes degrade the file, and both
apply only to a block that claims a type.

## What the index and the commands show

`bodies` holds the canonical JSON of the frontmatter, so `records.body` is
the frontmatter and `json_extract` reads its fields. `get` returns the file's
bytes and the frontmatter as its record. Links are read from the body after a
block that claims a type, and from the whole of any other file. `check` judges a
Markdown record's schema and references first and its body's links after.
`refs` lists the references under their JSON pointers, then the links as
written, and marks each link with `link: true` in JSON.

## Writes

`put --file` and a batch's `text` line check a Markdown file's frontmatter
before anything is written. Frontmatter that does not parse, a bad or
unknown type, and a record its schema refuses are each refused with their
code, and nothing is written. A dangling reference or link is written and
reported, as for a JSON record. Board writes the bytes as given and never
rewrites the YAML, so comments and key order stay as the writer left them.

## Share

[docs/plugins/share.md](../../plugins/share.md) says what a share copies from
a Markdown record.
