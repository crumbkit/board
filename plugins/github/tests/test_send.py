"""`pr-create`, `pr-edit`, `comment` and `issue-create`: gh runs only after the boundary passes what it would send."""

import json
from typing import Any

from conftest import Desk, git

PULL_URL = "https://github.com/Octo/Widgets/pull/12"
ISSUE_URL = "https://github.com/Octo/Widgets/issues/7"


def output(document: dict[str, Any]) -> dict[str, Any]:
    """The capability's output in a `--json` report."""
    assert document["exit"] == 0, document
    result: dict[str, Any] = document["data"]["output"]
    return result


def refused(document: dict[str, Any]) -> dict[str, Any]:
    """The fields of a boundary refusal, once its codes are checked."""
    assert (document["exit"], document["code"]) == (1, "github::boundary::refused"), document
    fields: dict[str, Any] = document["fields"]
    assert fields["code"] == "boundary::check::failed"
    assert fields["recovery"]["text"].startswith("Next: ")
    assert fields["recovery"]["text"] in document["message"]
    return fields


def test_pr_create_pushes_opens_the_pull_request_and_writes_its_record(desk: Desk) -> None:
    git(desk.checkout, "switch", "-q", "-c", "feature")
    sha = desk.commit("Add a lever")
    desk.gh_answers(f"Creating pull request\n{PULL_URL}\n")
    result = output(desk.call("pr-create", "--title", "Add a lever", "--body", "Levers lift.", "--base", "main"))
    assert desk.remote_head("feature") == sha
    assert desk.gh_argv() == [
        "pr",
        "create",
        "--repo",
        "octo/widgets",
        "--head",
        "feature",
        "--title",
        "Add a lever",
        "--body-file",
        "-",
        "--base",
        "main",
    ]
    assert desk.gh_stdin() == "Levers lift."
    assert (result["sent"], result["number"], result["path"]) == (PULL_URL, 12, "/gh/octo/widgets/pulls/12")
    assert desk.record("/gh/octo/widgets/pulls/12") == {
        "$type": "github/pull/v1",
        "owner": "octo",
        "repo": "widgets",
        "number": 12,
        "url": PULL_URL,
        "title": "Add a lever",
    }
    crossing = desk.record(result["crossing"])
    assert crossing is not None
    assert (crossing["action"], crossing["result"]) == ("github.pr-create", PULL_URL)


def test_pr_create_with_a_private_body_pushes_nothing_and_runs_no_gh_and_a_clean_body_is_sent(desk: Desk) -> None:
    desk.refuse(["pr.*"])
    git(desk.checkout, "switch", "-q", "-c", "feature")
    desk.commit("Add a lever")
    fields = refused(desk.call("pr-create", "--title", "Add a lever", "--body", "For acme-123."))
    assert [failure["part"] for failure in fields["failures"]] == ["pr.body"]
    assert desk.remote_head("feature") is None
    assert desk.gh_calls() == 0
    desk.gh_answers(f"{PULL_URL}\n")
    assert output(desk.call("pr-create", "--title", "Add a lever", "--body", "Levers lift."))["sent"] == PULL_URL
    assert "--draft" not in desk.gh_argv()


def test_pr_edit_sends_only_the_fields_given_and_a_private_title_is_refused(desk: Desk) -> None:
    desk.refuse(["pr.title"])
    refused(desk.call("pr-edit", "--number", "12", "--title", "acme-123 levers"))
    assert desk.gh_calls() == 0
    desk.gh_answers({"html_url": PULL_URL, "number": 12})
    result = output(desk.call("pr-edit", "--number", "12", "--title", "Levers"))
    assert desk.gh_argv() == ["api", "--method", "PATCH", "repos/octo/widgets/pulls/12", "--input", "-"]
    assert json.loads(desk.gh_stdin()) == {"title": "Levers"}
    assert result["sent"] == PULL_URL


def test_pr_edit_without_a_title_or_body_is_refused_by_board_and_with_a_body_is_sent(desk: Desk) -> None:
    failure = desk.call("pr-edit", "--number", "12")
    assert (failure["exit"], failure["code"]) == (1, "board::plugin::bad_input")
    desk.gh_answers({"html_url": PULL_URL, "number": 12})
    output(desk.call("pr-edit", "--number", "12", "--body", "Levers lift."))
    assert json.loads(desk.gh_stdin()) == {"body": "Levers lift."}


def test_a_comment_is_checked_then_posted_and_a_private_one_is_not(desk: Desk) -> None:
    desk.refuse(["comment.body"])
    refused(desk.call("comment", "--number", "7", "--body", "Ask acme-123."))
    assert desk.gh_calls() == 0
    url = f"{ISSUE_URL}#issuecomment-1"
    desk.gh_answers({"html_url": url, "id": 1})
    result = output(desk.call("comment", "--number", "7", "--body", "Looks good."))
    assert desk.gh_argv() == ["api", "--method", "POST", "repos/octo/widgets/issues/7/comments", "--input", "-"]
    assert json.loads(desk.gh_stdin()) == {"body": "Looks good."}
    assert result["sent"] == url


def test_issue_create_opens_the_issue_and_writes_its_record_and_a_private_title_is_refused(desk: Desk) -> None:
    desk.refuse(["issue.*"])
    refused(desk.call("issue-create", "--title", "acme-123 wobbles", "--body", "It wobbles."))
    assert desk.gh_calls() == 0
    desk.gh_answers({"html_url": ISSUE_URL, "number": 7})
    result = output(desk.call("issue-create", "--title", "Widgets wobble", "--body", "It wobbles."))
    assert desk.gh_argv() == ["api", "--method", "POST", "repos/octo/widgets/issues", "--input", "-"]
    assert json.loads(desk.gh_stdin()) == {"title": "Widgets wobble", "body": "It wobbles."}
    assert (result["sent"], result["path"]) == (ISSUE_URL, "/gh/octo/widgets/issues/7")
    assert desk.record("/gh/octo/widgets/issues/7") == {
        "$type": "github/issue/v1",
        "owner": "octo",
        "repo": "widgets",
        "number": 7,
        "url": ISSUE_URL,
        "title": "Widgets wobble",
    }


def test_a_failing_gh_after_a_pass_is_reported_and_no_record_is_written(desk: Desk) -> None:
    desk.gh_answers("", code=1, stderr="HTTP 422: Validation Failed\n")
    failure = desk.call("issue-create", "--title", "Widgets wobble", "--body", "It wobbles.")
    assert (failure["exit"], failure["code"]) == (1, "github::gh::failed")
    assert "Validation Failed" in failure["message"]
    assert desk.record("/gh/octo/widgets/issues/7") is None
    desk.gh_answers({"html_url": ISSUE_URL, "number": 7})
    assert output(desk.call("issue-create", "--title", "Widgets wobble", "--body", "It wobbles."))["sent"] == ISSUE_URL
