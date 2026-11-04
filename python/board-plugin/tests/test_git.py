"""How a plugin runs git: without the user's config, and with a failure it can report."""

import subprocess
from pathlib import Path

import pytest

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


def test_git_that_cannot_start_is_a_failure_that_says_so_and_one_that_starts_runs(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    subprocess.run(["git", "init", "-q", str(tmp_path)], check=True)
    failed = git(str(tmp_path / "missing"), "status")
    assert isinstance(failed, Failed)
    assert failed.started
    with monkeypatch.context() as patched:
        patched.setenv("PATH", str(tmp_path / "no-git"))
        unstarted = git(str(tmp_path), "status")
    assert isinstance(unstarted, Failed)
    assert not unstarted.started
    assert "git cannot be run" in unstarted.why
    assert isinstance(git(str(tmp_path), "status"), str)


def test_a_git_command_that_runs_past_its_timeout_says_it_timed_out_and_after_how_long(tmp_path: Path) -> None:
    subprocess.run(["git", "init", "-q", str(tmp_path)], check=True)
    late = git(str(tmp_path), "-c", "core.pager=cat", "status", timeout=0.000001)
    assert isinstance(late, Failed)
    assert (late.started, late.timed_out) == (True, True)
    assert late.why == f"`git status` timed out after 1e-06 s in {tmp_path}"
    on_time = git(str(tmp_path), "-c", "core.pager=cat", "status")
    assert isinstance(on_time, str)


def test_a_failure_names_the_git_command_after_any_config_options(tmp_path: Path) -> None:
    subprocess.run(["git", "init", "-q", str(tmp_path)], check=True)
    failed = git(str(tmp_path), "-c", "diff.noprefix=false", "rev-parse", "--verify", "nothing")
    assert isinstance(failed, Failed)
    assert failed.why.startswith(f"`git rev-parse` failed in {tmp_path}: ")
