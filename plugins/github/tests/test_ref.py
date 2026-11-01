"""`github.ref`: one pull request or issue, asked of gh once, written as a record once."""

import json

from conftest import Board, issue


def test_a_pull_request_is_written_at_its_pulls_path_with_its_title(board: Board) -> None:
    board.gh_answers(issue(12, "Add a widget", pull=True))
    output = board.call("octo/widgets#12")
    assert output == {
        "path": "/gh/octo/widgets/pulls/12",
        "type": "github/pull/v1",
        "written": True,
        "url": "https://github.com/octo/widgets/pull/12",
        "title": "Add a widget",
    }
    assert board.read("gh/octo/widgets/pulls/12.json") == {
        "$type": "github/pull/v1",
        "owner": "octo",
        "repo": "widgets",
        "number": 12,
        "url": "https://github.com/octo/widgets/pull/12",
        "title": "Add a widget",
    }
    assert board.gh_argv() == ["api", "repos/octo/widgets/issues/12"]


def test_an_issue_is_written_at_its_issues_path(board: Board) -> None:
    board.gh_answers(issue(7))
    output = board.call("octo/widgets#7")
    assert (output["path"], output["type"], output["written"]) == ("/gh/octo/widgets/issues/7", "github/issue/v1", True)
    assert board.read("gh/octo/widgets/issues/7.json") == {
        "$type": "github/issue/v1",
        "owner": "octo",
        "repo": "widgets",
        "number": 7,
        "url": "https://github.com/octo/widgets/issues/7",
        "title": "Widgets wobble",
    }


def test_a_second_ref_writes_nothing_asks_gh_nothing_and_reports_the_record_already_there(board: Board) -> None:
    board.gh_answers(issue(12, "Add a widget", pull=True))
    first = board.call("octo/widgets#12")
    written = (board.root / "gh/octo/widgets/pulls/12.json").read_bytes()
    board.gh_answers(issue(12, "Renamed since", pull=True))
    again = board.call("octo/widgets#12")
    assert again == first | {"written": False}
    assert (board.root / "gh/octo/widgets/pulls/12.json").read_bytes() == written
    assert board.gh_calls() == 1


def test_an_issue_already_recorded_is_reported_without_asking_gh(board: Board) -> None:
    board.write(
        "gh/octo/widgets/issues/7.json",
        json.dumps(
            {
                "$type": "github/issue/v1",
                "owner": "octo",
                "repo": "widgets",
                "number": 7,
                "url": "https://github.com/octo/widgets/issues/7",
            }
        ),
    )
    output = board.call("octo/widgets#7")
    assert output == {
        "path": "/gh/octo/widgets/issues/7",
        "type": "github/issue/v1",
        "written": False,
        "url": "https://github.com/octo/widgets/issues/7",
        "title": None,
    }
    assert board.gh_calls() == 0


def test_an_empty_title_is_left_out_of_the_record(board: Board) -> None:
    board.gh_answers(issue(7, ""))
    assert board.call("octo/widgets#7")["title"] is None
    assert "title" not in board.read("gh/octo/widgets/issues/7.json")


def test_owner_and_repository_are_stored_in_lower_case_and_the_url_as_github_gives_it(board: Board) -> None:
    board.gh_answers(issue(12, pull=True) | {"html_url": "https://github.com/Octo/Widgets/pull/12"})
    assert board.call("Octo/Widgets#12")["path"] == "/gh/octo/widgets/pulls/12"
    record = board.read("gh/octo/widgets/pulls/12.json")
    assert (record["owner"], record["repo"], record["url"]) == (
        "octo",
        "widgets",
        "https://github.com/Octo/Widgets/pull/12",
    )


def test_a_ref_differing_only_in_case_finds_the_same_record_without_asking_gh(board: Board) -> None:
    board.gh_answers(issue(12, pull=True))
    first = board.call("octo/widgets#12")
    assert board.call("Octo/WIDGETS#12") == first | {"written": False}
    assert board.gh_calls() == 1


def test_a_hand_written_record_in_capitals_is_degraded_and_one_in_lower_case_is_reported(board: Board) -> None:
    record = {"$type": "github/issue/v1", "number": 7, "url": "https://github.com/octo/widgets/issues/7"}
    board.write("gh/octo/widgets/issues/7.json", json.dumps(record | {"owner": "Octo", "repo": "widgets"}))
    assert board.refusal("octo/widgets#7")["code"] == "github::record::degraded"
    board.write("gh/octo/widgets/issues/7.json", json.dumps(record | {"owner": "octo", "repo": "widgets"}))
    assert board.call("octo/widgets#7")["written"] is False


def test_each_malformed_ref_is_refused_and_its_nearest_well_formed_twin_is_admitted(board: Board) -> None:
    pairs = [
        ("octo/widgets#0", "octo/widgets#1"),
        ("octo/widgets", "octo/widgets#2"),
        ("octo#3", "octo/octo#3"),
        ("octo/widgets/x#4", "octo/widgets-x#4"),
        ("octo/..#5", "octo/.github#5"),
    ]
    for bad, good in pairs:
        assert board.refusal(bad)["code"] == "board::plugin::bad_input", bad
        owner, repo_number = good.split("/")
        repo, number = repo_number.split("#")
        board.gh_answers(issue(int(number)) | {"html_url": f"https://github.com/{owner}/{repo}/issues/{number}"})
        assert board.call(good)["path"] == f"/gh/{owner}/{repo}/issues/{number}", good
    assert board.gh_calls() == len(pairs)


def test_a_number_github_does_not_know_is_refused_and_one_it_knows_is_admitted(board: Board) -> None:
    board.gh_answers('{"message":"Not Found"}', code=1, stderr="gh: Not Found (HTTP 404)\n")
    failure = board.refusal("octo/widgets#99")
    assert failure["code"] == "github::ref::not_found"
    assert "octo/widgets#99" in failure["message"]
    assert not (board.root / "gh").exists()
    board.gh_answers(issue(99))
    assert board.call("octo/widgets#99")["written"] is True


def test_a_failing_gh_is_refused_with_its_error_and_a_working_one_is_admitted(board: Board) -> None:
    board.gh_answers("", code=4, stderr="To get started with GitHub CLI, please run:  gh auth login\n")
    failure = board.refusal("octo/widgets#7")
    assert failure["code"] == "github::gh::failed"
    assert "gh auth login" in failure["message"]
    board.gh_answers(issue(7))
    assert board.call("octo/widgets#7")["written"] is True


def test_gh_missing_from_path_is_refused_and_gh_on_path_is_admitted(board: Board) -> None:
    board.gh_answers(issue(7))
    assert board.refusal("octo/widgets#7", gh=False)["code"] == "github::gh::failed"
    assert board.call("octo/widgets#7")["written"] is True


def test_an_answer_without_a_page_url_is_refused_and_one_with_it_is_admitted(board: Board) -> None:
    answer = issue(7)
    board.gh_answers({key: value for key, value in answer.items() if key != "html_url"})
    failure = board.refusal("octo/widgets#7")
    assert failure["code"] == "github::gh::failed"
    assert "run `gh api repos/octo/widgets/issues/7`" in failure["message"]
    assert "gh auth status" not in failure["message"]
    board.gh_answers(answer)
    assert board.call("octo/widgets#7")["written"] is True


def test_a_degraded_record_already_there_is_refused_and_a_valid_one_is_reported(board: Board) -> None:
    record = {"$type": "github/issue/v1", "owner": "octo", "repo": "widgets", "number": 7}
    board.write("gh/octo/widgets/issues/7.json", json.dumps(record))
    failure = board.refusal("octo/widgets#7")
    assert failure["code"] == "github::record::degraded"
    assert board.gh_calls() == 0
    board.write(
        "gh/octo/widgets/issues/7.json", json.dumps(record | {"url": "https://github.com/octo/widgets/issues/7"})
    )
    assert board.call("octo/widgets#7")["written"] is False
