"""`review.post` asks the owner with the whole comment, posts it with gh once approved, and records where it went."""

import json
import os
import subprocess
from pathlib import Path
from typing import TYPE_CHECKING, Any, cast

from conftest import COMMENT_URL, ISOLATED, Repo

from board_review.post import comment

if TYPE_CHECKING:
    from board_review.shapes import ReportRecord

PR = "https://github.com/o/r/pull/7"
RANGE = "a" * 40 + ".." + "b" * 40
REPORT: dict[str, Any] = {
    "$type": "review/report/v1",
    "range": RANGE,
    "patch_ids": [],
    "findings": [
        {"severity": "high", "file": "src/sync.rs", "line": 3, "text": "Ask @alice about the lock."},
        {"severity": "low", "text": "The change has no test."},
    ],
    "verdict": "changes",
    "by": "claude-opus",
}
EXPECTED = f"""**Review: changes** by claude-opus

Commits `{RANGE}`.

- **high** `src/sync.rs:3` Ask @\u200dalice about the lock.
- **low** The change has no test.

---
Report /reviews/r1/report, for {PR}.
"""


def reported(repo: Repo, **fields: object) -> str:
    """Commit a report at /reviews/r1/report holding `REPORT` and `fields`, and return its path."""
    repo.commit("reviews/r1/report.json", json.dumps(REPORT | fields))
    return "/reviews/r1/report"


def post(repo: Repo, report: str = "/reviews/r1/report", pr: str = PR) -> tuple[int, dict[str, Any]]:
    return repo.call("post", "--report", report, "--pr", pr)


def approve(repo: Repo) -> None:
    """Make the `board` shim run the plugin's decide as the owner, whom Board admits."""
    (repo.bin / "approve").touch()


def pending(repo: Repo) -> list[Path]:
    return sorted((repo.personal / "effects" / "pending").glob("*.json"))


def gh_calls(repo: Repo) -> int:
    calls = repo.bin / "gh-calls.txt"
    return len(calls.read_text().splitlines()) if calls.exists() else 0


def test_a_first_post_asks_the_owner_with_the_whole_comment_and_an_approved_one_posts_that_comment(repo: Repo) -> None:
    reported(repo)
    code, asked = post(repo)
    assert code == 1
    assert asked["code"] == "review::board::refused"
    assert asked["fields"]["code"] == "board::settings::asked"
    effect = asked["fields"]["effect"]
    assert f"pending effect {effect}" in asked["message"]
    [effect_file] = pending(repo)
    assert effect_file.stem == effect
    assert json.loads(effect_file.read_text())["text"] == EXPECTED
    assert gh_calls(repo) == 0

    approve(repo)
    code, posted = post(repo)
    assert code == 0, posted
    assert posted == {"report": "/reviews/r1/report", "pull_request": PR, "comment": COMMENT_URL, "posted": True}
    assert (repo.bin / "gh-argv.txt").read_text().splitlines() == ["pr", "comment", PR, "--body-file", "-"]
    assert (repo.bin / "gh-stdin.txt").read_text() == EXPECTED
    environment = (repo.bin / "gh-env.txt").read_text().splitlines()
    assert "GH_PROMPT_DISABLED=1" in environment
    assert "GH_NO_UPDATE_NOTIFIER=1" in environment
    assert repo.get("/reviews/r1/report") == REPORT | {"source": COMMENT_URL}


def test_a_post_the_owner_refused_is_refused_and_one_not_yet_answered_is_asked(repo: Repo) -> None:
    reported(repo)
    _, asked = post(repo)
    assert asked["fields"]["code"] == "board::settings::asked"
    effect = asked["fields"]["effect"]
    seen = repo.personal / "effects" / "seen" / f"{effect}.json"
    seen.parent.mkdir(parents=True)
    seen.write_text(json.dumps({"$type": "board/seen/v1", "effect": effect, "answer": "refuse"}))
    # The owner's refusal needs no signature, so git writes it as `board answer` would.
    git = ["git", "-C", str(repo.personal), "-c", "user.name=t", "-c", "user.email=t@t"]
    for arguments in (["add", "."], ["commit", "-qm", "refuse"]):
        subprocess.run([*git, *arguments], env=os.environ | ISOLATED, check=True, capture_output=True)
    code, refused = post(repo)
    assert code == 1
    assert refused["code"] == "review::board::refused"
    assert refused["fields"]["code"] == "board::settings::refused"
    assert refused["fields"]["effect"] == effect
    assert gh_calls(repo) == 0


def test_a_post_whose_report_has_a_source_skips_and_one_without_a_source_posts(repo: Repo) -> None:
    reported(repo)
    approve(repo)
    assert post(repo)[1]["posted"] is True
    code, again = post(repo)
    assert code == 0, again
    assert again == {"report": "/reviews/r1/report", "pull_request": PR, "comment": COMMENT_URL, "posted": False}
    assert gh_calls(repo) == 1


def test_a_post_whose_gh_fails_records_no_source_and_a_rerun_posts(repo: Repo) -> None:
    reported(repo)
    approve(repo)
    (repo.bin / "gh-exit.txt").write_text("1")
    code, failed = post(repo)
    assert code == 1
    assert failed["code"] == "review::gh::failed"
    assert failed["fields"]["exit"] == 1
    assert "HTTP 502: Bad Gateway" in failed["message"]
    assert "source" not in repo.get("/reviews/r1/report")
    (repo.bin / "gh-exit.txt").write_text("0")
    code, posted = post(repo)
    assert code == 0, posted
    assert posted["posted"] is True
    assert gh_calls(repo) == 2  # noqa: PLR2004 - the failed call and the one that posted


def test_a_changed_comment_asks_again_under_a_new_effect_and_an_unchanged_one_asks_under_the_same(repo: Repo) -> None:
    reported(repo)
    first = post(repo)[1]["fields"]["effect"]
    assert post(repo)[1]["fields"]["effect"] == first
    assert len(pending(repo)) == 1
    reported(repo, verdict="comment")
    second = post(repo)[1]["fields"]["effect"]
    assert second != first
    assert len(pending(repo)) == 2  # noqa: PLR2004 - the first ask stays open beside the second


def test_a_post_to_another_pull_request_asks_again_under_a_new_effect(repo: Repo) -> None:
    reported(repo)
    first = post(repo)[1]["fields"]["effect"]
    other = post(repo, pr="https://github.com/o/r/pull/8")[1]["fields"]["effect"]
    assert other != first


def test_a_report_that_is_not_there_is_refused_with_boards_code_and_one_that_is_there_is_asked(repo: Repo) -> None:
    code, missing = post(repo)
    assert code == 1
    assert missing["code"] == "review::board::refused"
    assert missing["fields"]["code"] == "board::path::not_found"
    reported(repo)
    assert post(repo)[1]["fields"]["code"] == "board::settings::asked"


def test_a_pull_request_that_is_not_a_github_pull_request_url_is_refused_and_one_that_is_is_asked(repo: Repo) -> None:
    reported(repo)
    code, refused = post(repo, pr="7")
    assert code == 1
    assert refused["code"] == "board::plugin::bad_input"
    assert post(repo)[1]["fields"]["code"] == "board::settings::asked"


def test_a_file_holding_backticks_gets_a_longer_code_span_and_a_plain_file_one_backtick() -> None:
    finding = {"severity": "medium", "file": "a``b.rs", "text": "t"}
    report = cast("ReportRecord", REPORT | {"findings": [finding, {"severity": "low", "file": "c.rs", "text": "u"}]})
    lines = comment("/reviews/r1/report", PR, report).splitlines()
    assert "- **medium** ``` a``b.rs ``` t" in lines
    assert "- **low** `c.rs` u" in lines


def test_a_report_with_no_findings_says_so_and_names_the_report_it_follows() -> None:
    report = cast("ReportRecord", REPORT | {"findings": [], "verdict": "approve", "follows": "/reviews/r0/report"})
    text = comment("/reviews/r1/report", PR, report)
    assert "\nNo findings.\n" in text
    assert "\nFollows /reviews/r0/report.\n" in text
