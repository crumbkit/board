"""How `edited` reports a git that cannot give the change's diff."""

from typing import TYPE_CHECKING, Any, cast

import pytest
from board_plugin.git import Failed

if TYPE_CHECKING:
    from board_plugin.board import Board

from board_changes import diff


class Refs:
    """A Board whose `refs` answers one mount at /repos/x that follows main."""

    def refs(self, _paths: list[str]) -> list[dict[str, Any]]:
        """The location of the mount."""
        return [{"location": {"repository": "/r", "path": "", "pointers": [{"follow": "main"}]}}]


@pytest.mark.parametrize(
    ("failed", "hint"),
    [
        (Failed("git cannot be run: gone; put git on PATH", "gone", started=False), False),
        (Failed("`git diff` timed out after 60 s in /r", "", started=True, timed_out=True), False),
        (Failed("`git diff` failed in /r: bad object", "bad object", started=True), True),
    ],
)
def test_only_a_git_that_ran_and_failed_is_told_to_fetch_the_commits(
    monkeypatch: pytest.MonkeyPatch, failed: Failed, *, hint: bool
) -> None:
    def fails(_repository: str, *_arguments: str) -> Failed:
        return failed

    monkeypatch.setattr(diff, "git", fails)
    files, why = diff.edited(cast("Board", Refs()), "/repos/x", "abc")
    assert files == []
    assert why is not None
    assert why.startswith(failed.why)
    assert ("fetch abc and main into the mounted repository" in why) is hint
