"""`review.run` reviews a brief's range with the stub agent and records a report beside the brief."""

import json
import os
import re
import signal
import subprocess
import sys
import time
from collections.abc import Callable
from pathlib import Path

import pytest
from board_plugin.protocol import PluginError
from conftest import ISOLATED, Repo, answer, library

from board_review import run as run_module
from board_review.git import Repository
from board_review.run import _agent  # pyright: ignore[reportPrivateUsage]

SLEEP = 6
FINDING = {"severity": "high", "file": "src/sync.rs", "line": 3, "text": "The retry never stops."}


def changed(repo: Repo) -> tuple[str, str]:
    """Two commits on top of the fixture's: the base and head of the range under review."""
    base = repo.git("rev-parse", "HEAD")
    repo.commit("src/sync.rs", "fn sync() {}\n")
    head = repo.commit("src/sync.rs", "fn sync() {\n    loop {}\n}\n")
    return base, head


def test_a_run_writes_the_report_beside_the_brief(repo: Repo) -> None:
    base, head = changed(repo)
    brief = repo.brief("fix-sync", f"{base}..{head}")
    repo.answers(answer("changes", [FINDING, {"severity": "low", "text": "Name the loop."}]))
    code, output = repo.call("run", "--brief", brief)
    assert code == 0, output
    assert (output["range"], output["verdict"], output["findings"]) == (f"{base}..{head}", "changes", 2)
    assert output["report"] == "/reviews/fix-sync/report"
    report = repo.get(output["report"])
    expected = [plan_patch_id(repo, commit) for commit in repo.git("rev-list", "--reverse", f"{base}..{head}").split()]
    assert report["patch_ids"] == expected
    assert report["brief"] == brief
    assert report["findings"] == [FINDING, {"severity": "low", "text": "Name the loop."}]
    assert (report["verdict"], report["by"]) == ("changes", "sh")
    assert report["cost"]["seconds"] >= 0
    assert "input_tokens" not in report["cost"]
    assert sorted(path.name for path in (repo.root / "reviews" / "fix-sync").iterdir()) == ["brief.json", "report.json"]
    assert not (repo.root / "suites").exists()
    assert repo.run("check").returncode == 0


def plan_patch_id(repo: Repo, commit: str) -> str:
    """The patch id plan/review.md defines: `git diff-tree -p ... --full-index <commit> | git patch-id --verbatim`."""
    patch = subprocess.run(
        ["git", "diff-tree", "-p", "--no-renames", "--no-ext-diff", "--no-textconv", "--full-index", commit],
        cwd=repo.root,
        env=os.environ | ISOLATED,
        capture_output=True,
        check=True,
    ).stdout
    ran = subprocess.run(
        ["git", "patch-id", "--verbatim"],
        cwd=repo.root,
        input=patch,
        env=os.environ | ISOLATED,
        capture_output=True,
        check=True,
    )
    return ran.stdout.decode().split()[0] if ran.stdout else ""


def test_patch_ids_are_the_ones_the_plan_s_per_commit_command_gives(repo: Repo) -> None:
    base = repo.git("rev-parse", "HEAD")
    repo.commit("src/a.txt", "one\n  indented\n")
    repo.commit("src/a.txt", "one\n\tindented\n")
    repo.commit("src/b.bin", "\0binary\0")
    repo.git("mv", "src/a.txt", "src/moved.txt")
    repo.git("commit", "-qm", "move")
    repo.git("rm", "-q", "src/b.bin")
    repo.git("commit", "-qm", "delete")
    repo.git("commit", "-q", "--allow-empty", "-m", "nothing")
    commits = repo.git("rev-list", "--reverse", f"{base}..HEAD").split()
    expected = {commit: plan_patch_id(repo, commit) for commit in commits}
    assert Repository(str(repo.root)).patch_ids(commits) == {c: i for c, i in expected.items() if i}
    assert len([i for i in expected.values() if i]) == len(commits) - 1


def test_two_binary_changes_to_one_path_get_different_patch_ids_and_one_change_on_two_bases_gets_one(
    repo: Repo,
) -> None:
    base = repo.git("rev-parse", "HEAD")
    first = repo.commit("src/image.bin", "\0one\0")
    repo.git("reset", "-q", "--hard", base)
    second = repo.commit("src/image.bin", "\0two\0")
    repo.git("reset", "-q", "--hard", base)
    repo.commit("src/other.txt", "other\n")
    rebased = repo.commit("src/image.bin", "\0one\0")
    ids = Repository(str(repo.root)).patch_ids([first, second, rebased])
    assert ids[first] != ids[second]
    assert ids[first] == ids[rebased]


def test_the_agent_reads_the_focus_questions_and_diff_and_a_brief_s_follows_passes_to_the_report(repo: Repo) -> None:
    base, head = changed(repo)
    _, first = repo.call("run", "--brief", repo.brief("one", f"{base}..{head}"))
    brief = repo.brief("two", f"{base}..{head}", questions=["Does it stop?"], follows=first["report"])
    code, output = repo.call("run", "--brief", brief)
    assert code == 0, output
    prompt = repo.prompt()
    assert "Focus: Check the sync change." in prompt
    assert "- Does it stop?" in prompt
    assert "+    loop {}" in prompt
    assert repo.get(output["report"])["follows"] == first["report"]


def test_a_second_run_of_a_reviewed_brief_is_refused_and_a_brief_that_follows_it_runs(repo: Repo) -> None:
    base, head = changed(repo)
    brief = repo.brief("fix-sync", f"{base}..{head}")
    _, first = repo.call("run", "--brief", brief)
    code, document = repo.call("run", "--brief", brief)
    assert (code, document["code"], document["fields"]["report"]) == (1, "review::report::exists", first["report"])
    code, second = repo.call("run", "--brief", repo.brief("fix-sync-2", f"{base}..{head}", follows=first["report"]))
    assert (code, second["report"]) == (0, "/reviews/fix-sync-2/report"), second


def test_claude_s_json_envelope_gives_the_answer_tokens_and_model(repo: Repo) -> None:
    base, head = changed(repo)
    envelope: dict[str, object] = {
        "type": "result",
        "result": answer(),
        "usage": {
            "input_tokens": 10,
            "cache_creation_input_tokens": 5,
            "cache_read_input_tokens": 100,
            "output_tokens": 40,
        },
        "modelUsage": {"claude-test-model": {}},
    }
    repo.answers(json.dumps(envelope))
    code, output = repo.call("run", "--brief", repo.brief("fix-sync", f"{base}..{head}"))
    assert code == 0, output
    report = repo.get(output["report"])
    assert report["by"] == "claude-test-model"
    tokens = (report["cost"]["input_tokens"], report["cost"]["output_tokens"])
    assert tokens == (10 + 5 + 100, 40)


def test_the_agent_sees_the_variables_the_manifest_names_and_not_others(repo: Repo) -> None:
    base, head = changed(repo)
    brief = repo.brief("fix-sync", f"{base}..{head}")
    code, _ = repo.call("run", "--brief", brief, extra={"ANTHROPIC_API_KEY": "test-key", "OTHER_SECRET": "x"})
    assert code == 0
    seen = (repo.agent / "env.txt").read_text().splitlines()
    assert "ANTHROPIC_API_KEY=test-key" in seen
    assert not any(line.startswith("OTHER_SECRET=") for line in seen)


def failed(repo: Repo, brief: str) -> str:
    """The code of a refused run of `brief`, after checking it wrote no report."""
    code, document = repo.call("run", "--brief", brief)
    assert code == 1, document
    assert not (repo.root / f"{brief.removesuffix('brief').removeprefix('/')}report.json").exists()
    return str(document["code"])


def test_a_brief_that_is_not_there_is_refused_and_one_that_is_runs(repo: Repo) -> None:
    base, head = changed(repo)
    repo.brief("fix-sync", f"{base}..{head}")
    assert failed(repo, "/reviews/missing/brief") == "review::brief::not_found"
    assert repo.call("run", "--brief", "/reviews/fix-sync/brief")[0] == 0


def test_a_range_git_cannot_resolve_is_refused_and_one_it_can_runs(repo: Repo) -> None:
    base, head = changed(repo)
    assert failed(repo, repo.brief("bad", f"{'0' * 40}..{head}")) == "review::range::unknown"
    assert repo.call("run", "--brief", repo.brief("good", f"{base}..{head}"))[0] == 0


def test_a_range_of_names_or_options_is_invalid_and_one_of_full_commit_ids_runs(repo: Repo) -> None:
    base, head = changed(repo)
    written = repo.root / "written"
    for range_ in ["main..HEAD", f"{base}..--output={written}", f"{base[:12]}..{head}"]:
        assert failed(repo, repo.brief("named", range_)) == "review::brief::not_found", range_
    assert not written.exists()
    code, output = repo.call("run", "--brief", repo.brief("good", f"{base}..{head}"))
    assert code == 0, output


def test_an_agent_that_fails_is_refused_and_one_that_succeeds_runs(repo: Repo) -> None:
    base, head = changed(repo)
    brief = repo.brief("fix-sync", f"{base}..{head}")
    repo.answers(answer(), exit_code=3)
    code, document = repo.call("run", "--brief", brief)
    assert (code, document["code"], document["fields"]["exit"]) == (1, "review::agent::failed", 3)
    repo.answers(answer(), exit_code=0)
    assert repo.call("run", "--brief", brief)[0] == 0


def test_an_agent_that_fails_with_its_error_on_stdout_carries_that_error_in_the_message(repo: Repo) -> None:
    base, head = changed(repo)
    brief = repo.brief("fix-sync", f"{base}..{head}")
    repo.answers('{"type": "result", "is_error": true, "result": "Credit balance is too low"}', exit_code=1)
    code, document = repo.call("run", "--brief", brief)
    assert (code, document["code"]) == (1, "review::agent::failed")
    assert "Credit balance is too low" in document["message"]


def test_an_answer_without_a_valid_verdict_and_findings_is_refused_and_a_valid_one_runs(repo: Repo) -> None:
    base, head = changed(repo)
    brief = repo.brief("fix-sync", f"{base}..{head}")
    for stdout in [
        "No JSON here.",
        answer("maybe"),
        answer("approve", [{"severity": "urgent", "text": "x"}]),
        answer("approve", [{"severity": "low", "text": ""}]),
    ]:
        repo.answers(stdout)
        assert failed(repo, brief) == "review::agent::bad_output", stdout
    repo.answers(answer("approve", [{"severity": "low", "text": "x"}]))
    assert repo.call("run", "--brief", brief)[0] == 0


def test_an_agent_that_cannot_start_is_refused_and_one_that_can_runs(repo: Repo) -> None:
    base, head = changed(repo)
    brief = repo.brief("fix-sync", f"{base}..{head}")
    missing = {"BOARD_REVIEW_AGENT": json.dumps([str(repo.agent / "no-such-agent")])}
    code, document = repo.call("run", "--brief", brief, extra=missing)
    assert (code, document["code"]) == (1, "review::agent::not_started")
    assert repo.call("run", "--brief", brief)[0] == 0


def test_an_agent_variable_unset_or_not_an_array_of_strings_is_refused_and_one_that_is_runs(repo: Repo) -> None:
    base, head = changed(repo)
    brief = repo.brief("fix-sync", f"{base}..{head}")
    code, document = repo.call("run", "--brief", brief, extra={"BOARD_REVIEW_AGENT": ""})
    assert (code, document["code"]) == (1, "review::agent::not_set")
    for value in ["sh agent.sh", "[]", '["", "agent.sh"]', '"sh"', "[1]"]:
        code, document = repo.call("run", "--brief", brief, extra={"BOARD_REVIEW_AGENT": value})
        assert (code, document["code"]) == (1, "review::agent::invalid"), value
    assert repo.call("run", "--brief", brief)[0] == 0


def test_the_readme_s_agent_with_an_empty_argument_runs_and_one_with_an_empty_program_is_refused(repo: Repo) -> None:
    base, head = changed(repo)
    brief = repo.brief("fix-sync", f"{base}..{head}")
    readme = (Path(__file__).resolve().parents[1] / "README.md").read_text()
    found = re.search(r"export BOARD_REVIEW_AGENT='(.*)'", readme)
    assert found is not None, "README.md shows BOARD_REVIEW_AGENT"
    example: list[str] = json.loads(found.group(1))
    assert example[0] == "claude"
    assert "" in example[1:]
    # A `claude` on PATH that records its argv, one argument a line, and runs the stub agent.
    claude = repo.bin / "claude"
    claude.write_text(f'#!/bin/sh\nprintf "%s\\n" "$@" > "{repo.agent}/argv.txt"\nexec sh "{repo.agent}/agent.sh"\n')
    claude.chmod(0o755)
    empty_program = json.dumps(["", *example[1:]])
    code, document = repo.call("run", "--brief", brief, extra={"BOARD_REVIEW_AGENT": empty_program})
    assert (code, document["code"]) == (1, "review::agent::invalid")
    code, output = repo.call("run", "--brief", brief, extra={"BOARD_REVIEW_AGENT": json.dumps(example)})
    assert code == 0, output
    assert (repo.agent / "argv.txt").read_text().split("\n")[:-1] == example[1:]


def test_an_agent_named_in_the_board_s_tree_is_never_started(repo: Repo) -> None:
    base, head = changed(repo)
    brief = repo.brief("fix-sync", f"{base}..{head}")
    marker = repo.root / "started"
    repo.commit("reviews/agent.json", json.dumps({"command": ["touch", str(marker)]}))
    code, document = repo.call("run", "--brief", brief, extra={"BOARD_REVIEW_AGENT": ""})
    assert (code, document["code"]) == (1, "review::agent::not_set")
    assert not marker.exists()
    assert repo.call("run", "--brief", brief)[0] == 0


def test_a_brief_reached_through_a_mount_is_refused_and_one_in_the_board_s_own_tree_runs(
    repo: Repo, tmp_path: Path
) -> None:
    base, head = changed(repo)
    brief = json.dumps({"$type": "review/brief/v1", "range": f"{base}..{head}", "focus": "Check it."})
    repo.commit("reviews/shared.json", json.dumps(library(tmp_path / "shared", {"brief.json": brief})))
    assert repo.run("get", "/reviews/shared/brief").returncode == 0
    assert failed(repo, "/reviews/shared/brief") == "review::brief::not_found"
    repo.commit("reviews/own/brief.json", brief)
    assert repo.call("run", "--brief", "/reviews/own/brief")[0] == 0


def test_a_base_that_moved_after_the_branch_point_is_replaced_by_the_merge_base(repo: Repo) -> None:
    point = repo.git("rev-parse", "HEAD")
    repo.git("checkout", "-q", "-b", "change")
    head = repo.commit("src/sync.rs", "fn sync() {}\n")
    repo.git("checkout", "-q", "main")
    moved = repo.commit("src/later.rs", "fn later() {}\n")
    repo.git("checkout", "-q", "change")
    code, output = repo.call("run", "--brief", repo.brief("fix-sync", f"{moved}..{head}"))
    assert code == 0, output
    assert output["range"] == f"{point}..{head}"
    assert "later" not in repo.prompt()
    assert "+fn sync() {}" in repo.prompt()


def test_an_agent_past_its_time_is_stopped_and_one_within_it_answers(tmp_path: Path) -> None:
    sleeper = ["sh", "-c", f"sleep {SLEEP}; echo late"]
    started = time.monotonic()
    with pytest.raises(PluginError) as refused:
        _agent(sleeper, "", str(tmp_path), 0.2)
    assert refused.value.code == "review::agent::timeout"
    assert time.monotonic() - started < SLEEP / 2, "the sleep the agent started was not stopped with it"
    assert _agent(["sh", "-c", "echo soon"], "", str(tmp_path), 5) == "soon\n"


def soon(condition: Callable[[], bool]) -> bool:
    """Whether `condition` holds within five seconds."""
    deadline = time.monotonic() + 5
    while not condition():
        if time.monotonic() > deadline:
            return False
        time.sleep(0.05)
    return True


def gone(pid: int) -> bool:
    """Whether the process `pid` stops within five seconds."""

    def stopped() -> bool:
        try:
            os.kill(pid, 0)
        except ProcessLookupError:
            return True
        return False

    return soon(stopped)


@pytest.mark.parametrize("signum", [signal.SIGTERM, signal.SIGINT, signal.SIGHUP])
def test_a_signal_that_stops_the_plugin_stops_the_agent_and_what_it_started(tmp_path: Path, signum: int) -> None:
    pidfile = tmp_path / "pid"
    command = ["sh", "-c", f"sleep {SLEEP} & echo $! > {pidfile}; wait"]
    script = f"from board_review.run import _agent; _agent({command!r}, '', {str(tmp_path)!r}, {SLEEP})"
    plugin = subprocess.Popen([sys.executable, "-c", script])
    assert soon(lambda: pidfile.exists() and pidfile.read_text().strip() != "")
    plugin.send_signal(signum)
    assert plugin.wait(timeout=10) == 128 + signum
    assert gone(int(pidfile.read_text()))


def test_a_process_the_agent_leaves_behind_is_stopped_when_the_agent_exits(tmp_path: Path) -> None:
    out = _agent(["sh", "-c", f"sleep {SLEEP} >/dev/null 2>&1 & echo $!"], "", str(tmp_path), 5)
    assert gone(int(out))


def test_an_agent_whose_escaped_child_holds_its_output_is_stopped_after_the_drain(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setattr(run_module, "DRAIN", 0.5)
    pidfile = tmp_path / "pid"
    escape = (
        "import subprocess, sys, time\n"
        f"sleeper = [sys.executable, '-c', 'import time; time.sleep({SLEEP})']\n"
        "child = subprocess.Popen(sleeper, start_new_session=True)\n"
        f"open({str(pidfile)!r}, 'w').write(str(child.pid))\n"
        f"time.sleep({SLEEP})\n"
    )
    started = time.monotonic()
    with pytest.raises(PluginError) as refused:
        _agent([sys.executable, "-c", escape], "", str(tmp_path), 1)
    assert refused.value.code == "review::agent::timeout"
    assert time.monotonic() - started < SLEEP / 2, "the plugin waited for the escaped child's output"
    os.kill(int(pidfile.read_text()), signal.SIGKILL)


def test_the_agent_starts_in_an_empty_directory_away_from_the_board_s_mcp_servers(repo: Repo) -> None:
    base, head = changed(repo)
    repo.write(".mcp.json", json.dumps({"mcpServers": {"tasks": {"command": "tasks-server"}}}))
    code, output = repo.call("run", "--brief", repo.brief("fix-sync", f"{base}..{head}"))
    assert code == 0, output
    directory, *listed = (repo.agent / "directory.txt").read_text().splitlines()
    assert listed == []
    assert not Path(directory).resolve().is_relative_to(repo.root.resolve())
    assert not Path(directory).exists()


def clone_of_app(repo: Repo, tmp_path: Path) -> tuple[Path, str, str]:
    """A repository `app` with two commits, recorded in the board as /repos/app without a checkout, and a clone of it.

    Returns the clone and the range of the second commit.
    """
    origin = tmp_path / "app"
    library(origin, {"README.md": "An app.\n"})
    git = ["git", "-c", "user.name=t", "-c", "user.email=t@t"]
    base = subprocess.run(
        [*git, "rev-parse", "HEAD"], cwd=origin, env=os.environ | ISOLATED, capture_output=True, text=True, check=True
    ).stdout.strip()
    (origin / "app.rs").write_text("fn app() {}\n")
    for arguments in (["add", "."], ["commit", "-qm", "app"]):
        subprocess.run([*git, *arguments], cwd=origin, env=os.environ | ISOLATED, check=True)
    head = subprocess.run(
        [*git, "rev-parse", "HEAD"], cwd=origin, env=os.environ | ISOLATED, capture_output=True, text=True, check=True
    ).stdout.strip()
    clone = tmp_path / "app-clone"
    subprocess.run([*git, "clone", "-q", "--local", str(origin), str(clone)], env=os.environ | ISOLATED, check=True)
    subprocess.run(
        [*git, "remote", "set-url", "origin", "git@github.com:example/app.git"],
        cwd=clone,
        env=os.environ | ISOLATED,
        check=True,
    )
    repo.commit("repos/app.json", json.dumps({"$type": "board/repo/v1", "remote": "example/app"}))
    return clone, base, head


def test_a_brief_naming_a_repo_reads_its_range_in_the_callers_clone_and_one_run_elsewhere_is_refused(
    repo: Repo, tmp_path: Path
) -> None:
    clone, base, head = clone_of_app(repo, tmp_path)
    brief = repo.brief("app", f"{base}..{head}", repo="/repos/app", step="review")
    code, refused = repo.call("run", "--brief", brief)
    assert (code, refused["code"]) == (1, "review::repo::not_caller")
    code, output = repo.call("run", "--brief", brief, cwd=clone)
    assert code == 0, output
    assert output["range"] == f"{base}..{head}"
    assert "+fn app() {}" in repo.prompt()
