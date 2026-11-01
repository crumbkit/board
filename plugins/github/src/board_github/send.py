"""The actions that send content to GitHub: each collects its parts, has the boundary check them, and sends on a pass.

Every part name is documented in README.md, so a boundary's globs can name them.
"""

from dataclasses import dataclass
from typing import Any, cast

from board_plugin.board import Board
from board_plugin.protocol import CallRequest

from board_github import checkout, crossing, gh
from board_github.objects import ISSUE_TYPE, PLUGIN, PULL_TYPE, write


@dataclass(frozen=True)
class Origin:
    """Where the call sends from and to: the checkout's root, its GitHub repository, and its mount, if any."""

    root: str
    top: str
    owner: str
    repo: str
    mount: str | None

    @property
    def to(self) -> str:
        """The destination the boundary matches, in lower case: `github:<owner>/<repo>`."""
        return f"github:{self.owner}/{self.repo}"

    def check(self, action: str, parts: dict[str, str]) -> dict[str, Any]:
        """The boundary's output for the parts of `github.<action>`, or its refusal passed on."""
        return crossing.check(self.root, f"github.{action}", self.to, self.mount, parts)

    def record(self, kind: str, number: int, url: str, title: str) -> dict[str, Any]:
        """The record of `kind` for number `number` in the checkout's repository."""
        return {"$type": kind, "owner": self.owner, "repo": self.repo, "number": number, "url": url, "title": title}

    def sent(self, passed: dict[str, Any], result: str) -> dict[str, object]:
        """Record what GitHub returned on the crossing, and the output fields every action shares."""
        crossing.record(self.root, passed["digest"], result)
        return {"digest": passed["digest"], "crossing": passed["crossing"], "note": passed["note"], "sent": result}


def origin(request: CallRequest) -> Origin:
    """The checkout Board was started in, its GitHub repository and its mount in the personal board."""
    top = checkout.top()
    owner, repo = checkout.repository(top)
    return Origin(request.board.root, top, owner, repo, checkout.mount(top, request.board.root))


def _pushed(place: Origin, action: str, extra: dict[str, str]) -> tuple[dict[str, Any], str, list[str], str]:
    """Check the commits a push would send with `extra` parts, then push: the check, branch, commits and git's line."""
    branch = checkout.branch(place.top)
    commits = checkout.outgoing(place.top)
    parts = {"push.branch": branch} | checkout.commit_parts(place.top, commits) | extra
    passed = place.check(action, parts)
    return passed, branch, commits, checkout.push(place.top, branch, passed["digest"])


def push(request: CallRequest) -> dict[str, object]:
    """Push the branch checked out, once the boundary passes its name and each commit it would send."""
    place = origin(request)
    passed, branch, commits, line = _pushed(place, "push", {})
    return place.sent(passed, line) | {"branch": branch, "commits": commits}


def pr_create(request: CallRequest) -> dict[str, object]:
    """Push the branch and open a pull request from it, once the boundary passes the commits, title and body."""
    place, given = origin(request), request.input
    title, body = str(given["title"]), str(given["body"])
    passed, branch, commits, _ = _pushed(place, "pr-create", {"pr.title": title, "pr.body": body})
    arguments = ["pr", "create", "--repo", f"{place.owner}/{place.repo}", "--head", branch, "--title", title]
    arguments += ["--body-file", "-"]
    if "base" in given:
        arguments += ["--base", str(given["base"])]
    if given.get("draft") is True:
        arguments.append("--draft")
    # In the checkout, so gh reads no other repository's state.
    printed = gh.send(arguments, body, "gh pr create", place.top).split()
    url = printed[-1] if printed else ""
    number = url.rpartition("/pull/")[2]
    if not url.startswith("https://") or not number.isdigit():
        raise gh.unsent("gh pr create", f"printed no pull request URL after pushing {branch}")
    at = write(Board(place.root, PLUGIN), place.record(PULL_TYPE, int(number), url, title))
    return place.sent(passed, url) | {"branch": branch, "commits": commits, "number": int(number), "path": at}


def pr_edit(request: CallRequest) -> dict[str, object]:
    """Change a pull request's title or body, once the boundary passes the new text."""
    place, given = origin(request), request.input
    fields = {name: str(given[name]) for name in ("title", "body") if name in given}
    passed = place.check("pr-edit", {f"pr.{name}": text for name, text in fields.items()})
    answer = gh.send_json("PATCH", f"repos/{place.owner}/{place.repo}/pulls/{given['number']}", fields)
    return place.sent(passed, answer["html_url"])


def comment(request: CallRequest) -> dict[str, object]:
    """Comment on a pull request or issue, once the boundary passes the comment."""
    place, given = origin(request), request.input
    body = str(given["body"])
    passed = place.check("comment", {"comment.body": body})
    answer = gh.send_json("POST", f"repos/{place.owner}/{place.repo}/issues/{given['number']}/comments", {"body": body})
    return place.sent(passed, answer["html_url"])


def issue_create(request: CallRequest) -> dict[str, object]:
    """Open an issue, once the boundary passes its title and body, and write its record."""
    place, given = origin(request), request.input
    title, body = str(given["title"]), str(given["body"])
    passed = place.check("issue-create", {"issue.title": title, "issue.body": body})
    answer = gh.send_json("POST", f"repos/{place.owner}/{place.repo}/issues", {"title": title, "body": body})
    url, number = cast("str", answer["html_url"]), answer.get("number")
    if not isinstance(number, int):
        raise gh.unsent("gh api --method POST issues", "answered without the issue's number")
    at = write(Board(place.root, PLUGIN), place.record(ISSUE_TYPE, number, url, title))
    return place.sent(passed, url) | {"number": number, "path": at}
