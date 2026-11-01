"""`changes.start`: a change to the caller's repository, its repo record when it has none, and its first claim."""

from pathlib import Path

from conftest import Board, clone, git, mount, record

CONTRIBUTING = "Run the checks.\n\nThe check suite is `/suites/checks`.\n"
LINK = '{"$type": "board/mount/v1", "path": "/workflows/personal"}'


def ready(board: Board) -> None:
    """The seed workflows, a suite, and a link choosing the personal workflow for the owner `example`."""
    board.call("seed", {})
    board.write("suites/checks.json", record("runs/suite/v1", subjects=["app"], method="Run it."))
    board.write("workflows/owners/example.json", LINK)


def given(**more: str) -> dict[str, str]:
    """`start`'s input: a title, a session and the suite, with `more`."""
    return {"title": "Let a suite declare a timeout", "session": "local_1", "suite": "/suites/checks", **more}


def test_start_in_an_unmounted_clone_creates_the_repo_record_once_and_a_second_start_reuses_it(board: Board) -> None:
    ready(board)
    cloned = clone(board)
    first = board.call("start", given(), cwd=cloned.path)
    path = "/changes/let-a-suite-declare-a-timeout"
    assert (first["path"], first["repo"], first["repo_created"]) == (path, "/repos/app", True)
    assert board.read("repos/app.json") == {"$type": "board/repo/v1", "remote": "example/app"}
    assert board.read("changes/let-a-suite-declare-a-timeout.json") == {
        "$type": "changes/change/v2",
        "title": "Let a suite declare a timeout",
        "repo": "/repos/app",
        "branch": "work",
        "workflow": "/workflows/personal",
        "suite": "/suites/checks",
        "route": "unread",
    }
    second = board.call("start", given(name="another"), cwd=cloned.path)
    assert (second["repo"], second["repo_created"]) == ("/repos/app", False)
    assert board.listed("repos") == ["app.json"]


def test_start_writes_a_first_claim_naming_the_session_and_the_clones_root(board: Board) -> None:
    ready(board)
    cloned = clone(board)
    (cloned.path / "src").mkdir()
    output = board.call("start", given(name="t"), cwd=cloned.path / "src")
    claim = board.read(output["claim"].removeprefix("/") + ".json")
    assert claim == {"$type": "changes/claim/v1", "session": "local_1", "worktree": str(cloned.path.resolve())}


def test_start_refuses_the_default_branch_and_admits_a_work_branch(board: Board) -> None:
    ready(board)
    cloned = clone(board, branch="main")
    refused = board.refusal("start", given(), cwd=cloned.path)
    assert (refused["code"], refused["fields"]["branch"]) == ("changes::branch::default", "main")
    git(cloned.path, "switch", "-qc", "work")
    assert board.call("start", given(), cwd=cloned.path)["branch"] == "work"


def test_start_refuses_a_detached_head_and_admits_a_branch(board: Board) -> None:
    ready(board)
    cloned = clone(board)
    git(cloned.path, "switch", "-q", "--detach")
    assert board.refusal("start", given(), cwd=cloned.path)["code"] == "changes::branch::detached"
    git(cloned.path, "switch", "-q", "work")
    assert board.call("start", given(), cwd=cloned.path)["branch"] == "work"


def test_start_refuses_a_clone_without_an_origin_and_admits_one_with_it(board: Board) -> None:
    ready(board)
    cloned = clone(board)
    git(cloned.path, "remote", "remove", "origin")
    assert board.refusal("start", given(), cwd=cloned.path)["code"] == "changes::clone::no_remote"
    git(cloned.path, "remote", "add", "origin", cloned.remote)
    assert board.call("start", given(), cwd=cloned.path)["repo"] == "/repos/app"


def test_start_outside_a_git_clone_is_refused_and_inside_one_is_admitted(board: Board, tmp_path: Path) -> None:
    ready(board)
    outside = tmp_path / "outside"
    outside.mkdir()
    assert board.refusal("start", given(), cwd=outside)["code"] == "changes::clone::none"
    assert board.call("start", given(), cwd=clone(board).path)["repo"] == "/repos/app"


def test_a_repo_name_another_remote_holds_is_refused_and_another_name_is_admitted(board: Board) -> None:
    ready(board)
    board.write("repos/app.json", record("board/repo/v1", remote="other/app"))
    cloned = clone(board)
    refused = board.refusal("start", given(), cwd=cloned.path)
    assert (refused["code"], refused["fields"]["repo"]) == ("changes::repo::name_taken", "/repos/app")
    output = board.call("start", given(**{"repo-name": "example-app"}), cwd=cloned.path)
    assert (output["repo"], output["repo_created"]) == ("/repos/example-app", True)


def test_a_repository_link_chooses_the_workflow_before_its_owners_link(board: Board) -> None:
    ready(board)
    board.write("workflows/repos/app.json", '{"$type": "board/mount/v1", "path": "/workflows/work"}')
    cloned = clone(board)
    assert board.call("start", given(name="a"), cwd=cloned.path)["workflow"] == "/workflows/work"
    (board.root / "workflows/repos/app.json").unlink()
    assert board.call("start", given(name="b"), cwd=cloned.path)["workflow"] == "/workflows/personal"


def test_with_no_link_start_refuses_naming_both_links_and_admits_a_workflow_given(board: Board) -> None:
    ready(board)
    (board.root / "workflows/owners/example.json").unlink()
    cloned = clone(board)
    refused = board.refusal("start", given(), cwd=cloned.path)
    assert refused["code"] == "changes::workflow::unchosen"
    assert refused["fields"]["candidates"] == ["/workflows/repos/app", "/workflows/owners/example"]
    assert board.call("start", given(workflow="/workflows/work"), cwd=cloned.path)["workflow"] == "/workflows/work"


def test_a_workflow_given_that_holds_no_workflow_is_refused_and_one_that_does_is_admitted(board: Board) -> None:
    ready(board)
    cloned = clone(board)
    refused = board.refusal("start", given(workflow="/workflows/none"), cwd=cloned.path)
    assert (refused["code"], refused["fields"]["workflow"]) == ("changes::workflow::not_found", "/workflows/none")
    assert board.call("start", given(workflow="/workflows/work"), cwd=cloned.path)["workflow"] == "/workflows/work"


def test_without_a_suite_start_refuses_and_with_one_it_is_admitted(board: Board) -> None:
    ready(board)
    cloned = clone(board)
    asked = given()
    del asked["suite"]
    assert board.refusal("start", asked, cwd=cloned.path)["code"] == "changes::suite::unknown"
    refused = board.refusal("start", given(suite="/suites/none"), cwd=cloned.path)
    assert (refused["code"], refused["fields"]["suite"]) == ("changes::suite::unknown", "/suites/none")
    assert board.call("start", given(), cwd=cloned.path)["suite"] == "/suites/checks"


def test_in_a_mounted_clone_the_route_comes_from_codeowners_and_the_suite_from_the_contributing_docs(
    board: Board,
) -> None:
    ready(board)
    files = {"CODEOWNERS": "* @luke\n/auth/ @sam\n", "CONTRIBUTING.md": CONTRIBUTING}
    cloned = clone(board, files)
    mount(board, cloned)
    asked = given(handle="@luke")
    del asked["suite"]
    output = board.call("start", asked, cwd=cloned.path)
    assert (output["repo"], output["repo_created"], output["route"]) == ("/repos/app", False, "ask-first")
    assert (output["ask_first"], output["codeowners"]) == (["/auth/"], "/repos/app/CODEOWNERS")
    assert (output["docs"], output["suite"]) == (["/repos/app/CONTRIBUTING.md"], "/suites/checks")


def test_start_refuses_a_name_a_change_already_has_and_admits_another(board: Board) -> None:
    ready(board)
    cloned = clone(board)
    board.call("start", given(name="t"), cwd=cloned.path)
    assert board.refusal("start", given(name="t"), cwd=cloned.path)["code"] == "changes::change::exists"
    assert board.call("start", given(name="t-2"), cwd=cloned.path)["path"] == "/changes/t-2"


def test_start_records_the_issue_when_one_is_given(board: Board) -> None:
    ready(board)
    issue = "https://github.com/example/app/issues/2"
    output = board.call("start", given(name="t", issue=issue), cwd=clone(board).path)
    assert board.read(output["path"].removeprefix("/") + ".json")["issue"] == issue


def test_a_name_or_repo_name_outside_the_pattern_is_refused_and_one_inside_is_admitted(board: Board) -> None:
    ready(board)
    cloned = clone(board)
    for bad in ({"name": "Has Spaces"}, {"repo-name": "Example.App"}):
        assert board.refusal("start", given(**bad), cwd=cloned.path)["code"].startswith("board::")
    output = board.call("start", given(name="timeout", **{"repo-name": "example-app"}), cwd=cloned.path)
    assert (output["path"], output["repo"]) == ("/changes/timeout", "/repos/example-app")
