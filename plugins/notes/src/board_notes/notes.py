"""The `add` and `daily` capabilities, and the parts of Reflect's layout they need.

A daily note is `daily/YYYY-MM-DD.md` by this machine's local date, and a
topic's note is `notes/<slug>.md`, its slug derived from the title as Reflect
derives it. `add` appends a `## HH:MM` section. It syncs the vault first, so
it appends to what the phone pushed, and again after, to push. When the note
moved meanwhile it reads it again and appends to the new text. When the vault
cannot sync, the note holds an edit Reflect has not committed, or the note
keeps moving, the section goes to the sibling `<name>.agent.md` instead,
which no one else writes, and `parked` names the reason.
"""

import datetime as dt
import re
import unicodedata
from typing import cast

from board_plugin.board import Board
from board_plugin.protocol import CallRequest, PluginError

from board_notes.vault import Vault

PLUGIN = "notes"
# Refusals of a write because the note or the clone moved after the read; the write is tried again on a new read.
MOVED = ("board::put::stale", "board::put::head_moved")
# The refusal of a write over an edit not committed in the clone, which no new read clears.
EDITED = "board::mount::stale"
ATTEMPTS = 3
# Reflect's slugForTitle (packages/core/src/markdown/slug.ts), whose golden corpus freezes it.
SLUG_CHARS = 60
# JavaScript's `\s`, which differs from Python's: it holds U+FEFF, and not U+001C to U+001F or U+0085.
SEPARATOR = re.compile("[\t\n\v\f\r \u00a0\u1680\u2000-\u200a\u2028\u2029\u202f\u205f\u3000\ufeff_-]+")
RESERVED = {"con", "prn", "aux", "nul", *(f"com{n}" for n in range(1, 10)), *(f"lpt{n}" for n in range(1, 10))}
# The values of `private` that plainly show a note; any other value hides it.
FALSE = ("false", "no", "off", "0")


def add(request: CallRequest) -> dict[str, object]:
    """Append the text to today's daily note, or to the topic's note."""
    vault = Vault.find(Board(request.board.root, PLUGIN), _optional(request, "vault"))
    now = dt.datetime.now().astimezone()
    topic = _optional(request, "topic")
    path = f"notes/{slug(topic)}.md" if topic else f"daily/{now.date().isoformat()}.md"
    section = f"## {now:%H:%M}\n\n{str(request.input['text']).rstrip()}\n"
    first = f"# {topic}\n\n" if topic else ""
    parked = vault.sync(push=True) or append(vault, path, first, section)
    if parked:
        path = path.removesuffix(".md") + ".agent.md"
        # The sibling takes no title, so Reflect never finds two notes of one title.
        if append(vault, path, "", section):
            why = f"{vault.logical(path)} also holds an edit not committed, or kept changing; nothing was written"
            raise PluginError("notes::note::busy", why, {"path": vault.logical(path)})
    # The note is committed now, so a refused push is reported, not raised: a retry would add the note twice.
    try:
        unpushed = vault.sync(push=True)
    except PluginError as error:
        unpushed = str(error.fields.get("code", error.code))
    return {
        "vault": vault.mount,
        "path": vault.logical(path),
        "commit": vault.pin(),
        "parked": parked,
        "unpushed": unpushed,
    }


def append(vault: Vault, path: str, first: str, section: str) -> str | None:
    """Append `section` to the note at `path`, starting a new one with `first`: None, or why it was not written."""
    moved = None
    for _ in range(ATTEMPTS):
        vault.pin()
        note = vault.read(path)
        text = note.text.rstrip("\n") + "\n\n" + section if note.text else first + section
        try:
            vault.write(path, text, note.blob)
        except PluginError as error:
            moved = refused(error)
            if moved == EDITED:
                return EDITED
            if moved not in MOVED:
                raise
        else:
            return None
    return str(moved)


def refused(error: PluginError) -> object:
    """The code Board refused a write with. A batch reports its one line's refusal in `refusals`."""
    refusals = error.fields.get("refusals")
    if isinstance(refusals, list) and len(cast("list[object]", refusals)) == 1:
        return cast("list[dict[str, object]]", refusals)[0].get("code")
    return error.fields.get("code")


def daily(request: CallRequest) -> dict[str, object]:
    """The day's daily note, unless it is private. A read creates no note."""
    vault = Vault.find(Board(request.board.root, PLUGIN), _optional(request, "vault"))
    date = _optional(request, "date") or dt.datetime.now().astimezone().date().isoformat()
    path = f"daily/{date}.md"
    unsynced = vault.sync(push=False)
    commit = vault.pin()
    note = vault.read(path)
    hidden = note.text is not None and private(note.text)
    return {
        "vault": vault.mount,
        "path": vault.logical(path),
        "exists": note.text is not None,
        "private": hidden,
        "text": None if hidden else note.text,
        "commit": commit,
        "unsynced": unsynced,
    }


def slug(title: str) -> str:
    """The file name Reflect gives a note titled `title`, without `.md`.

    It drops a letter or number newer than this Python's Unicode database
    knows, where Reflect's JavaScript, on a newer database, keeps it.
    """
    folded = unicodedata.normalize("NFC", title).lower()
    kept = "".join(c for c in folded if unicodedata.category(c)[0] in "LN" or SEPARATOR.match(c))
    capped = SEPARATOR.sub("-", kept).strip("-")[:SLUG_CHARS].strip("-")
    if not capped:
        return "untitled"
    return f"{capped}-note" if capped in RESERVED else capped


def private(text: str) -> bool:
    """Whether the note may be private: its frontmatter has a `private` key whose value is not plainly false.

    This reads the frontmatter's lines, not its YAML, and fails closed: a
    quoted key, a tag such as `!!bool`, or a second `private` line hides the
    note, so it may hide one that Reflect shows and never shows one that
    Reflect hides.
    """
    lines = text.removeprefix("\ufeff").splitlines()
    if not lines or lines[0].rstrip() != "---":
        return False
    hidden = False
    for line in lines[1:]:
        if line.rstrip() == "---":
            break
        key, colon, value = line.partition(":")
        if colon and key.strip().strip("'\"").strip() == "private":
            flag = value.split(" #")[0].strip().strip("'\"").strip().lower()
            hidden = hidden or flag not in FALSE
    return hidden


def _optional(request: CallRequest, name: str) -> str | None:
    value = request.input.get(name)
    return None if value is None else str(value)
