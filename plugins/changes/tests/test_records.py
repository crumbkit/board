"""`claim`, `files`, `check`, `landed` and `drop` each write one record, and an ended change takes none."""

from pathlib import Path

from conftest import HEAD, PATH, Board, clone, record, run, setup


def test_a_claim_from_a_subdirectory_records_its_worktrees_root(board: Board) -> None:
    setup(board)
    cloned = clone(board)
    (cloned.path / "src" / "deep").mkdir(parents=True)
    output = board.call("claim", {"path": PATH, "session": "local_2"}, cwd=cloned.path / "src" / "deep")
    assert output["worktree"] == str(cloned.path.resolve())
    claim = board.read(output["path"].removeprefix("/") + ".json")
    assert claim == {"$type": "changes/claim/v1", "session": "local_2", "worktree": str(cloned.path.resolve())}


def test_a_claim_outside_a_git_clone_is_refused_and_one_inside_is_written(board: Board, tmp_path: Path) -> None:
    setup(board)
    outside = tmp_path / "outside"
    outside.mkdir()
    assert board.refusal("claim", {"path": PATH, "session": "s"}, cwd=outside)["code"] == "changes::clone::none"
    assert board.call("claim", {"path": PATH, "session": "s"}, cwd=clone(board).path)["path"].startswith(PATH)


def test_files_records_the_paths_at_the_head_once_each(board: Board) -> None:
    setup(board)
    given = {"path": PATH, "head": HEAD, "files": ["src/a.rs", "README.md", "src/a.rs"]}
    assert board.call("files", given, stdin=True) == {"path": f"{PATH}/files/{HEAD}"}
    assert board.read(f"changes/t/files/{HEAD}.json") == {
        "$type": "changes/files/v1",
        "files": ["src/a.rs", "README.md"],
    }


def test_a_check_names_the_head_its_run_records_under_the_repo_records_name(board: Board) -> None:
    setup(board)
    board.write("suites/checks/runs/1.json", run())
    output = board.call("check", {"path": PATH, "run": "/suites/checks/runs/1"})
    assert output["head"] == HEAD
    assert board.read(output["path"].removeprefix("/") + ".json") == {
        "$type": "changes/check/v1",
        "run": "/suites/checks/runs/1",
    }


def test_a_check_of_something_that_is_not_a_run_is_refused_and_a_run_is_admitted(board: Board) -> None:
    setup(board)
    board.write("suites/checks/runs/1.json", record("runs/suite/v1", subjects=["app"], method="Run it."))
    refused = board.refusal("check", {"path": PATH, "run": "/suites/checks/runs/1"})
    assert (refused["code"], refused["fields"]["run"]) == ("changes::run::not_found", "/suites/checks/runs/1")
    board.write("suites/checks/runs/1.json", run())
    assert board.call("check", {"path": PATH, "run": "/suites/checks/runs/1"})["head"] == HEAD


def test_a_check_of_a_run_naming_no_commit_for_the_repository_is_refused_and_one_naming_it_is_admitted(
    board: Board,
) -> None:
    setup(board)
    board.write("suites/checks/runs/1.json", run(versions={"other": HEAD}))
    refused = board.refusal("check", {"path": PATH, "run": "/suites/checks/runs/1"})
    assert (refused["code"], refused["fields"]["name"]) == ("changes::run::no_commit", "app")
    board.write("suites/checks/runs/1.json", run())
    assert board.call("check", {"path": PATH, "run": "/suites/checks/runs/1"})["head"] == HEAD


def test_landed_ends_the_change_so_it_takes_no_further_record(board: Board) -> None:
    setup(board)
    assert board.call("landed", {"path": PATH, "commit": HEAD}) == {"path": f"{PATH}/landed"}
    assert board.read("changes/t/landed.json") == {"$type": "changes/landed/v1", "commit": HEAD}
    refused = board.refusal("drop", {"path": PATH, "why": "Too late."})
    assert (refused["code"], refused["fields"]["ended"]) == ("changes::change::ended", "landed")
    assert board.refusal("files", {"path": PATH, "head": HEAD, "files": []}, stdin=True)["code"] == (
        "changes::change::ended"
    )


def test_drop_records_why_and_a_dropped_change_takes_no_landed_record(board: Board) -> None:
    setup(board)
    board.call("drop", {"path": PATH, "why": "Not needed."})
    assert board.read("changes/t/dropped.json") == {"$type": "changes/dropped/v1", "why": "Not needed."}
    refused = board.refusal("landed", {"path": PATH, "commit": HEAD})
    assert (refused["code"], refused["fields"]["ended"]) == ("changes::change::ended", "dropped")


def test_a_path_that_holds_no_change_is_refused_and_one_that_does_is_admitted(board: Board) -> None:
    setup(board)
    assert board.refusal("drop", {"path": "/changes/none", "why": "x"})["code"] == "changes::change::not_found"
    assert board.call("drop", {"path": PATH, "why": "x"})["path"] == f"{PATH}/dropped"
