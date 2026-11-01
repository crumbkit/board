"""The URL rule the sending actions and the pre-push hook share: which URLs reach GitHub, and which cannot be read."""

import subprocess
from pathlib import Path

import pytest
from board_plugin.protocol import PluginError
from conftest import git

from board_github import checkout


def aliases(host: str) -> str | None:
    """A stand-in for `ssh -G`: `work-gh` is an alias of github.com, `elsewhere` of another host."""
    return {"work-gh": "github.com", "elsewhere": "git.example.com"}.get(host)


@pytest.mark.parametrize(
    "url",
    [
        "https://github.com/Octo/Widgets.git",
        "https://GitHub.com/octo/widgets",
        "https://github.com:443/octo/widgets.git",
        "https://user:secret@www.github.com/octo/widgets/",
        "git@github.com:octo/widgets.git",
        "ssh://git@ssh.github.com:443/octo/widgets.git",
        "ssh://git@github.com:22/octo/widgets",
        "git@github.com-work:octo/widgets.git",
        "git@work-gh:octo/widgets.git",
        "git://github.com/octo/widgets.git",
    ],
)
def test_every_form_of_a_github_url_names_its_repository(url: str) -> None:
    assert checkout.github_repository(url, aliases) == ("octo", "widgets")


@pytest.mark.parametrize(
    "url", ["/srv/git/widgets.git", "../widgets.git", "file:///srv/widgets.git", "https://gitlab.example/o/w.git"]
)
def test_a_local_path_or_another_host_is_not_github(url: str) -> None:
    assert checkout.github_repository(url, aliases) is None
    assert checkout.github_repository("git@elsewhere:octo/widgets.git", aliases) is None


@pytest.mark.parametrize(
    "url", ["git@unknown-alias:octo/widgets.git", "https://github.com/octo", "ext::ssh github.com", "git@-oX:a/b"]
)
def test_a_url_that_could_reach_github_but_cannot_be_read_is_refused(url: str) -> None:
    with pytest.raises(PluginError) as raised:
        checkout.github_repository(url, aliases)
    assert raised.value.code == "github::remote::unknown"
    assert checkout.github_repository("git@work-gh:octo/widgets.git", aliases) == ("octo", "widgets")


def repository(tmp_path: Path) -> str:
    """An empty git repository with an origin on GitHub."""
    subprocess.run(["git", "init", "-q", str(tmp_path)], check=True)
    git(tmp_path, "remote", "add", "origin", "https://github.com/Octo/Widgets.git")
    return str(tmp_path)


def test_a_push_to_a_github_url_goes_to_that_repository(tmp_path: Path) -> None:
    at = repository(tmp_path)
    assert checkout.destinations(at, "origin", "https://github.com/octo/other.git") == [
        "github:octo/other",
        "github:octo/widgets",
    ]


def test_a_push_rewritten_to_a_local_path_goes_to_the_remote_as_configured(tmp_path: Path) -> None:
    at = repository(tmp_path)
    assert checkout.destinations(at, "origin", "/srv/widgets.git") == ["github:octo/widgets"]


def test_a_push_url_unlike_the_fetch_url_names_the_push_url(tmp_path: Path) -> None:
    at = repository(tmp_path)
    git(tmp_path, "remote", "set-url", "--push", "origin", "git@github.com:octo/covered.git")
    assert checkout.destinations(at, "origin", "/srv/covered.git") == ["github:octo/covered"]


def test_a_push_to_another_host_goes_to_no_github_repository(tmp_path: Path) -> None:
    at = repository(tmp_path)
    git(tmp_path, "remote", "add", "elsewhere", "https://gitlab.example/o/w.git")
    assert checkout.destinations(at, "elsewhere", "https://gitlab.example/o/w.git") == []
