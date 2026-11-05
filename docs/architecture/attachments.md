# Attachments

This file holds detail for docs/architecture.md. It says how a board keeps
bytes outside its tree: large and binary files go to an object store, and the
tree holds a record that names them.

## The store

`.board/board.json` names the store:

```json
"store": {"backend": "directory", "location": "/Users/me/objects"}
```

`backend` is one of the backends Board offers; `directory` is the only one so
far, and its `location` is a directory, absolute or relative to the board's
root. A board without a `store` writes every file into the tree. Any
credential a backend needs comes from the owner's machine configuration, never
from a record.

## Putting a file

`board put <path> --file` with a store uploads a file over 1 MiB, or one that
is not UTF-8, under its SHA-256 digest: the `directory` backend writes
`<location>/<hex digest>` through a temporary file and a rename, before the
put takes the board's lock. Then the put writes a `board/attachment/v1` record
at `<path>.json` in the file's place, with the ordinary checks, `--expect` and
commit:

```json
{
  "$type": "board/attachment/v1",
  "digest": "sha256:9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08",
  "size": 5242880,
  "media_type": "image/png",
  "location": "/Users/me/objects/9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08"
}
```

`media_type` follows the file's extension, or is `application/octet-stream`.
A smaller UTF-8 file is written into the tree as before, and so is every
`text` line of a batch. The record is checked by a Rust type, so a record
missing one of its four fields, or holding another, is `record::invalid` in
`put` and `check` alike. A store the upload cannot write is
`board::put::failed`, and nothing is written.

## Reading

`check`, `get`, `ls`, `refs` and `query` read the record and never the store,
so they pass without the store or the network. `board get <path> --fetch`
reads the bytes at the record's `location`, hashes them, and prints them when
the digest matches. A location that holds nothing is
`board::attachment::missing`, bytes that hash to another digest are
`board::attachment::mismatch`, and either hint says to put the file again,
which uploads it under its digest and rewrites the record. A file that is no
attachment prints with `--fetch` as it does without.
