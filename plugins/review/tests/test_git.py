"""How the review plugin reports a git command that fails."""

import functools
import subprocess
from pathlib import Path

import pytest
from board_plugin import git as shared
from board_plugin.protocol import PluginError

from board_review import git as review_git
from board_review.git import Repository


def test_a_git_command_that_times_out_is_refused_saying_it_timed_out_and_one_on_time_runs(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    subprocess.run(["git", "init", "-q", str(tmp_path)], check=True)
    repository = Repository(str(tmp_path))
    with monkeypatch.context() as patched:
        patched.setattr(review_git, "git", functools.partial(shared.git, timeout=0.000001))
        with pytest.raises(PluginError) as refused:
            repository.git("status")
    assert refused.value.code == "review::git::failed"
    assert refused.value.message.startswith(f"`git status` timed out after 1e-06 s in {tmp_path}")
    assert isinstance(repository.git("status"), str)
