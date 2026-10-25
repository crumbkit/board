"""The `add` and `daily` capabilities, and the parts of Reflect's layout they need (README.md)."""

import datetime as dt
import json

from board_plugin.board import Board
from board_plugin.day import day_start, today
from board_plugin.protocol import CallRequest, PluginError
from board_plugin.repos import caller_remote, shown_remote
from board_plugin.slug import slug
from board_plugin.writing import ATTEMPTS, EDITED, HEAD_MOVED, MOVED, create, landed, refused

from board_notes.repo import NOT_SYNCED, Repo, choose

PLUGIN = "notes"
# How many names an inbox note tries, `<time>.md`, then `<time>-2.md` and on, when notes share a second.
INBOX_NAMES = 10
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
        stem, text = f"inbox/{now:%Y-%m-%d-%H%M%S}", inbox(remote, choice.candidates, topic) + section
        path, commit = create(repo, stem, text, names=INBOX_NAMES, busy="notes::note::busy")
    else:
        path = f"notes/{slug(topic)}.md" if topic else f"daily/{today(now, day_start()).isoformat()}.md"
        parked, commit = (synced, None) if synced else append(repo, path, f"# {topic}\n\n" if topic else "", section)
        if parked:
            path = path.removesuffix(".md") + ".agent.md"
            # The sibling takes no title, so Reflect never finds two notes of one title.
            busy, commit = append(repo, path, "", section)
            if busy:
                why = (
                    f"{repo.logical(path)} also changed after it was read, so the note was not added; call again, "
                    "and if this persists, commit or discard the edit not committed in its clone"
                )
                raise PluginError("notes::note::busy", why, {"path": repo.logical(path)})
    commit, unpushed = landed(repo, commit)
    return {
        "repo": repo.root,
        "path": repo.logical(path),
        "commit": commit,
        "candidates": choice.candidates,
        "parked": parked,
        "unpushed": unpushed,
    }


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


def inbox(remote: str | None, candidates: list[str], topic: str | None) -> str:
    """The frontmatter of a note in the inbox: the remote it came from, the repos it may belong to, and its topic."""
    fields = {"remote": None if remote is None else shown_remote(remote), "candidates": candidates, "topic": topic}
    # JSON is YAML, so each value is written as JSON and needs no quoting rules of its own.
    lines = [f"{key}: {json.dumps(value, ensure_ascii=False)}" for key, value in fields.items() if value is not None]
    return "---\n" + "\n".join(lines) + "\n---\n\n"


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
