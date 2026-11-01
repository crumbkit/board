"""The pull request and issue records: their types, where they live, and finding one already written."""

import json
from typing import Any, cast

from board_plugin.board import Board, literal
from board_plugin.protocol import PluginError

PLUGIN = "github"
PULL_TYPE = "github/pull/v1"
ISSUE_TYPE = "github/issue/v1"
# `owner/repo#number`. A repository name may start with a dot, but `.` and `..` are not names.
REF_PATTERN = r"^[A-Za-z0-9][A-Za-z0-9-]*/(\.?[A-Za-z0-9_-][A-Za-z0-9._-]*)#[1-9][0-9]{0,9}$"
# The folder under `/gh/<owner>/<repo>/` that holds each type, as GitHub names it in its API.
FOLDERS = {PULL_TYPE: "pulls", ISSUE_TYPE: "issues"}


def parse(text: str) -> tuple[str, str, int]:
    """The owner, repository and number of a reference Board has checked against REF_PATTERN.

    Owner and repository are lowercased, since GitHub matches them without case, so one object has one path.
    """
    owner, _, rest = text.lower().partition("/")
    repo, _, digits = rest.partition("#")
    return owner, repo, int(digits)


def path(kind: str, owner: str, repo: str, number: int) -> str:
    """The logical path of the record of `kind` for `owner/repo#number`."""
    return f"/gh/{owner}/{repo}/{FOLDERS[kind]}/{number}"


def held(board: Board, owner: str, repo: str, number: int) -> tuple[str, dict[str, Any]] | None:
    """The path and body of the pull or issue record already written for `owner/repo#number`, or None.

    A record Board marks degraded is `github::record::degraded`, since its fields cannot be trusted.
    """
    paths = ", ".join(literal(path(kind, owner, repo, number)) for kind in FOLDERS)
    kinds = ", ".join(literal(kind) for kind in FOLDERS)
    rows = board.rows(
        f"SELECT logical, code, body FROM records WHERE logical IN ({paths}) AND type IN ({kinds}) ORDER BY logical"  # noqa: S608 - values are quoted by literal()
    )
    if not rows:
        return None
    logical, code, body = rows[0]
    if code is not None:
        raise PluginError(
            "github::record::degraded",
            f"the record at {logical} is degraded with {code}; run `board check {logical}` and fix it",
            {"path": logical, "code": code},
        )
    return str(logical), cast("dict[str, Any]", json.loads(body))


def write(board: Board, record: dict[str, Any]) -> str:
    """Write a pull or issue record with an expect-none put, leaving out an empty title, and return its path."""
    if not record.get("title"):
        record = {key: value for key, value in record.items() if key != "title"}
    at = path(record["$type"], record["owner"], record["repo"], record["number"])
    line = {"path": at, "expect": "none", "record": record}
    board.document("put", "--batch", stdin=(json.dumps(line) + "\n").encode())
    return at
