"""The `start` capability: a change to the caller's repository, its first claim, and the repository's brief."""

import re

from board_plugin.board import Board
from board_plugin.protocol import CallRequest, PluginError
from board_plugin.repos import REPO_TYPE, caller_repo, shown_remote

from board_changes import clone
from board_changes.change import CHANGE_TYPE, PLUGIN, Change, stamp, write
from board_changes.records import claim_record
from board_changes.repo import CODEOWNERS, DOCS, ask_first, files, is_suite, owners, route, suite_in
from board_changes.workflow import choose

# The longest name a title is shortened to.
NAME_MAX = 60


def name_of(title: str) -> str:
    """A change's name from its title: lowercase words joined by hyphens."""
    return re.sub(r"[^a-z0-9]+", "-", title.casefold()).strip("-")[:NAME_MAX].strip("-")


def segments(remote: str) -> list[str]:
    """A stored remote's path segments, such as `["owner", "name"]` for `owner/name`, without a `.git` suffix."""
    return re.split(r"[/:]", remote.rstrip("/").removesuffix(".git"))


def mounted(board: Board, repo: str) -> bool:
    """Whether the repo record at `repo` has a checkout, so its files are in the board."""
    [answer] = board.refs([repo])
    return answer is not None and answer["location"]["source"] != "worktree"


def start(request: CallRequest) -> dict[str, object]:
    """Create the change at `/changes/<name>` for the caller's clone, with a repo record when it has none.

    The repo record, the change and the first claim are written together.
    """
    asked = request.input
    title = str(asked["title"])
    path = f"/changes/{asked.get('name') or name_of(title)}"
    board = Board(request.board.root, PLUGIN)
    if board.refs([path])[0] is not None:
        raise PluginError(
            "changes::change::exists",
            f"a change is already at {path}; give another --name, or resume it with "
            f"`board --personal call changes.next --path {path}`",
            {"path": path},
        )
    worktree = clone.root()
    branch = clone.branch(worktree)
    remote = shown_remote(clone.remote())
    lines: list[dict[str, object]] = []
    repo = caller_repo(board)
    created = repo is None
    if repo is None:
        repo = f"/repos/{asked.get('repo-name') or segments(remote)[-1]}"
        if board.refs([repo])[0] is not None:
            raise PluginError(
                "changes::repo::name_taken",
                f"{repo} already holds another repository, so this clone's remote {remote} cannot be recorded"
                " there; pass --repo-name <name> to record it at /repos/<name>",
                {"repo": repo, "remote": remote},
            )
        lines.append({"path": repo, "expect": "none", "record": {"$type": REPO_TYPE, "remote": remote}})
    parts = segments(remote)
    given = asked.get("workflow")
    owner = parts[-2] if len(parts) > 1 else None
    workflow = choose(board, given if isinstance(given, str) else None, repo.rsplit("/", 1)[-1], owner)
    readable = not created and mounted(board, repo)
    codeowners = files(board, repo, CODEOWNERS) if readable else {}
    rules = owners(next(iter(codeowners.values()), ""))
    handle = asked.get("handle")
    asking = ask_first(rules, handle if isinstance(handle, str) else None)
    docs = files(board, repo, DOCS) if readable else {}
    suite = asked.get("suite") or (suite_in(board, repo, docs) if readable else None)
    if not isinstance(suite, str) or not is_suite(board, suite):
        named = f"{suite} holds no runs/suite/v1" if suite else "no contributing doc names a runs/suite/v1"
        raise PluginError(
            "changes::suite::unknown",
            f"{named}, so the change has no check suite; pass --suite /suites/<name>",
            {"suite": suite},
        )
    change: Change = {
        "$type": CHANGE_TYPE,
        "title": title,
        "repo": repo,
        "branch": branch,
        "workflow": workflow,
        "suite": suite,
        "route": route(rules, asking) if readable else "unread",
    }
    issue = asked.get("issue")
    if isinstance(issue, str):
        change["issue"] = issue
    claim = f"{path}/claims/{stamp()}"
    lines.append({"path": path, "expect": "none", "record": change})
    lines.append({"path": claim, "expect": "none", "record": claim_record(str(asked["session"]))})
    write(board, lines)
    return {
        "path": path,
        "repo": repo,
        "repo_created": created,
        "branch": branch,
        "workflow": workflow,
        "suite": suite,
        "route": change["route"],
        "claim": claim,
        "owners": rules,
        "ask_first": asking,
        "codeowners": next(iter(codeowners), None),
        "docs": list(docs),
    }
