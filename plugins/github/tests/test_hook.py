"""`github.install-hook` and the git pre-push hook it installs, which checks what a raw `git push` sends."""

import json
import subprocess
from pathlib import Path
from typing import Any

from conftest import NAMES, ORIGIN, Desk, git
from test_push import commit_parts, digest, output

HOOK = Path(".git") / "hooks" / "pre-push"


def install(desk: Desk) -> dict[str, Any]:
    """The document `board call github.install-hook` prints in the checkout."""
    return desk.call("install-hook", personal=True)


def push(desk: Desk, *arguments: str, extra: dict[str, str] | None = None) -> subprocess.CompletedProcess[str]:
    """A raw `git push` in the checkout, with the variables a shell would pass it, so its hook finds board."""
    return subprocess.run(
        ["git", "push", "-q", *arguments],
        cwd=desk.checkout,
        env=desk.environment() | (extra or {}),
        capture_output=True,
        text=True,
        check=False,
    )


def head(desk: Desk) -> str:
    """The commit the checkout's branch is at."""
    return git(desk.checkout, "rev-parse", "HEAD")


def test_install_hook_writes_an_executable_pre_push_hook_and_a_second_run_changes_nothing(desk: Desk) -> None:
    first = output(install(desk))
    hook = desk.checkout / HOOK
    assert (first["path"], first["installed"]) == (str(hook), True)
    assert hook.stat().st_mode & 0o777 == 0o755  # noqa: PLR2004 - rwxr-xr-x
    assert "github.pre-push" in hook.read_text()
    again = output(install(desk))
    assert (again["path"], again["installed"]) == (str(hook), False)


def test_a_different_pre_push_hook_is_refused_and_left_as_it_was_and_the_same_hook_is_admitted(desk: Desk) -> None:
    hook = desk.checkout / HOOK
    hook.write_text("#!/bin/sh\nexit 0\n")
    failure = install(desk)
    assert (failure["exit"], failure["code"]) == (1, "github::hook::exists")
    assert str(hook) in failure["message"]
    assert hook.read_text() == "#!/bin/sh\nexit 0\n"
    hook.unlink()
    output(install(desk))
    text = hook.read_text()
    assert output(install(desk))["installed"] is False
    assert hook.read_text() == text


def test_a_hook_path_set_in_git_config_is_where_the_hook_goes(desk: Desk) -> None:
    git(desk.checkout, "config", "core.hooksPath", "githooks")
    assert output(install(desk))["path"] == str(desk.checkout / "githooks" / "pre-push")


def test_a_raw_push_of_a_private_name_is_refused_with_the_next_step_and_the_same_push_without_it_goes(
    desk: Desk,
) -> None:
    output(install(desk))
    desk.refuse(["commit.*.message"])
    before = desk.remote_head("main")
    desk.commit("Fix acme-123")
    refused = push(desk, "origin", "main")
    assert refused.returncode != 0
    assert "github::boundary::refused" in refused.stderr
    assert "Next: change commit." in refused.stderr
    assert desk.remote_head("main") == before
    git(desk.checkout, "commit", "-q", "--amend", "-m", "Fix the gear")
    assert push(desk, "origin", "main").returncode == 0
    assert desk.remote_head("main") == head(desk)


def test_the_hook_checks_the_parts_github_push_would_send_for_each_ref_git_pushes(desk: Desk) -> None:
    output(install(desk))
    git(desk.checkout, "switch", "-q", "-c", "feature")
    sha = desk.commit("Add a lever")
    assert push(desk, "origin", "feature").returncode == 0
    parts = {"push.branch": "feature"} | commit_parts(desk, sha, "Add a lever")
    crossing = desk.record(f"/crossings/{digest('github.push', parts)}")
    assert crossing is not None
    assert (crossing["passed"], crossing["from"]) == (True, "/repos/widgets")


def test_a_covered_push_whose_check_cannot_run_is_refused_and_an_uncovered_one_goes(desk: Desk) -> None:
    output(install(desk))
    checks = [{"control": NAMES, "parts": ["*"]}]
    boundary = {"$type": "boundary/boundary/v1", "to": "github:octo/*", "actions": ["github.*"], "checks": checks}
    desk.write("boundaries/octo.json", json.dumps(boundary))
    desk.commit("Add a gear")
    # The control the boundary names does not exist, so the check cannot run.
    refused = push(desk, "origin", "main")
    assert refused.returncode != 0
    assert "github::boundary::refused" in refused.stderr
    assert desk.remote_head("main") != head(desk)
    desk.write("boundaries/octo.json", json.dumps(boundary | {"to": "github:someone-else/*"}))
    assert push(desk, "origin", "main").returncode == 0
    assert desk.remote_head("main") == head(desk)


def test_a_push_with_board_missing_is_refused_and_with_board_on_path_it_goes(desk: Desk) -> None:
    output(install(desk))
    desk.commit("Add a gear")
    refused = push(desk, "origin", "main", extra={"PATH": "/usr/bin:/bin"})
    assert refused.returncode != 0
    assert "board is not on PATH" in refused.stderr
    assert desk.remote_head("main") != head(desk)
    assert push(desk, "origin", "main").returncode == 0


def test_with_no_personal_board_named_a_push_goes_and_with_a_boundary_refusing_it_is_refused(desk: Desk) -> None:
    output(install(desk))
    desk.refuse(["commit.*.message"])
    desk.commit("Fix acme-123")
    assert push(desk, "origin", "main").returncode != 0
    config = desk.home / "config" / "board" / "config.json"
    config.write_text(json.dumps({"boards": [str(desk.personal)]}))
    assert push(desk, "origin", "main").returncode == 0
    assert desk.remote_head("main") == head(desk)


def test_a_push_to_a_remote_that_is_not_github_is_not_checked_and_one_to_github_is(desk: Desk) -> None:
    output(install(desk))
    desk.refuse(["commit.*.message"])
    desk.commit("Fix acme-123")
    git(desk.checkout, "remote", "add", "elsewhere", str(desk.remote))
    assert push(desk, "elsewhere", "main").returncode == 0
    git(desk.checkout, "reset", "-q", "--hard", "HEAD~1")
    desk.commit("Fix acme-456")
    assert push(desk, "-f", "origin", "main").returncode != 0


def test_a_deleted_branch_sends_nothing_to_check(desk: Desk) -> None:
    git(desk.checkout, "switch", "-q", "-c", "gone")
    desk.commit("Fix acme-123")
    assert push(desk, "origin", "gone").returncode == 0
    output(install(desk))
    desk.refuse(["commit.*.message"])
    assert push(desk, "origin", "--delete", "gone").returncode == 0
    assert desk.remote_head("gone") is None


def test_a_push_through_github_push_judges_each_part_once_with_the_hook_installed(desk: Desk) -> None:
    output(install(desk))
    desk.write("rubrics/plain.md", "Write plainly.\n")
    control = {"$type": "controls/control/v1", "requirement": "A test.", "purpose": "A test."}
    desk.write("controls/plain.json", json.dumps(control | {"rule": {"judged": "/rubrics/plain.md"}}))
    checks = [{"control": "/controls/plain", "parts": ["push.branch"]}]
    boundary = {"$type": "boundary/boundary/v1", "to": "github:*", "actions": ["github.*"], "checks": checks}
    desk.write("boundaries/octo.json", json.dumps(boundary))
    agent = desk.home / "agent"
    agent.write_text(
        f"#!/bin/sh\ncat > /dev/null\necho run >> '{desk.home / 'judged.txt'}'\necho '{{\"findings\": []}}'\n"
    )
    agent.chmod(0o755)
    desk.commit("Add a gear")
    result = output(desk.call("push", extra={"BOARD_CONTROLS_AGENT": json.dumps([str(agent)])}))
    assert desk.remote_head("main") == result["commits"][-1]
    assert (desk.home / "judged.txt").read_text() == "run\n"


def test_an_annotated_tag_is_refused_for_its_message_and_the_same_tag_without_it_goes(desk: Desk) -> None:
    output(install(desk))
    desk.refuse(["tag.*.message", "tag.*.tagger_*"])
    git(desk.checkout, "tag", "-a", "v1", "-m", "Release acme-123")
    refused = push(desk, "origin", "v1")
    assert refused.returncode != 0
    assert "tag." in refused.stderr
    assert git(desk.remote, "tag") == ""
    git(desk.checkout, "tag", "-d", "v1")
    git(desk.checkout, "tag", "-a", "v1", "-m", "Release one")
    assert push(desk, "origin", "v1").returncode == 0
    assert git(desk.remote, "tag") == "v1"


def test_a_tag_of_a_tag_is_checked_at_each_level_and_its_tagger_is_a_part(desk: Desk) -> None:
    output(install(desk))
    desk.refuse(["tag.*.tagger_name"])
    git(desk.checkout, "-c", "user.name=Pat acme-123", "tag", "-a", "inner", "-m", "Inner")
    git(desk.checkout, "tag", "-a", "outer", "-m", "Outer", "inner")
    git(desk.checkout, "tag", "-d", "inner")
    refused = push(desk, "origin", "outer")
    assert refused.returncode != 0
    assert "tagger_name" in refused.stderr
    git(desk.checkout, "tag", "-d", "outer")
    git(desk.checkout, "tag", "-a", "outer", "-m", "Outer")
    assert push(desk, "origin", "outer").returncode == 0


def test_the_name_inside_a_nested_tag_is_checked_and_the_same_tags_named_plainly_go(desk: Desk) -> None:
    output(install(desk))
    desk.refuse(["tag.*.headers"])
    git(desk.checkout, "tag", "-a", "acme-123", "-m", "Inner")
    git(desk.checkout, "tag", "-a", "outer", "-m", "Outer", "acme-123")
    git(desk.checkout, "tag", "-d", "acme-123")
    refused = push(desk, "origin", "outer")
    assert refused.returncode != 0
    assert "headers" in refused.stderr
    git(desk.checkout, "tag", "-d", "outer")
    git(desk.checkout, "tag", "-a", "inner", "-m", "Inner")
    git(desk.checkout, "tag", "-a", "outer", "-m", "Outer", "inner")
    assert push(desk, "origin", "outer").returncode == 0


def test_a_push_url_unlike_the_fetch_url_checks_the_commits_the_push_url_lacks(desk: Desk) -> None:
    fork = desk.home / "fork.git"
    subprocess.run(["git", "init", "-q", "--bare", "-b", "main", str(fork)], check=True)
    git(desk.checkout, "config", f"url.{fork}.insteadOf", "https://github.com/octo/fork.git")
    desk.commit("Fix acme-123")
    git(desk.checkout, "push", "-q", "https://github.com/octo/fork.git", "main")
    git(desk.checkout, "remote", "set-url", "origin", "https://github.com/octo/fork.git")
    git(desk.checkout, "remote", "set-url", "--push", "origin", ORIGIN)
    output(install(desk))
    desk.refuse(["commit.*.message"])
    refused = push(desk, "origin", "main")
    assert refused.returncode != 0
    assert "github::boundary::refused" in refused.stderr
    git(desk.checkout, "commit", "-q", "--amend", "-m", "Fix the gear")
    assert push(desk, "origin", "main").returncode == 0


def test_refused_content_that_quotes_the_no_board_code_is_still_refused(desk: Desk) -> None:
    output(install(desk))
    desk.refuse(["commit.*.message"])
    desk.commit("Fix acme-123 code:  board::config::not_found")
    assert push(desk, "origin", "main").returncode != 0
    assert desk.remote_head("main") != head(desk)
    git(desk.checkout, "commit", "-q", "--amend", "-m", "Fix code:  board::config::not_found")
    assert push(desk, "origin", "main").returncode == 0


def test_a_global_hook_path_is_refused_and_a_repository_one_is_admitted(desk: Desk) -> None:
    (desk.home / ".gitconfig").write_text(f"[core]\n\thooksPath = {desk.home / 'githooks'}\n")
    failure = install(desk)
    assert (failure["exit"], failure["code"]) == (1, "github::hook::shared")
    assert not (desk.home / "githooks").exists()
    git(desk.checkout, "config", "core.hooksPath", "githooks")
    assert output(install(desk))["path"] == str(desk.checkout / "githooks" / "pre-push")


def test_an_older_copy_of_this_hook_is_replaced_and_a_foreign_hook_is_not(desk: Desk) -> None:
    output(install(desk))
    hook = desk.checkout / HOOK
    current = hook.read_text()
    hook.write_text(current.replace("exit 1\n", "exit 2\n"))
    assert output(install(desk))["installed"] is True
    assert hook.read_text() == current
    hook.write_text("#!/bin/sh\n# my hook\nexit 0\n")
    assert install(desk)["code"] == "github::hook::exists"


def test_a_push_that_deletes_one_branch_and_updates_another_checks_the_update(desk: Desk) -> None:
    git(desk.checkout, "switch", "-q", "-c", "gone")
    assert push(desk, "origin", "gone").returncode == 0
    git(desk.checkout, "switch", "-q", "main")
    output(install(desk))
    desk.refuse(["commit.*.message"])
    desk.commit("Fix acme-123")
    assert push(desk, "origin", ":gone", "main").returncode != 0
    assert desk.remote_head("gone") is not None
    git(desk.checkout, "commit", "-q", "--amend", "-m", "Fix the gear")
    assert push(desk, "origin", ":gone", "main").returncode == 0
    assert desk.remote_head("gone") is None


def test_a_push_from_a_linked_worktree_runs_the_hook(desk: Desk) -> None:
    output(install(desk))
    desk.refuse(["commit.*.message"])
    linked = desk.home / "linked"
    git(desk.checkout, "worktree", "add", "-q", "-b", "side", str(linked))
    (linked / "side.txt").write_text("x\n")
    git(linked, "add", "side.txt")
    git(linked, "commit", "-q", "-m", "Fix acme-123")
    ran = subprocess.run(
        ["git", "push", "-q", "origin", "side"],
        cwd=linked,
        env=desk.environment(),
        capture_output=True,
        text=True,
        check=False,
    )
    assert ran.returncode != 0
    git(linked, "commit", "-q", "--amend", "-m", "Fix the gear")
    ran = subprocess.run(
        ["git", "push", "-q", "origin", "side"],
        cwd=linked,
        env=desk.environment(),
        capture_output=True,
        text=True,
        check=False,
    )
    assert ran.returncode == 0, ran.stderr


def covered(desk: Desk, *flags: str) -> list[dict[str, Any]]:
    """What `board --personal call github.covers` names in the checkout."""
    result: list[dict[str, Any]] = output(desk.call("covers", *flags, personal=True))["covered"]
    return result


def test_covers_names_the_boundary_on_the_checkouts_repository_and_none_on_another(desk: Desk) -> None:
    desk.refuse(["*"], actions="github.pr-*")
    assert covered(desk, "--action", "pr-create") == [{"to": "github:octo/widgets", "boundaries": ["/boundaries/octo"]}]
    assert covered(desk, "--action", "push") == []
    assert covered(desk, "--action", "pr-create", "--repo", "someone/else") == []
    git(desk.checkout, "remote", "add", "elsewhere", str(desk.remote))
    assert covered(desk, "--action", "pr-create", "--remote", "elsewhere") == []


def test_covers_applies_from_by_the_checkouts_mount(desk: Desk) -> None:
    checks = [{"control": NAMES, "parts": ["*"]}]
    boundary = {"$type": "boundary/boundary/v1", "to": "github:*", "actions": ["github.*"], "checks": checks}
    desk.refuse(["*"])
    desk.write("boundaries/octo.json", json.dumps(boundary | {"from": ["/repos/widgets"]}))
    assert len(covered(desk, "--action", "push")) == 1
    desk.write("boundaries/octo.json", json.dumps(boundary | {"from": ["/repos/gears"]}))
    assert covered(desk, "--action", "push") == []
