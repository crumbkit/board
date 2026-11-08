# Record URIs

This file holds detail for docs/architecture/paths.md. It says how a
`board://` URI names a record in a board that other people clone, such as a
team board.

A URI is `board://`, the stored form of the board's git origin, and the
record's logical path, such as `board://acme/team-board/tasks/fix-login`.
The stored form is the one repo records use (architecture/paths/repos.md):
`owner/name` for GitHub, and otherwise the URL without its user and
password, with a relative URL made absolute. Two clones of one board
therefore give the same URI. `board uri` keeps the origin's own spelling.
Board does not percent-decode a URI: each character is matched as written.
`board uri <path>` prints it for a record in the current board. A board
with no origin is refused with `board::uri::no_remote`.

## Resolution

Only `check`, `get`, `ls`, `refs` and `uri` take a URI in place of a path.
`ui open`, `decide --path` and plugin path flags do not. Board reads the known boards file (docs/architecture.md, Boards and
entries), and computes the stored form of each known board's origin. The
board whose origin is the longest prefix of what follows `board://`, ending
at a `/`, is the run's board, and the rest of the URI is the record's path,
read as a plain path is, so a trailing `/` is dropped. A GitHub remote
matches in any case, as GitHub does; any other remote matches exactly. The
run reads that board as `board -C <root>` would, from any directory. The
longest prefix
needs no separator between the remote and the path, because the known
boards file says which remotes exist.

| Code | When |
| --- | --- |
| `board::uri::not_found` | no known board has the URI's remote; the hint names the remote to clone and list in the known boards file |
| `board::uri::ambiguous` | two known boards have that origin; the message names both |

Both messages name any known board that could not be opened.
| `board::uri::invalid` | the text after `board://` is not a remote and a path, or the path is malformed |
| `board::uri::read_only` | `put` or `rm` is given a URI; write in the clone with `-C` |
| `board::cli::usage` | the URIs of one run name two boards, or a URI is given beside a plain path, `-C` or `--personal`, which would choose the board too |

Board reads the clone as it is on disk and never fetches. A record the clone
lacks is `board::path::not_found`, and when the path came from a URI its
hint adds that the clone may be behind and names
`board -C <clone> sync --no-push`.

`share.to` prints the URI of each record it copies, in `uris`, when the
destination has an origin.

A URI names no commit, is not stored in records, and has no link handler.
