"""Remote matching: how a remote is shown, and which GitHub repositories a `notes/repo/v1` record covers."""

import pytest

from board_plugin.repos import covers, shown_remote


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
