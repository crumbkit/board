# notes

This plugin is still a Python program (docs/plugins/python.md). A new plugin
is a crate built into Board (docs/plugins/built-in/writing.md).

A Board plugin that appends to and reads notes in notes repos: git
repositories of Markdown notes in the layout of
[Reflect](https://github.com/team-reflect/reflect-open). `add` appends a
section to today's daily note or to a topic's note, and `daily` reads a
day's daily note. The plugin ships one record type, `notes/repo/v1`.

Install it from this checkout:

    uv sync --frozen --no-dev
    ln -s "$PWD" "${XDG_DATA_HOME:-$HOME/.local/share}/board/plugins/notes"

Then run `board plugin enable notes` in a board.

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

There is no caller's remote when the directory has no `origin`, or when git
fails or takes over ten seconds, so a lone repo whose record covers
something takes no call without a remote.

So a note from a remote that a repo's record excludes never reaches that
repo unless it is named. The caller's remote is the `origin` of the
directory Board was started in, which Board passes as `BOARD_CALLER_DIR`;
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
that is not `HH:MM` as `board::config::invalid` and passes it to the plugin
as `BOARD_DAY_START`. Without it the day starts at midnight. It is applied
when a call runs and never stored: with a day that starts at 07:00, a note
at 01:00 goes to yesterday's daily note under `## 01:00`, and `daily` reads
yesterday's note. The shared `board_plugin.day` module does the arithmetic.

## Capabilities

`add --text <markdown> [--repo <path>] [--remote <remote>] [--topic
<title>]` appends `## HH:MM` and the text, by the local time of the
caller's `TZ`, to `daily/YYYY-MM-DD.md` for the person's day. Text that is
only whitespace is refused; trailing whitespace is dropped and leading
indentation kept. With `--topic`, it appends to `notes/<slug>.md`, the slug
derived from the title as Reflect's `slugForTitle` derives it, and starts a
missing note with `# <title>`. It finds no note by title in any other file.
It prints the `repo`, the logical `path` written, the repo's `commit` that
holds it, `candidates`, `parked` and `unpushed`. `commit` is null when the
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
   remote branch, replays this clone's own commits onto it, pushes and moves
   the working tree. Commits Reflect's desktop app made in the same clone
   are replayed and pushed like any other.
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
sync refused with `board::sync::conflict` or `board::sync::edited`. The
text is then appended to the sibling `<name>.agent.md`, such as
`daily/2026-10-24.agent.md`, and `parked` names the code. Any other refusal
of the first sync, such as no network, refuses the call before anything is
written. Once the text is committed, a refused push in step 4 never refuses
the call: `unpushed` names the code, because a retry would add the text
twice.

The plugin runs git only to read the caller's `origin`, which changes
nothing, so it leaves no merge or rebase behind.
One race remains: when the phone pushes a change to the same note between
step 1 and step 4, the push sync refuses with `board::sync::conflict`, and
the commit stays in this clone with `unpushed` naming the code. Board cannot
drop an unpushed commit, so the plugin cannot redo it. Reflect's desktop
app merges it on its next pull, as it merges daily notes appended on two
devices, and pushes it. Replacing this needs Board to report the remote's
blob on a conflict and to let a sync drop a commit, which no other plugin
needs yet.

Board passes the plugin no `SSH_AUTH_SOCK`, so `board sync` cannot push to
a remote that authenticates through ssh-agent. Use an HTTPS remote with a
credential helper, such as `gh auth setup-git`.

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
- `notes::call::unknown_capability`, `notes::protocol::bad_request`.

## Tests

The suite runs the real `board` against a board that mounts a notes repo,
the repo's bare remote, and a second clone standing in for the phone. It
shows that a note reaches the remote in the file for the date in the
caller's `TZ`, that the phone's push is merged before the append, and that
a commit Reflect made in the clone is kept and pushed. It shows that an
uncommitted edit, a sync conflict and a phone push over an uncommitted edit
each park the text without leaving a merge, that a note and sibling both
holding edits are busy, and that a refused push after the commit is
reported, not raised. Four adds to one note run at once, and each section
lands exactly once. They usually collide, but no test forces
`board::put::stale`.

Other tests show that a mount without `daily/` is not a notes repo, that
the board's own `daily/` makes it one and is written without a sync, with a
null `commit` when the board does not commit puts, and that a put there
whose HEAD moved is written once while an inbox put through a mount is
tried again at its own name and left uncommitted when HEAD keeps moving.
They show that a note goes to the one
repo covering the remote of the directory Board ran in, that `--remote`
overrides it, that a note no single repo covers goes to the personal inbox
with its candidates, unparked even when the clone cannot sync, and to a
second name when the first is taken, and that the only repo refuses a
remote its record excludes, or any remote when it has no record. They show
that a token in a remote is neither stored nor shown, and that `daily`
without a remote reads the personal note. They show that a record without its schema is
refused by name, that `daily` is refused when no single repo covers the
remote, and that with a day starting at 07:00 a note at 01:00, in a time
zone chosen so the call runs then, lands in yesterday's daily note. The
suite does not run Reflect, so it does not show how Reflect merges a commit
left unpushed by the race above, or whether Reflect shows `inbox/`. The
slug cases were checked against `slugForTitle` on every code point.
