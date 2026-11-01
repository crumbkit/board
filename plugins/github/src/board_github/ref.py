"""The `ref` capability: record one pull request or issue, asking GitHub only when it is not yet recorded."""

import json

from board_plugin.board import Board
from board_plugin.protocol import CallRequest, PluginError

from board_github import gh
from board_github.objects import ISSUE_TYPE, PLUGIN, PULL_TYPE, held, path

# `owner/repo#number`. A repository name may start with a dot, but `.` and `..` are not names.
REF_PATTERN = r"^[A-Za-z0-9][A-Za-z0-9-]*/(\.?[A-Za-z0-9_-][A-Za-z0-9._-]*)#[1-9][0-9]{0,9}$"


def ref(request: CallRequest) -> dict[str, object]:
    """Write the record for `owner/repo#number` with an expect-none put, or report the one already there.

    Board has checked `ref` against REF_PATTERN.
    """
    text = str(request.input["ref"])
    owner, _, rest = text.partition("/")
    repo, _, digits = rest.partition("#")
    number = int(digits)
    board = Board(request.board.root, PLUGIN)
    found = held(board, owner, repo, number)
    if found is not None:
        return answer(*found, written=False)
    issue = gh.api(f"repos/{owner}/{repo}/issues/{number}")
    if issue is None:
        raise PluginError(
            "github::ref::not_found",
            f"GitHub has no pull request or issue {text}, or gh's account cannot see it; check the number "
            f"with `gh issue view {number} --repo {owner}/{repo}`",
            {"ref": text},
        )
    url, title = issue.get("html_url"), issue.get("title")
    if not isinstance(url, str) or not url.startswith("https://"):
        raise gh.failed(f"repos/{owner}/{repo}/issues/{number}", "printed no html_url")
    kind = PULL_TYPE if "pull_request" in issue else ISSUE_TYPE
    record: dict[str, object] = {"$type": kind, "owner": owner, "repo": repo, "number": number, "url": url}
    if isinstance(title, str) and title:
        record["title"] = title
    at = path(kind, owner, repo, number)
    line = {"path": at, "expect": "none", "record": record}
    board.document("put", "--batch", stdin=(json.dumps(line) + "\n").encode())
    return answer(at, record, written=True)


def answer(at: str, record: dict[str, object], *, written: bool) -> dict[str, object]:
    """What `ref` reports about the record at `at`."""
    return {"path": at, "type": record["$type"], "written": written, "url": record["url"], "title": record.get("title")}
