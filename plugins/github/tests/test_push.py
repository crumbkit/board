"""`github.push`: the commits a push would send are checked at the boundary, and pushed only on a pass."""

import hashlib
import json
import os
import subprocess
from typing import Any

from conftest import NAMES, ORIGIN, Desk, board_binary, git

TO = "github:octo/widgets"
PULL_NUMBER = 12


def digest(action: str, parts: dict[str, str]) -> str:
    """The boundary's digest of content, as its README defines it."""
    content = {"action": action, "parts": parts, "to": TO}
    canonical = json.dumps(content, sort_keys=True, separators=(",", ":"), ensure_ascii=False)
    return hashlib.sha256(canonical.encode()).hexdigest()


def commit_parts(desk: Desk, sha: str, message: str, binary: str = "") -> dict[str, str]:
    """The parts of one commit, named by its short SHA."""
    flags = ("--format=", "--diff-merges=first-parent", "--no-color", "--no-ext-diff", "--no-textconv", "--text")
    diff = git(desk.checkout, "-c", "core.quotePath=false", "show", *flags, sha)
    short = sha[:12]
    return {
        f"commit.{short}.message": message + "\n",
        f"commit.{short}.author_name": "t",
        f"commit.{short}.author_email": "t@t",
        f"commit.{short}.committer_name": "t",
        f"commit.{short}.committer_email": "t@t",
        f"commit.{short}.diff": diff + "\n",
        f"commit.{short}.headers": "",
        f"commit.{short}.binary": binary,
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
    assert result["sent"].startswith(f"{second}:refs/heads/main")
    assert git(desk.checkout, "rev-parse", "--abbrev-ref", "main@{upstream}") == "origin/main"


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


def test_a_private_name_in_a_diff_an_author_email_or_a_name_is_refused_and_the_same_commit_without_it_is_pushed(
    desk: Desk,
) -> None:
    desk.refuse(["commit.*.diff", "commit.*.author_*", "commit.*.committer_name"])
    desk.commit("Add a gear", text="see acme-123\n")
    assert desk.call("push")["fields"]["failures"][0]["part"].endswith(".diff")
    for setting, part in [
        ("user.email=acme-456@example.com", ".author_email"),
        ("user.name=Pat (acme-789)", ".author_name"),
    ]:
        git(desk.checkout, "reset", "-q", "--hard", "HEAD~1")
        git(desk.checkout, "-c", setting, "commit", "-q", "--allow-empty", "-m", "Empty")
        parts = {failure["part"][-len(part) :] for failure in desk.call("push")["fields"]["failures"]}
        assert part in parts, setting
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


def test_text_hidden_from_diffs_by_an_attribute_is_checked_and_binary_paths_are_named(desk: Desk) -> None:
    desk.refuse(["commit.*.diff"])
    (desk.checkout / ".gitattributes").write_text("*.txt -diff\n")
    git(desk.checkout, "add", ".gitattributes")
    sha = desk.commit("Add notes", file="notes.txt", text="token acme-123\n")
    failure = desk.call("push")
    assert failure["fields"]["failures"][0]["part"] == f"commit.{sha[:12]}.diff"
    git(desk.checkout, "reset", "-q", "--hard", "HEAD~1")
    (desk.checkout / ".gitattributes").write_text("*.txt -diff\n")
    git(desk.checkout, "add", ".gitattributes")
    sha = desk.commit("Add notes", file="notes.txt", text="token fine\n")
    result = output(desk.call("push"))
    assert result["digest"] == digest(
        "github.push", {"push.branch": "main"} | commit_parts(desk, sha, "Add notes", "notes.txt\n")
    )


def test_the_push_sends_the_commit_checked_even_when_the_branch_moves_during_the_check(desk: Desk) -> None:
    desk.write("rubrics/plain.md", "Write plainly.\n")
    control = {
        "$type": "controls/control/v1",
        "requirement": "A test.",
        "purpose": "A test.",
        "rule": {"judged": "/rubrics/plain.md"},
    }
    desk.write("controls/plain.json", json.dumps(control))
    checks = [{"control": "/controls/plain", "parts": ["push.branch"]}]
    desk.write(
        "boundaries/octo.json",
        json.dumps({"$type": "boundary/boundary/v1", "to": "github:*", "actions": ["github.*"], "checks": checks}),
    )
    agent = desk.home / "agent"
    agent.write_text(
        "#!/bin/sh\ncat > /dev/null\n"
        f"git -C '{desk.checkout}' -c user.name=t -c user.email=t@t commit -q --allow-empty -m Later\n"
        "echo '{\"findings\": []}'\n"
    )
    agent.chmod(0o755)
    checked = desk.commit("Add a gear")
    result = output(desk.call("push", extra={"BOARD_CONTROLS_AGENT": json.dumps([str(agent)])}))
    assert result["commits"] == [checked]
    assert desk.remote_head("main") == checked
    assert git(desk.checkout, "log", "-1", "--format=%s") == "Later"


def test_a_push_url_unlike_the_fetch_url_is_refused_and_the_same_checkout_without_it_is_pushed(desk: Desk) -> None:
    desk.commit("Add a gear")
    git(desk.checkout, "remote", "set-url", "--push", "origin", "https://user:secret@github.com/me/widgets.git")
    failure = desk.call("push")
    assert (failure["exit"], failure["code"]) == (1, "github::remote::split")
    assert "secret" not in json.dumps(failure)
    git(desk.checkout, "config", "--unset", "remote.origin.pushurl")
    assert len(output(desk.call("push"))["commits"]) == 1


def test_tags_git_would_follow_are_not_pushed_and_the_branch_is(desk: Desk) -> None:
    git(desk.checkout, "config", "push.followTags", "true")
    sha = desk.commit("Add a gear")
    git(desk.checkout, "tag", "-a", "v1", "-m", "Release acme-123")
    output(desk.call("push"))
    assert desk.remote_head("main") == sha
    assert git(desk.remote, "tag") == ""


def test_an_origin_url_holding_a_password_is_never_quoted(desk: Desk) -> None:
    git(desk.checkout, "remote", "set-url", "origin", "https://user:secret@gitlab.example/octo/widgets.git")
    failure = desk.call("push")
    assert (failure["exit"], failure["code"]) == (1, "github::remote::not_github")
    assert "secret" not in json.dumps(failure)
    assert "https://gitlab.example/octo/widgets.git" in failure["message"]


def test_a_remote_git_cannot_reach_is_refused_and_a_reachable_one_is_pushed(desk: Desk) -> None:
    desk.commit("Add a gear")
    git(
        desk.checkout,
        "config",
        "--replace-all",
        f"url.{desk.home / 'missing.git'}.insteadOf",
        "https://github.com/Octo/Widgets.git",
    )
    git(desk.checkout, "config", "--unset", f"url.{desk.remote}.insteadOf")
    failure = desk.call("push")
    assert (failure["exit"], failure["code"]) == (1, "github::git::failed")
    git(desk.checkout, "config", "--unset", f"url.{desk.home / 'missing.git'}.insteadOf")
    git(desk.checkout, "config", f"url.{desk.remote}.insteadOf", "https://github.com/Octo/Widgets.git")
    assert len(output(desk.call("push"))["commits"]) == 1


def test_a_personal_board_without_boundary_is_refused_and_with_it_enabled_the_push_goes(desk: Desk) -> None:
    desk.commit("Add a gear")
    config = desk.personal / ".board" / "board.json"
    enabled = config.read_text()
    settings = json.loads(enabled)
    settings["plugins"] = [name for name in settings["plugins"] if name != "boundary"]
    config.write_text(json.dumps(settings))
    failure = desk.call("push")
    assert (failure["exit"], failure["code"]) == (1, "github::board::refused")
    assert failure["fields"]["code"] == "board::plugin::not_enabled"
    assert desk.remote_head("main") != git(desk.checkout, "rev-parse", "HEAD")
    config.write_text(enabled)
    assert len(output(desk.call("push"))["commits"]) == 1


def test_a_board_the_plugin_cannot_run_is_refused_and_one_it_can_run_is_used(desk: Desk) -> None:
    desk.commit("Add a gear")
    environment = desk.environment()
    environment["PATH"] = environment["PATH"].replace(str(board_binary().parent) + os.pathsep, "")
    ran = subprocess.run(
        [str(board_binary()), "--json", "call", "github.push"],
        cwd=desk.checkout,
        env=environment,
        capture_output=True,
        text=True,
        check=False,
    )
    assert (ran.returncode, json.loads(ran.stdout)["code"]) == (1, "github::board::failed")
    assert len(output(desk.call("push"))["commits"]) == 1


def test_a_single_branch_clone_pushes_a_new_branch_and_sets_its_upstream(desk: Desk) -> None:
    clone = desk.home / "clone"
    git(desk.home, "clone", "-q", "--single-branch", "-b", "main", str(desk.remote), str(clone))
    git(clone, "remote", "set-url", "origin", ORIGIN)
    git(clone, "config", f"url.{desk.remote}.insteadOf", ORIGIN)
    git(clone, "switch", "-q", "-c", "feature")
    git(clone, "commit", "-q", "--allow-empty", "-m", "Add a lever")
    desk.gh_answers("https://github.com/Octo/Widgets/pull/12\n")
    result = output(desk.call("pr-create", "--title", "Add a lever", "--body", "Levers.", cwd=clone, personal=True))
    assert result["number"] == PULL_NUMBER
    assert desk.remote_head("feature") == git(clone, "rev-parse", "HEAD")
    assert git(clone, "config", "branch.feature.merge") == "refs/heads/feature"


def test_a_commit_of_a_file_git_lfs_stores_is_refused_and_the_same_file_outside_lfs_is_pushed(desk: Desk) -> None:
    pointer = "version https://git-lfs.github.com/spec/v1\noid sha256:" + "0" * 64 + "\nsize 12\n"
    for attributes, stored in [("*.csv filter=lfs diff=lfs merge=lfs -text\n", True), ("*.csv text\n", False)]:
        (desk.checkout / ".gitattributes").write_text(attributes)
        git(desk.checkout, "add", ".gitattributes")
        sha = desk.commit("Add data", file="data.csv", text=pointer)
        before = desk.remote_head("main")
        result = desk.call("push")
        if stored:
            assert (result["exit"], result["code"]) == (1, "github::push::lfs")
            assert result["fields"]["paths"] == ["data.csv"]
            assert desk.remote_head("main") == before
            git(desk.checkout, "reset", "-q", "--hard", "HEAD~1")
        else:
            assert output(result)["commits"] == [sha]


def test_a_private_name_with_a_non_ascii_letter_in_a_file_name_is_refused_and_its_ascii_twin_is_pushed(
    desk: Desk,
) -> None:
    desk.refuse(["commit.*.diff"])
    desk.write("private/names.txt", "Müller\n")
    desk.commit("Add a list", file="Müller.txt", text="fine\n")
    assert desk.call("push")["fields"]["failures"][0]["text"] == "Müller"
    git(desk.checkout, "reset", "-q", "--hard", "HEAD~1")
    desk.commit("Add a list", file="Muller.txt", text="fine\n")
    assert len(output(desk.call("push"))["commits"]) == 1


def test_a_private_name_in_a_commit_header_is_refused_and_a_commit_without_it_is_pushed(desk: Desk) -> None:
    desk.refuse(["commit.*.headers"])
    git(desk.checkout, "-c", "i18n.commitEncoding=acme-123", "commit", "-q", "--allow-empty", "-m", "Empty")
    failure = desk.call("push")
    assert failure["fields"]["failures"][0]["part"].endswith(".headers")
    git(desk.checkout, "reset", "-q", "--hard", "HEAD~1")
    git(desk.checkout, "commit", "-q", "--allow-empty", "-m", "Empty")
    assert len(output(desk.call("push"))["commits"]) == 1


def test_git_hooks_and_gh_never_see_the_model_credentials_and_the_boundarys_agent_does(desk: Desk) -> None:
    hook = desk.checkout / ".git" / "hooks" / "pre-push"
    hook.write_text(f"#!/bin/sh\nenv > '{desk.home / 'hook-env.txt'}'\n")
    hook.chmod(0o755)
    desk.write("rubrics/plain.md", "Write plainly.\n")
    rule = {"judged": "/rubrics/plain.md"}
    desk.write(
        "controls/plain.json",
        json.dumps({"$type": "controls/control/v1", "requirement": "A test.", "purpose": "A test.", "rule": rule}),
    )
    checks = [{"control": "/controls/plain", "parts": ["*"]}]
    boundary = {"$type": "boundary/boundary/v1", "to": "github:*", "actions": ["github.*"], "checks": checks}
    desk.write("boundaries/octo.json", json.dumps(boundary))
    agent = desk.home / "agent"
    findings = '{"findings": []}'
    agent.write_text(f"#!/bin/sh\ncat > /dev/null\nenv > '{desk.home / 'agent-env.txt'}'\necho '{findings}'\n")
    agent.chmod(0o755)
    keys = {"ANTHROPIC_API_KEY": "model-key-1", "CLAUDE_CODE_OAUTH_TOKEN": "model-token-1"}
    extra = keys | {"BOARD_CONTROLS_AGENT": json.dumps([str(agent)])}
    desk.commit("Add a gear")
    output(desk.call("push", extra=extra))
    desk.gh_answers({"html_url": "https://github.com/Octo/Widgets/issues/7#issuecomment-1"})
    output(desk.call("comment", "--number", "7", "--body", "Fine.", extra=extra))
    for seen in [desk.home / "hook-env.txt", desk.bin / "gh-env.txt"]:
        assert not any(value in seen.read_text() for value in keys.values()), seen
    assert all(value in (desk.home / "agent-env.txt").read_text() for value in keys.values())
