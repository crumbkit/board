# notes

A Board plugin that appends to and reads notes in a vault: a git repository
of Markdown notes in the layout of [Reflect](https://github.com/team-reflect/reflect-open),
mounted in a board at `/vaults/<name>`. `add` appends a section to today's
daily note or to a topic's note, and `daily` reads a day's daily note. The
plugin ships no record types.

Install it from this checkout:

    uv sync --frozen --no-dev
    ln -s "$PWD" "${XDG_DATA_HOME:-$HOME/.local/share}/board/plugins/notes"

Then run `board plugin enable notes` in a board.

## A vault

A vault is one git repository for each context, such as personal notes or
one employer's. Which vaults a machine can reach is decided by the clones and
credentials it holds. The vault's clone must:

- be a board itself: hold `.board/board.json`, such as
  `{"$type": "board/config/v1"}`, and not set `commit_on_put` to false,
  because Board writes through a mount only into a board that commits each
  put;
- be checked out on a local branch, such as `main`, that follows a remote
  branch, because `board sync` pulls and pushes it;
- keep Reflect's layout: `daily/YYYY-MM-DD.md`, `notes/<slug>.md` and
  `assets/`, with `.reflect/` in `.gitignore`.

The board mounts it at `/vaults/<name>` with a mount that follows the local
branch, such as `vaults/personal.json`:

```json
{
  "$type": "board/mount/v1",
  "repo": "/Users/me/Notes/personal",
  "follow": "main",
  "commit": "<the clone's HEAD>"
}
```

A call names the vault with `--vault <name>`, or leaves it out when only one
is mounted.

## Capabilities

`add --text <markdown> [--vault <name>] [--topic <title>]` appends
`## HH:MM` and the text, by the local time of the caller's `TZ`, to
`daily/YYYY-MM-DD.md` for today's local date. Board passes `TZ` through to
the plugin. Text that is only whitespace is refused; trailing whitespace is
dropped and leading indentation kept. With `--topic`, it appends to
`notes/<slug>.md`, the slug derived from the title as Reflect's
`slugForTitle` derives it, and starts a missing note with `# <title>`. It
finds no note by title in any other file. It prints the logical `path`
written, the vault's `commit` that holds it, `parked` and `unpushed`.

`daily [--vault <name>] [--date YYYY-MM-DD]` prints the day's note, today's
by default. A read creates no note: `exists` is false and `text` null when
there is none. Reflect keeps private notes from AI, so a note whose
frontmatter has a `private` key has `private` true and `text` null, unless
every such key's value is plainly false (`false`, `no`, `off` or `0`). The
plugin reads the frontmatter's lines rather than parsing YAML, and fails
closed: a quoted key, a YAML tag or a repeated key hides the note. It may
hide a note Reflect shows, and never shows one Reflect hides.

## Sync, pins and conflicts

Board itself never merges contents. Every call runs, through Board:

1. `board -C <clone> sync` (with `--no-push` for `daily`), which fetches the
   remote branch, replays this clone's own commits onto it, pushes and moves
   the working tree. Commits Reflect's desktop app made in the same clone
   are replayed and pushed like any other.
2. `board mount update /vaults/<name>`, which pins the mount to the clone's
   HEAD, so `board get` through the mount reads what the sync brought in.
   This commits the pin in the board, for a read too.
3. `board get` of the note through the mount, and for `add`, `board put
   --batch` of the appended text through the mount with `expect` set to the
   blob read. The put commits in the vault and moves the pin.
4. For `add`, another `board -C <clone> sync` to push, and another pin.

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

The plugin never runs git itself, so it leaves no merge or rebase behind.
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

- `notes::vault::not_found`: nothing is mounted at `/vaults/<name>`, or no
  vault is mounted when `--vault` is left out; `vaults` lists the mounted
  names.
- `notes::vault::ambiguous`: several vaults are mounted and `--vault` is
  left out.
- `notes::vault::not_a_mount`: the record at `/vaults/<name>` mounts no
  other repository; `vault` names it.
- `notes::note::busy`: the `.agent.md` file also holds an edit not
  committed, or kept changing; `path` names it, and nothing was written.
- `notes::note::not_text`: the note is not UTF-8 text, so nothing can be
  appended to it; `path` names it.
- `notes::board::refused`: Board refused, with Board's code in `code`.
- `notes::call::unknown_capability`, `notes::protocol::bad_request`.

## Tests

The suite runs the real `board` against a board that mounts a vault, the
vault's bare remote, and a second clone standing in for the phone. It shows
that a note reaches the remote in the file for the date in the caller's
`TZ`, that the phone's push is merged before the append, and that a commit
Reflect made in the clone is kept and pushed. It shows that an uncommitted
edit, a sync conflict and a phone push over an uncommitted edit each park
the text without leaving a merge, that a note and sibling both holding
edits are busy, and that a refused push after the commit is reported, not
raised. Four adds to one note run at once, and each section lands exactly
once. They usually collide, but no test forces `board::put::stale`.
It does not run Reflect, so it does not show how Reflect merges a commit
left unpushed by the race above. The slug cases were checked against
`slugForTitle` on every code point.
