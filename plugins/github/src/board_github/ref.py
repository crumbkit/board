"""The `ref` capability: record one pull request or issue, asking GitHub only when it is not yet recorded."""

import json

from board_plugin.board import Board
from board_plugin.protocol import CallRequest, PluginError

from board_github import gh
from board_github.objects import ISSUE_TYPE, PLUGIN, PULL_TYPE, held, parse, path


def ref(request: CallRequest) -> dict[str, object]:
    """Write the record for `owner/repo#number` with an expect-none put, or report the one already there."""
    text = str(request.input["ref"])
    owner, repo, number = parse(text)
    board = Board(request.board.root, PLUGIN)
    found = held(board, owner, repo, number)
    if found is not None:
        return answer(*found, written=False)
    endpoint = f"repos/{owner}/{repo}/issues/{number}"
    issue = gh.api(endpoint)
    if issue is None:
        raise PluginError(
            "github::ref::not_found",
            f"GitHub has no pull request or issue {text}, or gh's account cannot see it; check the number "
            f"with `gh issue view {number} --repo {owner}/{repo}`",
            {"ref": text},
        )
    url, title = issue.get("html_url"), issue.get("title")
    if not isinstance(url, str) or not url.startswith("https://"):
        raise gh.failed(endpoint, "answered without an https html_url", f"run `gh api {endpoint}` to see the answer")
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
