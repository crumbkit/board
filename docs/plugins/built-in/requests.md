# The requests plugin

This file holds detail for docs/plugins/built-in.md. `requests` is built
into Board. It shows a plugin's request to the owner as a thread, for
Board.app's request screen. It only reads.

## Records

- `requests/reply/v1`: the owner's reply to a pending effect.
  - `effect`: the id of the version it answers, as a `Digest`:
    `sha256:` and 64 lowercase hex digits. Board.app writes it in this
    form.
    It is an id, not a reference: the effect moves from
    `/effects/pending/` to `/effects/answered/` when answered.
  - `text`: what the owner says, not empty.
  - It lives at `/requests/replies/<effect id>/<time>` in the personal
    board. `<time>` is a sortable UTC time, such as `20261030T224454Z`.
  - The owner writes it with a plain `board put`. It is outside
    `effects/`, so it needs no signature. It approves nothing.

## `requests.thread`

`board --personal requests thread --effect <id>` takes the id of any
version, pending or answered.

- The versions are every `board/effect/v1` under `/effects/pending/` or
  `/effects/answered/` whose `board`, `plugin`, `op` and `path` equal the
  given effect's. A decide showing other content writes such a version.
- They come oldest first by `asked`. Effects without `asked` come first, in
  id order, as do ties.
- Each version has its id, `asked`, `record`, `text` and the owner's
  `answer` and `reason`, from `/effects/seen/<id>`, or null.
- `pending` is true under `/effects/pending/` with no answer. The newest
  pending version is `latest`, and replaces each older pending one.
- A file whose name is not its content's id is left out, as
  `board answer` refuses it. A pending effect wins over an answered copy
  of the same id.
- `replies` are the replies to those versions, ordered by file name. A
  reply counts only under `/requests/replies/<its effect>/` in this board,
  not in a mounted one.
- An id that is not 64 lowercase hex digits, or names no effect there, is
  refused with `requests::effect::not_found`.

## `requests.threads`

`board --personal requests threads` takes no input and returns
`{"threads": [...]}`, for a caller such as Board.app's request relay that
shows every pending request.

- A request is listed when at least one of its versions is `pending`. Each
  item is what `requests.thread` returns for any of its versions' ids.
- The threads are ordered by `board`, `plugin`, `op` and `path`, compared
  as strings. With no pending effect the list is empty.
- The call reads the effects once and the replies at most once, however
  many threads it lists.
- An input property is refused: Board's schema check refuses it with
  `board::plugin::bad_input`, and the plugin refuses one that reaches it
  with `requests::call::bad_input`.
