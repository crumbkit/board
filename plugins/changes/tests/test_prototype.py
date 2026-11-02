"""`changes.prototype` and `changes.verdict`: a change that answers one question on its own branch and never lands."""

from conftest import TASK, Board, change, mount_repo, task

from board_changes.tasks import split_task

PATH = "/changes/step-list"
QUESTION = "Does a step list read better than a graph?"
PARENT = "/tasks/flows-editor.md"


def prototype(phase: str = "build", **fields: object) -> str:
    """A prototype change in `phase`, still exploring its question."""
    given: dict[str, object] = {"kind": "prototype", "question": QUESTION, "state": "exploring"}
    return change(phase, **(given | fields))


def test_prototype_adds_its_task_under_the_parent_and_writes_an_exploring_change_in_build(tasks_board: Board) -> None:
    mount = mount_repo(tasks_board, {"README.md": "x"})
    tasks_board.write(PARENT.removeprefix("/"), task())
    given = {"mount": mount, "question": QUESTION, "parent": PARENT, "title": "Step list"}
    output = tasks_board.call("prototype", given)
    assert (output["path"], output["task"], output["branch"]) == (PATH, "/tasks/step-list.md", "proto/step-list")
    assert output["brief"]["brief_line"] == f"board-brief: {PATH}"
    assert output["brief"]["phase"] == "build"
    record = tasks_board.read("changes/step-list.json")
    assert {key: record[key] for key in ("phase", "kind", "task", "question", "state")} == {
        "phase": "build",
        "kind": "prototype",
        "task": "/tasks/step-list.md",
        "question": QUESTION,
        "state": "exploring",
    }
    assert 'parent: "/tasks/flows-editor.md"' in (tasks_board.root / "tasks/step-list.md").read_text()
    plan = (tasks_board.root / record["plan"].removeprefix("/")).read_text()
    assert plan == f"# Step list\n\n## Question\n\n{QUESTION}\n"


def test_prototype_refuses_a_name_a_change_already_has_and_admits_another(tasks_board: Board) -> None:
    mount = mount_repo(tasks_board, {"README.md": "x"})
    tasks_board.write(PARENT.removeprefix("/"), task())
    tasks_board.write("changes/step-list.json", prototype())
    given = {"mount": mount, "question": QUESTION, "parent": PARENT, "title": "Step list"}
    assert tasks_board.refusal("prototype", given)["code"] == "changes::change::exists"
    assert tasks_board.call("prototype", given | {"name": "step-list-2"})["path"] == "/changes/step-list-2"


def test_a_prototype_refuses_to_advance_and_a_build_change_in_build_is_held_by_its_own_gate(board: Board) -> None:
    board.write("changes/step-list.json", prototype())
    refused = board.refusal("advance", {"path": PATH})
    assert (refused["code"], refused["fields"]["missing"]) == ("changes::prototype::never_lands", ["verdict"])
    board.write("changes/step-list.json", change("build"))
    assert board.refusal("advance", {"path": PATH})["code"] == "changes::gate::unmet"


def test_next_tells_a_prototypes_builder_to_iterate_on_its_branch_and_ask_for_the_verdict(board: Board) -> None:
    board.write("changes/step-list.json", prototype())
    output = board.call("next", {"path": PATH})
    assert "on branch proto/step-list" in output["instruction"]
    assert "changes.verdict" in output["instruction"]


def test_a_settled_verdict_ends_the_prototype_in_done_with_the_artifact_it_means(tasks_board: Board) -> None:
    tasks_board.write("changes/step-list.json", prototype())
    artifact = "https://claude.ai/artifact/abc"
    given = {"path": PATH, "state": "settled", "verdict": "A step list reads better.", "artifact": artifact}
    assert tasks_board.call("verdict", given) == {"path": PATH, "state": "settled", "phase": "done"}
    record = tasks_board.read("changes/step-list.json")
    assert (record["state"], record["verdict"], record["verdict_artifact"]) == (
        "settled",
        "A step list reads better.",
        artifact,
    )
    assert 'status: "done"' in (tasks_board.root / "tasks/given.md").read_text()


def test_an_abandoned_verdict_ends_the_prototype_in_dropped(tasks_board: Board) -> None:
    tasks_board.write("changes/step-list.json", prototype())
    given = {"path": PATH, "state": "abandoned", "verdict": "The owner lost interest."}
    assert tasks_board.call("verdict", given)["phase"] == "dropped"
    assert "verdict_artifact" not in tasks_board.read("changes/step-list.json")
    assert 'status: "dropped"' in (tasks_board.root / "tasks/given.md").read_text()


def test_verdict_refuses_a_build_change_and_admits_a_prototype(tasks_board: Board) -> None:
    given = {"path": PATH, "state": "settled", "verdict": "Yes."}
    tasks_board.write("changes/step-list.json", change("build"))
    assert tasks_board.refusal("verdict", given)["code"] == "changes::prototype::not_prototype"
    tasks_board.write("changes/step-list.json", prototype())
    assert tasks_board.call("verdict", given)["phase"] == "done"


def test_verdict_refuses_a_prototype_already_ended_and_admits_one_exploring(tasks_board: Board) -> None:
    given = {"path": PATH, "state": "settled", "verdict": "Yes."}
    tasks_board.write("changes/step-list.json", prototype("done", state="settled", verdict="Yes."))
    assert tasks_board.refusal("verdict", given)["code"] == "changes::phase::final"
    tasks_board.write("changes/step-list.json", prototype())
    assert tasks_board.call("verdict", given)["phase"] == "done"


def test_open_by_kind_prototype_lists_every_prototype_with_its_question_verdict_and_branch(board: Board) -> None:
    board.write("changes/a.json", change("build"))
    board.write("changes/b.json", prototype())
    board.write("changes/c.json", prototype("done", state="settled", verdict="Yes."))
    output = board.call("open", {"kind": "prototype"})
    assert [(entry["path"], entry["kind"]) for entry in output["changes"]] == [
        ("/changes/b", "prototype"),
        ("/changes/c", "prototype"),
    ]
    assert output["changes"][1]["prototype"] == {
        "question": QUESTION,
        "state": "settled",
        "verdict": "Yes.",
        "verdict_artifact": None,
        "branch": "proto/c",
    }
    assert [entry["path"] for entry in board.call("open", {"kind": "build"})["changes"]] == ["/changes/a"]
    assert [entry["path"] for entry in board.call("open", {})["changes"]] == ["/changes/a", "/changes/b"]


def test_a_task_path_splits_into_its_notes_repo_and_its_path_there() -> None:
    assert split_task("/repos/notes/tasks/a.md") == ("/repos/notes", "/tasks/a.md")
    assert split_task(TASK) == ("/", TASK)


def test_the_schema_refuses_an_unknown_kind_or_state_and_admits_a_prototype(board: Board) -> None:
    board.write("repos/x/README.md", "x")
    board.write("changes/kind.json", prototype(kind="spike"))
    board.write("changes/state.json", prototype(state="paused"))
    board.write("changes/good.json", prototype())
    report = board.json("check", "--all")
    codes = {
        entry["logical"]: entry.get("code") for entry in report["data"] if entry["logical"].startswith("/changes/")
    }
    assert codes == {
        "/changes/kind": "board::record::invalid",
        "/changes/state": "board::record::invalid",
        "/changes/good": None,
    }


def test_prototype_refuses_when_three_prototypes_explore_and_starts_once_one_is_settled(tasks_board: Board) -> None:
    mount = mount_repo(tasks_board, {"README.md": "x"})
    tasks_board.write(PARENT.removeprefix("/"), task())
    for number in range(3):
        tasks_board.write(f"changes/p{number}.json", prototype())
    given = {"mount": mount, "question": QUESTION, "parent": PARENT, "title": "Step list"}
    assert tasks_board.refusal("prototype", given)["code"] == "changes::wip::full"
    tasks_board.call("verdict", {"path": "/changes/p0", "state": "settled", "verdict": "Yes."})
    assert tasks_board.call("prototype", given)["path"] == PATH


def test_a_verdict_leaves_a_task_already_closed_as_it_is(tasks_board: Board) -> None:
    tasks_board.write("tasks/given.md", task("done"))
    tasks_board.write("changes/step-list.json", prototype())
    given = {"path": PATH, "state": "abandoned", "verdict": "Not needed."}
    assert tasks_board.call("verdict", given)["phase"] == "dropped"
    assert "status: done" in (tasks_board.root / "tasks/given.md").read_text()


def test_prototype_refuses_a_parent_outside_a_tasks_directory_and_admits_one_inside(tasks_board: Board) -> None:
    mount = mount_repo(tasks_board, {"README.md": "x"})
    tasks_board.write("projects/flows.md", task())
    tasks_board.write(PARENT.removeprefix("/"), task())
    given = {"mount": mount, "question": QUESTION, "title": "Step list"}
    refused = tasks_board.refusal("prototype", given | {"parent": "/projects/flows.md"})
    assert refused["code"] == "changes::task::not_a_task_path"
    assert not (tasks_board.root / "changes/step-list.json").exists()
    assert tasks_board.call("prototype", given | {"parent": PARENT})["path"] == PATH


def test_build_changes_in_build_do_not_fill_the_prototype_cap(tasks_board: Board) -> None:
    mount = mount_repo(tasks_board, {"README.md": "x"})
    tasks_board.write(PARENT.removeprefix("/"), task())
    for number in range(3):
        tasks_board.write(f"changes/b{number}.json", change("build"))
    given = {"mount": mount, "question": QUESTION, "parent": PARENT, "title": "Step list"}
    assert tasks_board.call("prototype", given)["path"] == PATH


def test_a_paused_prototype_resumes_only_while_fewer_than_three_others_explore(board: Board) -> None:
    board.write("changes/step-list.json", prototype())
    assert board.call("pause", {"path": PATH})["to"] == "paused"
    for number in range(3):
        board.write(f"changes/p{number}.json", prototype())
    refused = board.refusal("resume", {"path": PATH})
    assert (refused["code"], refused["fields"]["kind"]) == ("changes::wip::full", "prototype")
    (board.root / "changes/p0.json").unlink()
    assert board.call("resume", {"path": PATH})["to"] == "build"


def test_a_verdict_ends_a_paused_prototype(tasks_board: Board) -> None:
    tasks_board.write("changes/step-list.json", prototype("paused"))
    given = {"path": PATH, "state": "abandoned", "verdict": "The owner lost interest."}
    assert tasks_board.call("verdict", given)["phase"] == "dropped"
