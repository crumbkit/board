"""How a remote is shown, which GitHub repositories a `notes/repo/v1` record covers, and the repo listing."""

import subprocess
from pathlib import Path

import pytest

from board_plugin.protocol import PluginError
from board_plugin.repos import caller_origin, covers, parsed, record, shown_remote


@pytest.mark.parametrize(
    ("remote", "shown"),
    [
        ("https://x-access-token:SECRET@github.com/crumbkit/viitta.git", "crumbkit/viitta"),
        ("https://user:SECRET@gitlab.com/example-org/home.git", "https://gitlab.com/example-org/home.git"),
        ("https://user:p@ss@gitlab.com/example-org/home.git", "https://gitlab.com/example-org/home.git"),
        ("ssh://git@gitlab.com/example-org/home.git", "ssh://gitlab.com/example-org/home.git"),
        ("git@gitlab.com:example-org/home.git", "gitlab.com:example-org/home.git"),
        ("/Users/me/Code/home", "/Users/me/Code/home"),
    ],
)
def test_a_remote_is_shown_as_owner_and_name_on_github_and_elsewhere_without_its_user(remote: str, shown: str) -> None:
    assert shown_remote(remote) == shown


@pytest.mark.parametrize(
    ("remote", "covered"),
    [
        ("https://github.com/example-org/home.git", True),
        ("https://github.com/example-org/Home", True),
        ("git@github.com:example-org/home.git", True),
        ("ssh://git@github.com/example-org/home.git", True),
        ("git@github.com-work:example-org/home.git", True),
        ("example-org/home", True),
        ("crumbkit/viitta", True),
        ("crumbkit/board", False),
        ("https://gitlab.com/example-org/home.git", False),
        ("git@gitlab.com:example-org/home.git", False),
        ("git@github.com.evil.io:example-org/home.git", False),
        ("ssh://git@github.com.evil.io/example-org/home.git", False),
        ("ssh://git@gitlab.com/example-org/home.git", False),
        ("/Users/me/Code/home", False),
        (None, False),
    ],
)
def test_a_record_covers_a_github_remote_by_owner_or_by_repository(remote: str | None, *, covered: bool) -> None:
    assert covers({"covers": ["example-org/*", "crumbkit/viitta"]}, remote) is covered


def test_a_listed_mount_carries_its_pin_and_clone_and_the_boards_own_tree_neither() -> None:
    rows = [
        ["/repos/work/", "c0ffee", "/clones/work", None, None, None, None],
        ["/", "worktree", "/board", None, None, None, None],
    ]
    found = parsed("tasks", rows)
    assert [each[:3] for each in found] == [("/", None, None), ("/repos/work", "c0ffee", "/clones/work")]
    assert record("tasks", "/repos/work", found[1][3]) is None
    with pytest.raises(PluginError) as refused:
        parsed("tasks", [["/repos/lost/", "c0ffee", None, None, None, None, None]])
    assert (refused.value.code, refused.value.fields) == ("tasks::board::refused", {"path": "/repos/lost"})


def test_outside_a_git_repository_there_is_no_origin_and_no_reason_and_inside_one_without_origin_a_reason(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    outside, inside = tmp_path / "outside", tmp_path / "inside"
    outside.mkdir()
    subprocess.run(["git", "init", "-q", str(inside)], check=True)
    monkeypatch.setenv("GIT_CEILING_DIRECTORIES", str(tmp_path))
    monkeypatch.setenv("BOARD_CALLER_DIR", str(outside))
    assert caller_origin() == (None, None)
    monkeypatch.setenv("BOARD_CALLER_DIR", str(inside))
    remote, why = caller_origin()
    assert remote is None
    assert why is not None
    assert why.startswith(f"git gave no origin for {inside}: ")
    subprocess.run(["git", "-C", str(inside), "remote", "add", "origin", "git@github.com:acme/x.git"], check=True)
    assert caller_origin() == ("git@github.com:acme/x.git", None)
