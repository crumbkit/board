"""`github.push`: the commits a push would send are checked at the boundary, and pushed only on a pass."""

import hashlib
import json
from typing import Any

from conftest import NAMES, Desk, git

TO = "github:octo/widgets"


def digest(action: str, parts: dict[str, str]) -> str:
    """The boundary's digest of content, as its README defines it."""
    content = {"action": action, "parts": parts, "to": TO}
    canonical = json.dumps(content, sort_keys=True, separators=(",", ":"), ensure_ascii=False)
    return hashlib.sha256(canonical.encode()).hexdigest()


def commit_parts(desk: Desk, sha: str, message: str) -> dict[str, str]:
    """The parts of one commit, named by its short SHA."""
    diff = git(desk.checkout, "show", "--format=", "--diff-merges=first-parent", sha)
    short = sha[:12]
    return {
        f"commit.{short}.message": message + "\n",
        f"commit.{short}.author_email": "t@t",
        f"commit.{short}.committer_email": "t@t",
        f"commit.{short}.diff": diff + "\n",
    }


def output(document: dict[str, Any]) -> dict[str, Any]:
    """The capability's output in a `--json` report."""
    assert document["exit"] == 0, document
    result: dict[str, Any] = document["data"]["output"]
    return result


def test_a_push_checks_each_new_commit_by_name_then_pushes_and_records_the_crossing(desk: Desk) -> None:
    first = desk.commit("Add a gear")
    second = desk.commit("Add a spring", file="spring.txt")
    result = output(desk.call("push"))
    parts = (
        {"push.branch": "main"} | commit_parts(desk, first, "Add a gear") | commit_parts(desk, second, "Add a spring")
    )
    assert result["digest"] == digest("github.push", parts)
    assert (result["branch"], result["commits"]) == ("main", [first, second])
    assert desk.remote_head("main") == second
    crossing = desk.record(result["crossing"])
    assert crossing is not None
    assert (crossing["passed"], crossing["to"], crossing["from"]) == (True, TO, "/repos/widgets")
    assert crossing["result"] == result["sent"]
    assert result["sent"].startswith("refs/heads/main:refs/heads/main")


def test_a_new_branch_sends_only_the_commits_its_remote_lacks(desk: Desk) -> None:
    git(desk.checkout, "switch", "-q", "-c", "feature")
    only = desk.commit("Add a lever")
    result = output(desk.call("push"))
    assert (result["branch"], result["commits"]) == ("feature", [only])
    assert desk.remote_head("feature") == only
    assert git(desk.checkout, "rev-parse", "--abbrev-ref", "feature@{upstream}") == "origin/feature"


def test_a_private_name_in_a_commit_message_is_refused_with_the_boundarys_next_step_and_nothing_is_pushed(
    desk: Desk,
) -> None:
    desk.refuse(["commit.*.message"])
    before = desk.remote_head("main")
    sha = desk.commit("Fix acme-123")
    failure = desk.call("push")
    assert (failure["exit"], failure["code"]) == (1, "github::boundary::refused")
    fields = failure["fields"]
    assert fields["code"] == "boundary::check::failed"
    assert fields["recovery"]["kind"] == "hint"
    assert fields["recovery"]["text"].startswith("Next: change commit.")
    assert fields["recovery"]["text"] in failure["message"]
    assert fields["failures"] == [
        {"control": NAMES, "part": f"commit.{sha[:12]}.message", "text": "acme-123", "line": 1}
    ]
    assert desk.remote_head("main") == before
    git(desk.checkout, "commit", "-q", "--amend", "-m", "Fix the gear")
    assert output(desk.call("push"))["commits"] == [git(desk.checkout, "rev-parse", "HEAD")]
    assert desk.remote_head("main") != before


def test_a_private_name_in_a_diff_or_an_author_email_is_refused_and_the_same_commit_without_it_is_pushed(
    desk: Desk,
) -> None:
    desk.refuse(["commit.*.diff", "commit.*.author_email"])
    desk.commit("Add a gear", text="see acme-123\n")
    assert desk.call("push")["fields"]["failures"][0]["part"].endswith(".diff")
    git(desk.checkout, "reset", "-q", "--hard", "HEAD~1")
    git(desk.checkout, "-c", "user.email=acme-456@example.com", "commit", "-q", "--allow-empty", "-m", "Empty")
    assert desk.call("push")["fields"]["failures"][0]["part"].endswith(".author_email")
    git(desk.checkout, "reset", "-q", "--hard", "HEAD~1")
    desk.commit("Add a gear", text="fine\n")
    assert len(output(desk.call("push"))["commits"]) == 1


def test_a_checkout_no_board_mounts_is_checked_without_from_and_a_mounted_one_names_its_mount(desk: Desk) -> None:
    desk.commit("Add a gear")
    git(desk.checkout, "switch", "-q", "-c", "mounted")
    mounted = desk.record(output(desk.call("push", personal=True))["crossing"])
    assert mounted is not None
    assert mounted["from"] == "/repos/widgets"
    (desk.personal / "repos" / "widgets.json").unlink()
    git(desk.checkout, "switch", "-q", "-c", "unmounted")
    unmounted = desk.record(output(desk.call("push", personal=True))["crossing"])
    assert unmounted is not None
    assert "from" not in unmounted


def test_a_detached_head_is_refused_and_the_same_commit_on_a_branch_is_pushed(desk: Desk) -> None:
    sha = desk.commit("Add a gear")
    git(desk.checkout, "switch", "-q", "--detach", sha)
    failure = desk.call("push")
    assert (failure["exit"], failure["code"]) == (1, "github::checkout::detached")
    git(desk.checkout, "switch", "-q", "main")
    assert output(desk.call("push"))["commits"] == [sha]


def test_a_remote_that_is_not_github_is_refused_and_a_github_remote_is_checked(desk: Desk) -> None:
    desk.commit("Add a gear")
    git(desk.checkout, "remote", "set-url", "origin", str(desk.remote))
    failure = desk.call("push")
    assert (failure["exit"], failure["code"]) == (1, "github::remote::not_github")
    assert str(desk.remote) in failure["message"]
    git(desk.checkout, "remote", "set-url", "origin", "git@github.com:Octo/Widgets.git")
    git(desk.checkout, "config", f"url.{desk.remote}.insteadOf", "git@github.com:Octo/Widgets.git")
    assert output(desk.call("push"))["branch"] == "main"


def test_a_push_git_refuses_is_reported_and_the_crossing_keeps_no_result(desk: Desk) -> None:
    desk.commit("Add a gear")
    git(desk.checkout, "push", "-q", "origin", "main")
    git(desk.checkout, "reset", "-q", "--hard", "HEAD~1")
    desk.commit("Add a spring", file="spring.txt")
    failure = desk.call("push")
    assert (failure["exit"], failure["code"]) == (1, "github::push::failed")
    crossing = desk.record(f"/crossings/{failure['fields']['digest']}")
    assert crossing is not None
    assert "result" not in crossing
    git(desk.checkout, "pull", "-q", "--rebase", "origin", "main")
    assert len(output(desk.call("push"))["commits"]) == 1


def test_a_call_outside_a_git_repository_is_refused_and_one_inside_is_admitted(desk: Desk) -> None:
    failure = desk.call("push", cwd=desk.home, personal=True)
    assert (failure["exit"], failure["code"]) == (1, "github::checkout::not_found")
    assert output(desk.call("push", personal=True))["branch"] == "main"


def test_a_mount_in_a_board_other_than_the_personal_one_gives_no_from(desk: Desk) -> None:
    work = desk.home / "work"
    work.mkdir()
    git(work, "init", "-q")
    (desk.personal / "repos" / "widgets.json").rename(work / "widgets.json")
    (work / ".board").mkdir()
    (work / ".board" / "board.json").write_text(json.dumps({"$type": "board/config/v1", "commit_on_put": False}))
    known = {"boards": [str(work)], "personal": str(desk.personal)}
    (desk.home / "config" / "board" / "config.json").write_text(json.dumps(known))
    enabled = desk.run("plugin", "enable", "github", cwd=work)
    assert enabled.returncode == 0, enabled.stderr
    desk.commit("Add a gear")
    crossing = desk.record(output(desk.call("push"))["crossing"])
    assert crossing is not None
    assert "from" not in crossing
