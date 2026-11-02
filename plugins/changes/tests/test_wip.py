"""The cap on changes in build and prototypes exploring, and the paused phase that holds a change outside it."""

from typing import TYPE_CHECKING, Any

from conftest import PLAN, Board, change

from board_changes.wip import WIP_LIMIT, building

if TYPE_CHECKING:
    from board_changes.change import Change

PATH = "/changes/t"


def held(phase: str, **fields: Any) -> "Change":  # noqa: ANN401 - the change's other fields
    """A change in `phase` as the cap reads it."""
    return {"$type": "changes/change/v1", "title": "T", "mount": "/repos/x", "phase": phase, **fields}  # type: ignore[typeddict-item]


def test_the_cap_counts_build_changes_in_build_apart_from_prototypes_exploring() -> None:
    changes = [
        held("build"),
        held("build", kind="build"),
        held("paused"),
        held("review"),
        held("build", kind="prototype", state="exploring"),
        held("done", kind="prototype", state="settled"),
    ]
    assert building(changes, "build") == changes[:2]
    assert building(changes, "prototype") == [changes[4]]


def fill(board: Board, count: int, **fields: Any) -> None:  # noqa: ANN401 - the changes' other fields
    """`count` other changes in build."""
    for number in range(count):
        board.write(f"changes/busy-{number}.json", change("build", **fields))


def ready(board: Board) -> None:
    """A change in plan whose gate passes."""
    board.write("changes/t.json", change(plan="/changes/t/plan.md", route="owner"))
    board.write("changes/t/plan.md", PLAN)


def test_advance_into_build_refuses_when_the_cap_is_full_and_moves_on_below_it(board: Board) -> None:
    ready(board)
    fill(board, WIP_LIMIT)
    refused = board.refusal("advance", {"path": PATH})
    assert refused["code"] == "changes::wip::full"
    assert refused["fields"]["building"] == [f"/changes/busy-{number}" for number in range(WIP_LIMIT)]
    board.call("pause", {"path": "/changes/busy-0"})
    assert board.call("advance", {"path": PATH})["to"] == "build"


def test_prototypes_exploring_do_not_fill_the_build_cap(board: Board) -> None:
    ready(board)
    fill(board, WIP_LIMIT, kind="prototype", state="exploring")
    assert board.call("advance", {"path": PATH})["to"] == "build"


def test_pause_moves_a_change_in_build_to_paused_and_refuses_one_in_another_phase(board: Board) -> None:
    board.write("changes/t.json", change("review"))
    assert board.refusal("pause", {"path": PATH})["code"] == "changes::pause::not_building"
    board.write("changes/t.json", change("build"))
    assert board.call("pause", {"path": PATH}) == {"path": PATH, "from": "build", "to": "paused"}
    assert board.call("open", {})["changes"][0]["phase"] == "paused"


def test_a_paused_change_does_not_advance_and_says_to_resume(board: Board) -> None:
    board.write("changes/t.json", change("paused"))
    assert board.refusal("advance", {"path": PATH})["code"] == "changes::phase::paused"
    assert "changes.resume" in board.call("next", {"path": PATH})["instruction"]
    board.write("changes/t.json", change("build"))
    assert board.refusal("advance", {"path": PATH})["code"] == "changes::gate::unmet"


def test_resume_refuses_when_the_cap_is_full_and_moves_the_change_back_to_build_below_it(board: Board) -> None:
    board.write("changes/t.json", change("paused"))
    fill(board, WIP_LIMIT)
    assert board.refusal("resume", {"path": PATH})["code"] == "changes::wip::full"
    (board.root / "changes/busy-0.json").unlink()
    assert board.call("resume", {"path": PATH}) == {"path": PATH, "from": "paused", "to": "build"}


def test_resume_refuses_a_change_that_is_not_paused_and_admits_one_that_is(board: Board) -> None:
    board.write("changes/t.json", change("build"))
    assert board.refusal("resume", {"path": PATH})["code"] == "changes::resume::not_paused"
    board.write("changes/t.json", change("paused"))
    assert board.call("resume", {"path": PATH})["to"] == "build"
