"""Control scope: `repos`, named files, the caller's repository, `--limit` and a repository's own controls."""

import json
import subprocess
from pathlib import Path
from typing import Any

from conftest import Board, commit, control, repo

SECRETS = "src/secrets/key.rs"


def listed(report: dict[str, Any]) -> tuple[list[tuple[str, str]], list[tuple[str, str]]]:
    """The measured and the stated controls of a `check` report, each by its path and its board."""
    assert report["schema"] == "board/call-report/v2", report
    output = report["data"]["output"]
    return (
        [(item["control"], item["board"]) for item in output["measured"]],
        [(item["control"], item["board"]) for item in output["stated"]],
    )


def stated(report: dict[str, Any]) -> list[str]:
    """The stated controls of a `check` report, by path."""
    return [control for control, _ in listed(report)[1]]


def repositories(board: Board, tmp_path: Path) -> None:
    """Three mounted repositories and the board's own tree, each holding the same secrets file.

    `/controls/secrets` covers it in two of the repositories by `repos`, and `/controls/root` names no
    repository, so its pattern matches from the board's root. Each repository's file differs, since Board
    indexes one commit once and would show three identical ones through one mount.
    """
    for name in ["app", "web", "other"]:
        board.write(f"repos/{name}.json", repo(f"owner/{name}", tmp_path / name, {SECRETS: name}))
    board.write(SECRETS, "key")
    board.write(
        "controls/secrets.json", control("Review.", repos=["/repos/app", "/repos/web"], paths=["src/secrets/*"])
    )
    board.write("controls/root.json", control("Review.", paths=["src/secrets/*"]))


def test_a_control_with_repos_matches_a_file_in_each_listed_repository_and_not_the_same_path_in_another(
    board: Board, tmp_path: Path
) -> None:
    repositories(board, tmp_path)
    for path in ["/repos/app", "/repos/web"]:
        report = board.call("check", {"path": path})
        assert stated(report) == ["/controls/secrets"], report
        assert report["data"]["output"]["stated"][0]["files"] == 1
    assert stated(board.call("check", {"path": "/repos/other"})) == []


def test_a_control_without_repos_matches_from_the_boards_root_and_one_with_repos_does_not(
    board: Board, tmp_path: Path
) -> None:
    repositories(board, tmp_path)
    assert stated(board.call("check", {"path": "/src"})) == ["/controls/root"]


def test_for_path_matches_a_control_with_repos_in_a_listed_repository_and_not_from_the_boards_root(
    board: Board, tmp_path: Path
) -> None:
    repositories(board, tmp_path)

    def applying(path: str) -> list[str]:
        report = board.call("for-path", {"path": path})
        assert report["schema"] == "board/call-report/v2", report
        return [item["control"] for item in report["data"]["output"]["controls"]]

    assert applying(f"/repos/app/{SECRETS}") == ["/controls/secrets"]
    assert applying(f"/repos/other/{SECRETS}") == []
    assert applying(f"/{SECRETS}") == ["/controls/root"]


def test_repos_naming_nothing_is_degraded_and_repos_naming_a_repo_record_is_not(board: Board) -> None:
    board.write("repos/app.json", repo("owner/app"))
    board.write("controls/gone.json", control("R.", repos=["/repos/gone"]))
    board.write("controls/right.json", control("R.", repos=["/repos/app"]))
    status = {entry["logical"]: entry for entry in json.loads(board.run("--json", "check").stdout)["data"]}
    assert status["/controls/gone"]["code"] == "board::ref::dangling"
    assert status["/controls/right"]["status"] == "ok"


def unmounted(board: Board) -> None:
    """A repo record without a checkout, and a control on its secrets."""
    board.write("repos/app.json", repo("owner/app"))
    board.write("controls/secrets.json", control("Review.", repos=["/repos/app"], paths=["src/secrets/*"]))


def test_named_files_match_an_unmounted_repositorys_files_and_files_the_control_does_not_cover_match_nothing(
    board: Board,
) -> None:
    unmounted(board)
    report = board.call_json("check", {"path": "/repos/app", "files": [SECRETS, "README.md"]})
    assert stated(report) == ["/controls/secrets"], report
    assert report["data"]["output"]["stated"][0]["files"] == 1
    assert stated(board.call_json("check", {"path": "/repos/app", "files": ["README.md"]})) == []


def test_a_named_file_with_a_leading_slash_is_refused_and_the_same_file_without_it_is_admitted(board: Board) -> None:
    unmounted(board)
    refused = board.call_json("check", {"path": "/repos/app", "files": [f"/{SECRETS}"]})
    assert refused["code"] == "board::plugin::bad_input", refused
    assert stated(board.call_json("check", {"path": "/repos/app", "files": [SECRETS]})) == ["/controls/secrets"]


def test_a_repo_record_without_a_checkout_reads_no_own_controls_and_one_with_a_checkout_does(
    board: Board, tmp_path: Path
) -> None:
    unmounted(board)
    report = board.call("check", {"path": "/repos/app"})
    assert report["data"]["output"]["own_controls"] == "not_read", report
    assert "checkout" in report["data"]["output"]["next"]
    board.write("repos/app.json", repo("owner/app", tmp_path / "app", {SECRETS: "key"}))
    output = board.call("check", {"path": "/repos/app"})["data"]["output"]
    assert (output["own_controls"], "next" in output) == ("read", False), output


def test_without_a_path_a_check_from_a_mounted_clone_checks_its_repo_records_path_and_one_from_the_board_checks_root(
    board: Board, tmp_path: Path
) -> None:
    repositories(board, tmp_path)
    output = board.call_json("check", {}, cwd=tmp_path / "app" / "src")["data"]["output"]
    assert (output["path"], [item["control"] for item in output["stated"]]) == ("/repos/app", ["/controls/secrets"])
    assert board.call_json("check", {})["data"]["output"]["path"] == "/"


def test_without_a_path_a_check_from_a_linked_worktree_of_a_mounted_clone_checks_its_repo_records_path(
    board: Board, tmp_path: Path
) -> None:
    repositories(board, tmp_path)
    linked = tmp_path / "app-linked"
    subprocess.run(["git", "worktree", "add", "-q", "-b", "work", str(linked)], cwd=tmp_path / "app", check=True)
    assert board.call_json("check", {}, cwd=linked)["data"]["output"]["path"] == "/repos/app"


def test_without_a_path_a_clone_of_an_unmounted_repository_checks_its_record_and_a_clone_without_one_checks_nothing(
    board: Board, tmp_path: Path
) -> None:
    unmounted(board)
    commit(tmp_path / "app", {}, "owner/app")
    commit(tmp_path / "stray", {}, "owner/stray")
    found = board.call_json("check", {"files": [SECRETS]}, cwd=tmp_path / "app")["data"]["output"]
    assert (found["path"], [item["control"] for item in found["stated"]]) == ("/repos/app", ["/controls/secrets"])
    nothing = board.call_json("check", {}, cwd=tmp_path / "stray")["data"]["output"]
    assert (nothing["path"], nothing["stated"], nothing["measured"]) == (None, [], []), nothing
    assert "owner/stray" in nothing["next"]
    assert "no mount or repo record" in nothing["next"]


LIMIT = 2


def test_limit_cuts_each_list_and_reports_its_total_and_a_limit_that_holds_them_cuts_nothing(board: Board) -> None:
    board.write("docs/a.md", "one")
    for name in ["a", "b", "c"]:
        board.write(f"controls/{name}.json", control("Plain.", paths=["docs/*"]))
    for name in ["m", "n"]:
        board.write(f"controls/{name}.json", control("Short.", paths=["docs/*"], rule={"budget": "words", "max": 9}))
    cut = board.call("check", {"limit": LIMIT})["data"]["output"]
    assert ([item["control"] for item in cut["stated"]], len(cut["measured"])) == (["/controls/a", "/controls/b"], 2)
    assert (cut["total"], cut["truncated"]) == ({"measured": 2, "stated": 3}, {"measured": False, "stated": True})
    whole = board.call("check", {"limit": LIMIT + 1})["data"]["output"]
    assert (len(whole["stated"]), whole["truncated"]) == (LIMIT + 1, {"measured": False, "stated": False})


def own(board: Board, tmp_path: Path) -> None:
    """A mounted repository that is itself a board, with its own controls, and a control on it in the board."""
    lib = tmp_path / "lib"
    lib.mkdir()
    (lib / ".board").mkdir()
    (lib / ".board" / "board.json").write_text(json.dumps({"$type": "board/config/v1", "commit_on_put": False}))
    subprocess.run(["git", "init", "-q", "-b", "main"], cwd=lib, check=True)
    enabled = board.run("plugin", "enable", "controls", cwd=lib)
    assert enabled.returncode == 0, enabled.stderr
    rule = {"budget": "words", "max": 5}
    files = {
        "AGENTS.md": "one two three",
        "src/main.rs": "fn main() {}",
        "controls/style.json": control("Plain code.", paths=["src/*"]),
        "controls/context.json": control("Short context.", applies_to=["/AGENTS.md"], rule=rule),
    }
    board.write("repos/lib.json", repo("owner/lib", lib, files))
    board.write("controls/lib.json", control("Reviewed.", repos=["/repos/lib"], paths=["src/*"]))


def test_a_repositorys_own_controls_appear_beside_the_boards_with_their_board_and_a_check_of_root_leaves_them_out(
    board: Board, tmp_path: Path
) -> None:
    own(board, tmp_path)
    report = board.call("check", {"path": "/repos/lib"})
    assert listed(report) == (
        [("/repos/lib/controls/context", "/repos/lib")],
        [("/controls/lib", "/"), ("/repos/lib/controls/style", "/repos/lib")],
    )
    output = report["data"]["output"]
    assert (output["own_controls"], output["measured"][0]["files"]) == ("read", ["/repos/lib/AGENTS.md"])
    assert output["measured"][0]["used"] == len(["one", "two", "three"])
    root = board.call("check", {})
    assert listed(root) == ([], [("/controls/lib", "/")])
    assert "own_controls" not in root["data"]["output"]


def test_a_repositorys_own_controls_match_named_files_beneath_the_mount_and_not_files_they_do_not_cover(
    board: Board, tmp_path: Path
) -> None:
    own(board, tmp_path)
    report = board.call_json("check", {"path": "/repos/lib", "files": ["src/new.rs"]})
    assert listed(report)[1] == [("/controls/lib", "/"), ("/repos/lib/controls/style", "/repos/lib")], report
    assert listed(board.call_json("check", {"path": "/repos/lib", "files": ["README.md"]}))[1] == []


def decision(supersedes: list[str] | None = None) -> str:
    """A `decisions/decision/v1` record."""
    return json.dumps(
        {
            "$type": "decisions/decision/v1",
            "decision": "Review secrets.",
            "why": "A leak costs most.",
            "decided": "2026-10-31T12:00:00Z",
            "session": "local_test",
            "supersedes": supersedes or [],
        }
    )


def test_a_control_resting_on_a_superseded_decision_is_marked_and_one_resting_on_its_successor_is_not(
    board: Board,
) -> None:
    assert board.run("plugin", "enable", "decisions").returncode == 0
    board.write("projects/app/decisions/old.json", decision())
    board.write("projects/app/decisions/new.json", decision(["/projects/app/decisions/old"]))
    board.write("controls/old.json", control("Old rule.", basis=["/projects/app/decisions/old"]))
    board.write("controls/new.json", control("New rule.", basis=["/projects/app/decisions/new"]))
    output = board.call("check", {})["data"]["output"]
    marked = {item["control"]: item.get("basis_superseded") for item in output["stated"]}
    assert marked == {"/controls/new": None, "/controls/old": ["/projects/app/decisions/old"]}, output


def test_check_shows_the_step_a_control_adds_and_none_for_one_that_adds_no_step(board: Board) -> None:
    board.write("controls/step.json", control("Review.", summary="Secrets: adds a review.", adds_step="review"))
    board.write("controls/plain.json", control("Plain."))
    output = board.call("check", {})["data"]["output"]
    assert {item["control"]: item["adds_step"] for item in output["stated"]} == {
        "/controls/plain": None,
        "/controls/step": "review",
    }


SUMMARY = 120


def test_a_summary_over_120_characters_is_invalid_and_one_of_120_is_not(board: Board) -> None:
    board.write("controls/long.json", control("R.", summary="x" * (SUMMARY + 1)))
    board.write("controls/fits.json", control("R.", summary="x" * SUMMARY))
    status = {entry["logical"]: entry for entry in json.loads(board.run("--json", "check").stdout)["data"]}
    assert status["/controls/long"]["code"] == "board::record::invalid"
    assert status["/controls/fits"]["status"] == "ok"
