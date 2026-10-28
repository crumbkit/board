"""What a mounted repository says about contributing: its code owners, its contributing docs and its check suite."""

import re
from typing import TypedDict

from board_plugin.board import Board, literal
from board_plugin.protocol import PluginError

SUITE_TYPE = "runs/suite/v1"
# Where GitHub looks for CODEOWNERS, in its order; the first found is used.
CODEOWNERS = (".github/CODEOWNERS", "CODEOWNERS", "docs/CODEOWNERS")
# The contributing docs, in the order a check suite is looked for in them.
DOCS = ("CONTRIBUTING.md", ".github/CONTRIBUTING.md", "docs/CONTRIBUTING.md", "AGENTS.md")
# A logical path written as inline code, such as `/suites/board-tests`.
CODE_PATH = re.compile(r"`(/[A-Za-z0-9._/-]+)`")


class Rule(TypedDict):
    """One CODEOWNERS line: a pattern and the owners of the files it matches."""

    pattern: str
    owners: list[str]


def files(board: Board, repo: str, names: tuple[str, ...]) -> dict[str, str]:
    """The text of each of `names` in `repo` that exists, by its logical path, in the order of `names`."""
    paths = [f"{repo}/{name}" for name in names]
    found = board.get(paths)
    return {
        path: text.decode(errors="replace") for path, text in zip(paths, found, strict=True) if isinstance(text, bytes)
    }


def owners(text: str) -> list[Rule]:
    """The rules of a CODEOWNERS file, in order, without comments."""
    rules: list[Rule] = []
    for line in text.splitlines():
        words = line.split("#", 1)[0].split()
        if words:
            rules.append(Rule(pattern=words[0], owners=words[1:]))
    return rules


def ask_first(rules: list[Rule], handle: str | None) -> list[str]:
    """The patterns someone other than `handle` owns, each once, in order."""
    mine = handle.casefold() if handle else ""
    owned = [rule["pattern"] for rule in rules if rule["owners"] and mine not in [o.casefold() for o in rule["owners"]]]
    return list(dict.fromkeys(owned))


def route(rules: list[Rule], asking: list[str]) -> str:
    """`unowned` when nothing names owners, `owner` when nobody else owns a pattern, else `ask-first`."""
    if not any(rule["owners"] for rule in rules):
        return "unowned"
    return "ask-first" if asking else "owner"


def suite_in(board: Board, repo: str, docs: dict[str, str]) -> str | None:
    """The first `runs/suite/v1` the docs name as inline code: in the repository's own board, or else in this one."""
    named = [path for text in docs.values() for path in CODE_PATH.findall(text)]
    candidates = [candidate for path in named for candidate in (f"{repo}{path}", path)]
    if not candidates:
        return None
    listed = ", ".join(literal(candidate) for candidate in candidates)
    rows = board.rows(
        f"SELECT logical FROM records WHERE type = {literal(SUITE_TYPE)} AND logical IN ({listed})"  # noqa: S608 - values are quoted by literal()
    )
    suites = {row[0] for row in rows}
    return next((candidate for candidate in candidates if candidate in suites), None)


def check_suite(board: Board, repo: str) -> str | None:
    """The check suite the repository's contributing docs name, or `None`."""
    return suite_in(board, repo, files(board, repo, DOCS))


def exists(board: Board, repo: str) -> None:
    """Refuse a repository path that holds nothing with `changes::repo::not_found`."""
    [answer] = board.refs([repo])
    if answer is None:
        raise PluginError(
            "changes::repo::not_found",
            f"nothing is at {repo}; name the repository by the logical path of its mount, such as /repos/<name>, "
            "and see what is mounted with `board ls /repos`",
            {"repo": repo},
        )
