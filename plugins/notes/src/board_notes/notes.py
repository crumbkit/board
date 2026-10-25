"""The `add` and `daily` capabilities, and the parts of Reflect's layout they need (README.md)."""

import datetime as dt
import json
import re
import unicodedata
from typing import cast

from board_plugin.board import Board
from board_plugin.day import day_start, today
from board_plugin.protocol import CallRequest, PluginError
from board_plugin.repos import caller_remote, shown_remote

from board_notes.repo import NOT_SYNCED, Repo, choose

PLUGIN = "notes"
STALE = "board::put::stale"
# A put to the board's own tree refused because HEAD moved while it wrote: the file is written, and not committed.
HEAD_MOVED = "board::put::head_moved"
# Refusals of a write because the note or the clone moved after the read; the write is tried again on a new read.
MOVED = (STALE, HEAD_MOVED)
# The refusal of a write over an edit not committed in the clone, which no new read clears.
EDITED = "board::mount::stale"
ATTEMPTS = 3
# How many names an inbox note tries, `<time>.md`, then `<time>-2.md` and on, when notes share a second.
INBOX_NAMES = 10
# Reflect's slugForTitle (packages/core/src/markdown/slug.ts), whose golden corpus freezes it.
SLUG_CHARS = 60
# JavaScript's `\s`, which differs from Python's: it holds U+FEFF, and not U+001C to U+001F or U+0085.
SEPARATOR = re.compile("[\t\n\v\f\r \u00a0\u1680\u2000-\u200a\u2028\u2029\u202f\u205f\u3000\ufeff_-]+")
RESERVED = {"con", "prn", "aux", "nul", *(f"com{n}" for n in range(1, 10)), *(f"lpt{n}" for n in range(1, 10))}
# The values of `private` that plainly show a note; any other value hides it.
FALSE = ("false", "no", "off", "0")


def add(request: CallRequest) -> dict[str, object]:
    """Append the text to today's daily note, to the topic's note, or to a new note in the inbox."""
    name, topic = _optional(request, "repo"), _optional(request, "topic")
    remote = _remote(request, name)
    choice = choose(Board(request.board.root, PLUGIN), name, remote, inbox=True)
    repo = choice.repo
    now = dt.datetime.now().astimezone()
    section = f"## {now:%H:%M}\n\n{str(request.input['text']).rstrip()}\n"
    own = repo.clone is None
    synced = None if own else repo.sync(push=True)
    parked = None
    if choice.candidates is not None:
        # An inbox note is a new file, which nothing else writes, so it is never parked.
        path, commit = create(repo, f"inbox/{now:%Y-%m-%d-%H%M%S}", inbox(remote, choice.candidates, topic) + section)
    else:
        path = f"notes/{slug(topic)}.md" if topic else f"daily/{today(now, day_start()).isoformat()}.md"
        parked, commit = (synced, None) if synced else append(repo, path, f"# {topic}\n\n" if topic else "", section)
        if parked:
            path = path.removesuffix(".md") + ".agent.md"
            # The sibling takes no title, so Reflect never finds two notes of one title.
            busy, commit = append(repo, path, "", section)
            if busy:
                why = f"{repo.logical(path)} also holds an edit not committed, or kept changing; nothing was written"
                raise PluginError("notes::note::busy", why, {"path": repo.logical(path)})
    return {
        "repo": repo.root,
        "path": repo.logical(path),
        "commit": None if commit is None else repo.pin() or commit,
        "candidates": choice.candidates,
        "parked": parked,
        "unpushed": NOT_SYNCED if own else push(repo),
    }


def push(repo: Repo) -> str | None:
    """Push the clone: None, or the code of the refusal. The note is committed, so a retry would add it twice."""
    try:
        return repo.sync(push=True)
    except PluginError as error:
        return str(error.fields.get("code", error.code))


def append(repo: Repo, path: str, first: str, section: str) -> tuple[str | None, str | None]:
    """Append `section` to the note at `path`, starting a new one with `first`.

    It returns why the section was not written, or None, and the commit that
    holds the write, None when the board's own tree left it uncommitted.
    """
    moved = None
    for _ in range(ATTEMPTS):
        repo.pin()
        note = repo.read(path)
        text = note.text.rstrip("\n") + "\n\n" + section if note.text else first + section
        try:
            return None, repo.write(path, text, note.blob)
        except PluginError as error:
            moved = refused(error)
            if moved == HEAD_MOVED and repo.clone is None:
                return None, None
            if moved == EDITED:
                return EDITED, None
            if moved not in MOVED:
                raise
    return str(moved), None


def create(repo: Repo, stem: str, text: str) -> tuple[str, str | None]:
    """Write `text` to a new note at `<stem>.md`, or at `<stem>-2.md` and on when one is there: its path and commit."""
    number, moved = 1, 0
    while number <= INBOX_NAMES:
        path = f"{stem}.md" if number == 1 else f"{stem}-{number}.md"
        repo.pin()
        try:
            return path, repo.write(path, text, None)
        except PluginError as error:
            code = refused(error)
            if code == STALE and moved:
                # The put whose HEAD moved wrote this file, so it is this note.
                return path, None
            if code == HEAD_MOVED:
                moved += 1
                if repo.clone is None or moved == ATTEMPTS:
                    # Board wrote the file before HEAD moved, so the note is in the working tree, uncommitted.
                    return path, None
            elif code == STALE:
                number += 1
            else:
                raise
    why = f"{repo.logical(stem)}.md and the {INBOX_NAMES - 1} names after it are taken; nothing was written; call again"
    raise PluginError("notes::note::busy", why, {"path": repo.logical(f"{stem}.md")})


def inbox(remote: str | None, candidates: list[str], topic: str | None) -> str:
    """The frontmatter of a note in the inbox: the remote it came from, the repos it may belong to, and its topic."""
    fields = {"remote": None if remote is None else shown_remote(remote), "candidates": candidates, "topic": topic}
    # JSON is YAML, so each value is written as JSON and needs no quoting rules of its own.
    lines = [f"{key}: {json.dumps(value, ensure_ascii=False)}" for key, value in fields.items() if value is not None]
    return "---\n" + "\n".join(lines) + "\n---\n\n"


def refused(error: PluginError) -> object:
    """The code Board refused a write with. A batch reports its one line's refusal in `refusals`."""
    refusals = error.fields.get("refusals")
    if isinstance(refusals, list) and len(cast("list[object]", refusals)) == 1:
        return cast("list[dict[str, object]]", refusals)[0].get("code")
    return error.fields.get("code")


def daily(request: CallRequest) -> dict[str, object]:
    """The day's daily note in the repo the caller's remote routes to, unless it is private. A read creates no note."""
    name = _optional(request, "repo")
    repo = choose(Board(request.board.root, PLUGIN), name, _remote(request, name), inbox=False).repo
    date = _optional(request, "date") or today(dt.datetime.now().astimezone(), day_start()).isoformat()
    path = f"daily/{date}.md"
    own = repo.clone is None
    unsynced = NOT_SYNCED if own else repo.sync(push=False)
    commit = repo.pin()
    note = repo.read(path)
    hidden = note.text is not None and private(note.text)
    return {
        "repo": repo.root,
        "path": repo.logical(path),
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


def _remote(request: CallRequest, name: str | None) -> str | None:
    """The remote to route by: `--remote`, else the caller's origin; not needed when `--repo` names the repo."""
    if name is not None:
        return None
    return _optional(request, "remote") or caller_remote()


def _optional(request: CallRequest, name: str) -> str | None:
    value = request.input.get(name)
    return None if value is None else str(value)
