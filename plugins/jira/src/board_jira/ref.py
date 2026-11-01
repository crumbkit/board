"""The `ref` capability: write one Jira issue as a `jira/issue/v1` record (README.md)."""

import json
import re
import subprocess
from typing import Any, cast

from board_plugin.board import Board
from board_plugin.protocol import CallRequest, PluginError

PLUGIN = "jira"
ISSUE_TYPE = "jira/issue/v1"
# A project key, a hyphen and an issue number, as Jira writes keys. The schema's pattern matches the same keys.
KEY = r"[A-Z][A-Z0-9_]+-[1-9][0-9]*"
# The name before .atlassian.net in a Jira Cloud site's address.
SITE = r"[a-z0-9]([a-z0-9-]*[a-z0-9])?"
# How long acli may take to read one work item. A call has 30 seconds in all.
ACLI_SECONDS = 10
NO_SUMMARY = "acli printed no JSON work item with a summary"


def url_of(site: str, key: str) -> str:
    """The address of the issue's page."""
    return f"https://{site}.atlassian.net/browse/{key}"


def ref(request: CallRequest) -> dict[str, object]:
    """Write the issue at `/jira/<site>/<key>` unless a record is there, and report the record."""
    board = Board(request.board.root, PLUGIN)
    key = str(request.input["key"])
    if not re.fullmatch(KEY, key):
        why = (
            f"{key!r} is not a Jira issue key, which is a project key in capitals, a hyphen and a number; "
            "pass a key such as ACME-123"
        )
        raise PluginError("jira::key::invalid", why, {"key": key})
    site = str(request.input["site"])
    if not re.fullmatch(SITE, site):
        why = (
            f"{site!r} is not a Jira site name, the lowercase name before .atlassian.net; "
            "pass a site such as acme for acme.atlassian.net"
        )
        raise PluginError("jira::site::invalid", why, {"site": site})
    path = f"/jira/{site}/{key}"
    held = read(board, path)
    if held is not None:
        return reported(path, held, written=False, note=None)
    title, note = acli_title(site, key)
    record: dict[str, Any] = {"$type": ISSUE_TYPE, "site": site, "key": key, "url": url_of(site, key)}
    if title is not None:
        record["title"] = title
    line = {"path": path, "expect": "none", "record": record}
    board.document("put", "--batch", stdin=(json.dumps(line) + "\n").encode())
    return reported(path, record, written=True, note=note)


def read(board: Board, path: str) -> dict[str, Any] | None:
    """The record at `path`, or None when nothing is there."""
    [held] = board.get([path])
    if isinstance(held, PluginError):
        if held.fields.get("code") == "board::path::not_found":
            return None
        raise held
    record: dict[str, Any] = json.loads(held)
    return record


def reported(path: str, record: dict[str, Any], *, written: bool, note: str | None) -> dict[str, object]:
    """The output: the record's fields, whether this call wrote it, and why it has no title."""
    return {
        "path": path,
        "site": record["site"],
        "key": record["key"],
        "url": record["url"],
        "title": record.get("title"),
        "written": written,
        "title_note": note,
    }


def acli_title(site: str, key: str) -> tuple[str | None, str | None]:
    """The issue's title from `acli jira workitem view`, or None and why there is none.

    The plugin finds acli on the PATH Board passes it, so a test controls
    which acli runs, or that none does, through that PATH.
    """
    try:
        ran = subprocess.run(  # noqa: S603 - acli only reads the work item
            ["acli", "jira", "workitem", "view", key, "--fields", "summary", "--json"],  # noqa: S607 - acli is found on PATH
            capture_output=True,
            text=True,
            timeout=ACLI_SECONDS,
            check=False,
        )
    except FileNotFoundError:
        return None, "acli is not on PATH, so the record has no title"
    except subprocess.TimeoutExpired:
        return None, f"acli ran past {ACLI_SECONDS} s, so the record has no title"
    if ran.returncode != 0:
        last = (ran.stderr.strip().splitlines() or [""])[-1][:300]
        return None, f"acli exited {ran.returncode}: {last}"
    return title_from(ran.stdout, site)


def title_from(stdout: str, site: str) -> tuple[str | None, str | None]:
    """The summary in acli's JSON, or None and why. A work item read from another site gives no title."""
    try:
        parsed: object = json.loads(stdout)
    except ValueError:
        return None, NO_SUMMARY
    item: dict[str, Any] = cast("dict[str, Any]", parsed) if isinstance(parsed, dict) else {}
    fields: object = item.get("fields")
    summary: object = cast("dict[str, Any]", fields).get("summary") if isinstance(fields, dict) else None
    if not isinstance(summary, str) or not summary:
        return None, NO_SUMMARY
    source: object = item.get("self")
    if isinstance(source, str) and not source.startswith(f"https://{site}.atlassian.net/"):
        return None, f"acli is signed in to another site ({source})"
    return summary, None
