"""`changes.plan`: a change's plan, the records it uses, and the limits on both."""

from conftest import PLAN, Board, change, record

from board_changes.plan import PLAN_WORDS, USES_WORDS

PATH = "/changes/t"


def words(count: int) -> str:
    """A Markdown document of `count` words under one heading word."""
    return "#" + " w" * count + "\n"


def plans(board: Board) -> list[str]:
    """The files under the change's plans directory, by name."""
    return sorted(path.name for path in (board.root / "changes/t/plans").iterdir())


def test_plan_writes_the_plan_and_its_text_and_points_the_change_at_the_text(board: Board) -> None:
    board.write("changes/t.json", change())
    board.write("standards/errors.md", "# Errors\n")
    output = board.given("plan", {"path": PATH, "text": PLAN, "uses": ["/standards/errors.md"]})
    stamp = output["path"].removeprefix("/changes/t/plans/")
    assert output == {"path": f"/changes/t/plans/{stamp}", "text": f"/changes/t/plans/{stamp}.md", "uses": 1}
    assert board.read(f"changes/t/plans/{stamp}.json") == {
        "$type": "changes/plan/v1",
        "text": f"/changes/t/plans/{stamp}.md",
        "uses": ["/standards/errors.md"],
    }
    assert (board.root / f"changes/t/plans/{stamp}.md").read_text() == PLAN
    assert board.read("changes/t.json")["plan"] == f"/changes/t/plans/{stamp}.md"


def test_a_second_plan_is_written_beside_the_first_and_becomes_the_change_s_plan(board: Board) -> None:
    board.write("changes/t.json", change())
    first = board.given("plan", {"path": PATH, "text": PLAN, "uses": []})
    second = board.given("plan", {"path": PATH, "text": PLAN + "More.\n", "uses": []})
    assert first["path"] < second["path"]
    assert len(plans(board)) == 2 * 2  # two plans, each a record and its text
    assert board.read("changes/t.json")["plan"] == second["text"]


def test_a_plan_that_uses_a_path_holding_nothing_is_refused_and_writes_nothing(board: Board) -> None:
    board.write("changes/t.json", change())
    board.write("standards/errors.md", "# Errors\n")
    refused = board.refused("plan", {"path": PATH, "text": PLAN, "uses": ["/standards/errors.md", "/standards/none"]})
    assert refused["code"] == "changes::ref::dangling"
    assert refused["fields"]["refs"] == ["/standards/none"]
    assert not (board.root / "changes/t/plans").exists()
    assert board.given("plan", {"path": PATH, "text": PLAN, "uses": ["/standards/errors.md"]})["uses"] == 1


def test_a_plan_over_3000_words_is_refused_and_one_of_3000_is_written(board: Board) -> None:
    board.write("changes/t.json", change())
    refused = board.refused("plan", {"path": PATH, "text": words(PLAN_WORDS), "uses": []})
    assert refused["code"] == "changes::plan::too_long"
    assert refused["fields"]["words"] == PLAN_WORDS + 1
    assert board.given("plan", {"path": PATH, "text": words(PLAN_WORDS - 1), "uses": []})["uses"] == 0


def test_documents_used_over_3000_words_in_all_are_refused_and_3000_are_admitted(board: Board) -> None:
    board.write("changes/t.json", change())
    board.write("standards/a.md", words(1499))
    board.write("standards/b.md", words(1500))
    board.write("decisions/d.json", record("plans/decision/v1", decision=" w" * 4000))
    uses = ["/standards/a.md", "/standards/b.md", "/decisions/d"]
    refused = board.refused("plan", {"path": PATH, "text": PLAN, "uses": uses})
    assert refused["code"] == "changes::plan::uses_too_long"
    assert refused["fields"]["words"] == USES_WORDS + 1
    board.write("standards/b.md", words(1499))
    assert board.given("plan", {"path": PATH, "text": PLAN, "uses": uses})["uses"] == len(uses)


def test_a_plan_for_a_done_change_is_refused_and_one_for_a_change_in_build_is_written(board: Board) -> None:
    board.write("changes/t.json", change("done"))
    assert board.refused("plan", {"path": PATH, "text": PLAN, "uses": []})["code"] == "changes::phase::final"
    board.write("changes/t.json", change("build"))
    assert board.given("plan", {"path": PATH, "text": PLAN, "uses": []})["uses"] == 0
