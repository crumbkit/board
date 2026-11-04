"""How a plugin runs git: without the user's config, and with a failure it can report."""

import subprocess
from pathlib import Path

from board_plugin.git import Failed, git


def test_a_git_command_that_fails_is_a_failure_with_its_stderr_and_one_that_succeeds_is_its_output(
    tmp_path: Path,
) -> None:
    subprocess.run(["git", "init", "-q", str(tmp_path)], check=True)
    failed = git(str(tmp_path), "rev-parse", "--verify", "--end-of-options", "nothing^{commit}")
    assert isinstance(failed, Failed)
    assert failed.started
    assert failed.stderr
    assert f"`git rev-parse` failed in {tmp_path}" in failed.why
    assert git(str(tmp_path), "rev-parse", "--is-inside-work-tree") == "true\n"


def test_git_that_cannot_start_is_a_failure_that_says_so_and_one_that_starts_runs(tmp_path: Path) -> None:
    failed = git(str(tmp_path / "missing"), "status")
    assert isinstance(failed, Failed)
    assert failed.started
    unstarted = git(str(tmp_path), "status", program=str(tmp_path / "no-git"))
    assert isinstance(unstarted, Failed)
    assert not unstarted.started
    assert "git cannot be run" in unstarted.why
