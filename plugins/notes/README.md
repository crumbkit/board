# notes

A Board plugin, built into Board as `crates/board-plugin-notes`, that appends to and reads notes in notes repos: git
repositories of Markdown notes in the layout of
[Reflect](https://github.com/team-reflect/reflect-open). `add` appends a
section to today's daily note or to a topic's note, and `daily` reads a
day's daily note. The plugin ships one record type, `notes/repo/v1`.

Run `board plugin enable notes` in a board, which writes the schema the
plugin ships to `.board/types/notes/`.

## Notes repos

A notes repo keeps Reflect's layout: `daily/YYYY-MM-DD.md`,
`notes/<slug>.md` and `assets/`, with `.reflect/` in `.gitignore`. The
board's own tree, and every mount, whose tree has a tracked file under
`daily/` at its root is a notes repo. Git tracks no empty directory, so a new
notes repo needs a file such as `daily/.gitkeep` to be found. Two mounts
of the same commit show as one notes repo. A mount may sit anywhere, such as
`/repos/notes-personal`. Keep one repository for each context, such as
personal notes or one employer's; which repos a machine reaches is decided
by the clones and credentials it holds.

Notes in the board's own tree are read and written with Board's ordinary
get and put, and are for agents only. Reflect syncs a graph only at the
root of a git repository, and a graph at the board's root would let Reflect
commit the whole board. So reading and writing notes on the phone needs a
dedicated notes repo, mounted in the board. Its clone must:

- be a board itself: hold `.board/board.json`, such as
  `{"$type": "board/config/v1"}`, and not set `commit_on_put` to false,
  because Board writes through a mount only into a board that commits each
  put;
- be checked out on a local branch, such as `main`, that follows a remote
  branch, because `board sync` pulls and pushes it.

The board mounts it with a mount that follows the local branch, such as
`repos/notes-personal.json`:

```json
{
  "$type": "board/mount/v1",
  "repo": "/Users/me/Notes/personal",
  "follow": "main",
  "commit": "<the clone's HEAD>"
}
```

## Routing a note

Each notes repo may hold `notes-repo.json` at its root, a `notes/repo/v1`
record naming the GitHub repositories its notes are about:

```json
{"$type": "notes/repo/v1", "covers": ["example-org/*", "crumbkit/board"], "personal": true}
```

`owner/*` covers every repository of that owner. The record is valid only
where its schema is, so run `board -C <clone> plugin enable notes` in a
mounted repo's clone before writing it, and `board mount update <mount>`
after, because a call reads each repo's record at its mount's pin.

A call picks one repo:

1. the one at `--repo`: `/` for the board's own tree, or a mount's path;
2. else the one repo whose record covers the caller's remote, matched in
   any case;
3. else, when there is one notes repo, that repo, if it is personal, or if
   there is no caller's remote and it has no record;
4. else the personal repo, the one whose record says `personal: true`.
   `add` writes a new file in its `inbox/`, with or without a remote.
   `daily` reads its daily note when there is no caller's remote, and is
   otherwise refused as `notes::repo::ambiguous`, so a session in a
   repository that no record covers never reads a personal note by default.

There is no caller's remote when the directory has no `origin`, or when its
git config cannot be read, so a lone repo whose record covers something
takes no call without a remote.

So a note from a remote that a repo's record excludes never reaches that
repo unless it is named. The caller's remote is the `origin` of the
directory Board was started in, read from its git config;
`--remote` overrides it, and `--repo` makes it unread. A GitHub remote is matched in any of its forms,
such as `https://github.com/example-org/home.git`,
`git@github.com:example-org/home.git`, an ssh host alias that starts with
`github.com`, such as `git@github.com-work:example-org/home.git`, or
`example-org/home`. A repo without a record covers nothing, even when it is
the only one, and a record that is not a valid `notes/repo/v1`,
such as one whose schema the repo lacks, refuses every routed call as
`notes::repo::invalid_record`.

An inbox note is named for the time, such as `inbox/2026-10-25-093000.md`,
or `inbox/2026-10-25-093000-2.md` when that name is taken. Its frontmatter
records `remote`, `candidates`, the repos that cover the remote, and
`topic`, each value written as JSON. `remote` is `owner/name` for GitHub
and otherwise the URL without its user and password, so a token in a remote
is never stored or shown in a message. It is a new file, so it is written
even when the clone cannot sync, and never parked. A put of it through a
mount whose HEAD moved is tried again at the same name, up to three times,
and is then left in the clone's working tree, uncommitted.

## The day

The person's day starts at `day_start`, such as `"07:00"`, in
`$XDG_CONFIG_HOME/board/config.json` (`~/.config/board/config.json` by
default), the file where Board lists known boards. Board refuses a value
that is not `HH:MM` as `board::config::invalid`. Without it the day starts at midnight. It is applied
when a call runs and never stored: with a day that starts at 07:00, a note
at 01:00 goes to yesterday's daily note under `## 01:00`, and `daily` reads
yesterday's note. The `board-repos` crate does the arithmetic.

## Capabilities

`add --text <markdown> [--repo <path>] [--remote <remote>] [--topic
<title>]` appends `## HH:MM` and the text, by the local time of the
time zone Board reads from `TZ` or the system, to `daily/YYYY-MM-DD.md` for the person's day. Text that is
only whitespace is refused; trailing whitespace is dropped and leading
indentation kept. With `--topic`, it appends to `notes/<slug>.md`, the slug
derived from the title as Reflect's `slugForTitle` derives it, and starts a
missing note with `# <title>`. It finds no note by title in any other file.
It prints the `repo`, the logical `path` written, the repo's `commit` that
holds it, `candidates`, `parked`, `unpushed` and `conflicts`. `commit` is null when the
note is in the working tree but not committed: the board sets
`commit_on_put` to false, HEAD moved during a put to the board's own tree,
or HEAD kept moving during an inbox put through a mount.

`daily [--repo <path>] [--remote <remote>] [--date YYYY-MM-DD]` prints the
day's note, today's by default. A read creates no note: `exists` is false
and `text` null when there is none. Reflect keeps private notes from AI, so
a note whose frontmatter has a `private` key has `private` true and `text`
null, unless every such key's value is plainly false (`false`, `no`, `off`
or `0`). The plugin reads the frontmatter's lines rather than parsing YAML,
and fails closed: a quoted key, a YAML tag or a repeated key hides the
note. It may hide a note Reflect shows, and never shows one Reflect hides.

## Sync, pins and conflicts

Board itself never merges contents. Every call to a mounted repo runs,
through Board:

1. `board -C <clone> sync` (with `--no-push` for `daily`), which fetches the
   remote branch, merges it into this clone, pushes and moves the working
   tree. Commits Reflect's desktop app made in the same clone are pushed
   like any other.
2. `board mount update <mount>`, which pins the mount to the clone's HEAD,
   so `board get` through the mount reads what the sync brought in. This
   commits the pin in the board, for a read too.
3. `board get` of the note through the mount, and for `add`, `board put
   --batch` of the appended text through the mount with `expect` set to the
   blob read. The put commits in the repo and moves the pin.
4. For `add`, another `board -C <clone> sync` to push, and another pin.

The board's own tree is never synced, because `board sync` there would
push the whole board: `unpushed` and `unsynced` are
`notes::repo::not_synced`, `daily`'s `commit` is null because it reads the
working tree, and `add`'s `commit` is the put's. A put there that Board
refuses with `board::put::head_moved` has already written the file, so it
is not tried again.

The phone's text is merged because the sync in step 1 brings it in before
the plugin appends. When the put is refused because the note or the clone
moved after the read (`board::put::stale` or `board::put::head_moved`), the
plugin pins, reads and appends again, up to three times. Three things park
the text: a note that keeps moving, a note holding an edit Reflect has not
committed (`board::mount::stale`, which no new read clears), and a first
sync refused with `board::sync::edited`. The
text is then appended to the sibling `<name>.agent.md`, such as
`daily/2026-10-24.agent.md`, and `parked` names the code. Any other refusal
of the first sync, such as no network, refuses the call before anything is
written. Once the text is committed, a refused push in step 4 never refuses
the call: `unpushed` names the code, because a retry would add the text
twice.

Board's sync merges with a commit of its own and leaves no merge or rebase
in progress. When the phone pushes a change to the same note between step 1
and step 4, the push sync merges the two line by line. Changes to different
parts of the note both survive, and so do two appends at the end of the
note: the phone's text comes first and the plugin's after it, with no
conflict. When both changed or deleted the same lines, the note holds the
phone's text, the plugin's text stays in the commit it made, and a
`board/conflict/v1` record under `/conflicts/` names both versions. The call
reports nothing parked or unpushed in either case, and `conflicts` lists the
paths of the records that name the note written (an empty list when none
does), so a caller sees when its text lost.

Each sync and pin has git's deadline of 60 seconds.

## Codes

- `notes::repo::not_found`: no notes repo is at `--repo`, or there is none;
  `repos` lists the notes repos.
- `notes::repo::no_personal`: no single repo takes the note, and not exactly
  one is marked personal; `repos` lists the notes repos and `personal`
  those marked.
- `notes::repo::ambiguous`: `daily` found no single repo for the remote;
  `candidates` lists those covering it and `repos` all of them.
- `notes::repo::invalid_record`: a notes repo's record is not a valid
  `notes/repo/v1`; `path` names it, and `code` and `status` say why.
- `notes::note::busy`: the `.agent.md` file also changed after it was read,
  or ten inbox names were taken; `path` names it, and nothing was written.
  Call again, and if the refusal persists, commit or discard the edit not
  committed in the clone.
- `notes::note::not_text`: the note is not UTF-8 text, so nothing can be
  appended to it; `path` names it.
- `notes::board::refused`: Board refused, with Board's code in `code`.
- `notes::call::unknown_capability`.

## Tests

`crates/board-cli/src/tests/built_in/notes.rs` and its modules run the
`board` binary in process against a board that mounts a notes repo, the
repo's bare remote, and a second clone standing in for the phone. They show
that the phone's push is merged before the append, that a commit Reflect
made in the clone is kept and pushed, that two appends at the end of one
note both survive, that an overlapping edit is named in `conflicts`, that
an uncommitted edit and a phone push over an uncommitted edit each park the
text, and that a refused push after the commit is reported. They show the routing
rules above, the inbox's names and frontmatter, and the day across UTC
offsets and `day_start`. The crate's own tests write through a real mount
that a second Board writes to between the read and the put.
`crates/board-cli/tests/notes.rs` runs the built binary with `TZ` set. No
test runs Reflect, so none shows how Reflect merges a commit left unpushed
by the race above, or whether Reflect shows `inbox/`.
