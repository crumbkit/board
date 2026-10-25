"""`loop.start`: a new change, its plan, its route and the repository's brief."""

from conftest import Board, mount_repo, record

CONTRIBUTING = "Run the checks.\n\nThe check suite is `/suites/checks`.\n"


def test_start_creates_the_change_in_shape_with_a_plan_whose_sections_are_left_to_fill_in(board: Board) -> None:
    repo = mount_repo(board, {"README.md": "x"})
    output = board.call("start", {"repo": repo, "title": "Let a suite declare a timeout!"})
    assert (output["path"], output["plan"]) == (
        "/changes/let-a-suite-declare-a-timeout",
        "/changes/let-a-suite-declare-a-timeout/plan.md",
    )
    assert board.read("changes/let-a-suite-declare-a-timeout.json") == {
        "$type": "loop/change/v1",
        "title": "Let a suite declare a timeout!",
        "repo": "/repos/x",
        "phase": "shape",
        "plan": "/changes/let-a-suite-declare-a-timeout/plan.md",
        "route": "proactive",
    }
    plan = (board.root / "changes/let-a-suite-declare-a-timeout/plan.md").read_text()
    assert plan == "# Let a suite declare a timeout!\n\n## Problem\n\n## Outcome\n\n## Out of scope\n"


def test_start_keeps_a_plan_already_written_at_the_plan_path(board: Board) -> None:
    repo = mount_repo(board, {"README.md": "x"})
    board.write("changes/t/plan.md", "# Mine\n")
    board.call("start", {"repo": repo, "title": "T"})
    assert (board.root / "changes/t/plan.md").read_text() == "# Mine\n"


def test_the_route_is_owner_when_the_handle_owns_every_owned_pattern(board: Board) -> None:
    repo = mount_repo(board, {".github/CODEOWNERS": "# owners\n* @luke\ndocs/ @Luke  # docs too\n"})
    output = board.call("start", {"repo": repo, "title": "T", "handle": "@luke"})
    assert (output["route"], output["ask_first"]) == ("owner", [])
    assert output["codeowners"] == "/repos/x/.github/CODEOWNERS"
    assert output["owners"] == [{"pattern": "*", "owners": ["@luke"]}, {"pattern": "docs/", "owners": ["@Luke"]}]


def test_the_route_is_ask_first_and_lists_the_patterns_someone_else_owns(board: Board) -> None:
    repo = mount_repo(board, {"CODEOWNERS": "* @luke\n/auth/ @sam @kim\n"})
    output = board.call("start", {"repo": repo, "title": "T", "handle": "@luke"})
    assert (output["route"], output["ask_first"]) == ("ask-first", ["/auth/"])
    assert board.read("changes/t.json")["route"] == "ask-first"


def test_without_a_handle_every_owned_pattern_is_ask_first(board: Board) -> None:
    repo = mount_repo(board, {"CODEOWNERS": "* @luke\n"})
    output = board.call("start", {"repo": repo, "title": "T"})
    assert (output["route"], output["ask_first"]) == ("ask-first", ["*"])


def test_the_route_is_proactive_when_the_repository_names_no_owners(board: Board) -> None:
    repo = mount_repo(board, {"CODEOWNERS": "# nobody yet\n"})
    output = board.call("start", {"repo": repo, "title": "T", "handle": "@luke"})
    assert (output["route"], output["owners"], output["codeowners"]) == ("proactive", [], "/repos/x/CODEOWNERS")


def test_the_brief_lists_the_contributing_docs_and_the_check_suite_they_name(board: Board) -> None:
    repo = mount_repo(board, {"CONTRIBUTING.md": CONTRIBUTING, "AGENTS.md": "Read `/nowhere`.\n"})
    board.write("suites/checks.json", record("runs/suite/v1", subjects=["x"], method="Run it."))
    output = board.call("start", {"repo": repo, "title": "T"})
    assert output["docs"] == ["/repos/x/CONTRIBUTING.md", "/repos/x/AGENTS.md"]
    assert output["suite"] == "/suites/checks"


def test_a_contributing_doc_in_the_github_directory_is_found(board: Board) -> None:
    repo = mount_repo(board, {".github/CONTRIBUTING.md": CONTRIBUTING})
    board.write("suites/checks.json", record("runs/suite/v1", subjects=["x"], method="Run it."))
    output = board.call("start", {"repo": repo, "title": "T"})
    assert (output["docs"], output["suite"]) == (["/repos/x/.github/CONTRIBUTING.md"], "/suites/checks")


def test_a_suite_in_the_repositorys_own_board_is_found_before_one_in_this_board(board: Board) -> None:
    suite = record("runs/suite/v1", subjects=["x"], method="Run it.")
    files = {
        "CONTRIBUTING.md": CONTRIBUTING,
        "suites/checks.json": suite,
        ".board/types/runs/suite/v1.schema.json": "{}",
    }
    repo = mount_repo(board, files)
    board.write("suites/checks.json", suite)
    assert board.call("start", {"repo": repo, "title": "T"})["suite"] == "/repos/x/suites/checks"


def test_a_named_path_that_holds_no_suite_is_not_the_check_suite(board: Board) -> None:
    repo = mount_repo(board, {"CONTRIBUTING.md": CONTRIBUTING})
    board.write("suites/checks.json", record("plans/decision/v1", refs=[]))
    assert board.call("start", {"repo": repo, "title": "T"})["suite"] is None


def test_start_refuses_a_name_a_change_already_has_and_admits_another(board: Board) -> None:
    repo = mount_repo(board, {"README.md": "x"})
    board.call("start", {"repo": repo, "title": "T"})
    refused = board.refusal("start", {"repo": repo, "title": "T"})
    assert refused["code"] == "loop::change::exists"
    assert board.call("start", {"repo": repo, "title": "T", "name": "t-2"})["path"] == "/changes/t-2"


def test_start_refuses_a_repository_path_that_holds_nothing_and_admits_a_mounted_one(board: Board) -> None:
    mount_repo(board, {"README.md": "x"})
    assert board.refusal("start", {"repo": "/repos/y", "title": "T"})["code"] == "loop::repo::not_found"
    assert board.call("start", {"repo": "/repos/x", "title": "T"})["path"] == "/changes/t"
