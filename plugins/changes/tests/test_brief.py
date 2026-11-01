"""`changes.brief`: what an agent needs to start, from the change's newest plan."""

from conftest import PLAN, Board, record, setup

from board_changes.plan import BRIEF_WORDS, CONTROLS

PATH = "/changes/t"
CONTROL = "controls/control/v1"


def test_brief_gives_one_line_for_each_record_the_newest_plan_uses_with_a_controls_summary(board: Board) -> None:
    setup(board)
    board.write("controls/a.json", record(CONTROL, requirement="Refusals carry a code.", purpose="p"))
    board.write("controls/b.json", record(CONTROL, summary="Stay small.", requirement="Long text.", purpose="p"))
    board.write("decisions/d.json", record("plans/decision/v1", decision="Plans are records."))
    board.write("evals/e.json", record("evals/eval/v1", question="Do trees help?", verdict="supported"))
    board.write("standards/errors.md", "Intro.\n\n## Refusal codes\n\nText.\n")
    board.call("plan", {"path": PATH, "text": PLAN, "uses": []}, stdin=True)
    uses = ["/controls/a", "/controls/b", "/decisions/d", "/evals/e", "/standards/errors.md"]
    newest = board.call("plan", {"path": PATH, "text": PLAN, "uses": uses}, stdin=True)
    brief = board.call("brief", {"path": PATH})
    assert brief["plan"] == newest["text"]
    assert brief["uses"] == [
        {"ref": "/controls/a", "line": "Refusals carry a code."},
        {"ref": "/controls/b", "line": "Stay small."},
        {"ref": "/decisions/d", "line": "Plans are records."},
        {"ref": "/evals/e", "line": "Do trees help? Verdict: supported."},
        {"ref": "/standards/errors.md", "line": "Refusal codes"},
    ]
    assert (brief["left_out"], brief["next"], brief["actor"]) == (0, "build", "agent")
    assert brief["controls"] == "board --personal call controls.check"


def test_brief_stays_within_600_words_with_100_uses_and_counts_what_it_left_out(board: Board) -> None:
    setup(board)
    count = 100
    for number in range(count):
        board.write(f"standards/s{number:03}.md", "# " + "word " * 20 + "\n")
    uses = [f"/standards/s{number:03}.md" for number in range(count)]
    board.call("plan", {"path": PATH, "text": PLAN, "uses": uses}, stdin=True)
    brief = board.call("brief", {"path": PATH})
    shown = len(brief["uses"])
    assert brief["left_out"] == count - shown
    assert 0 < shown < count
    lines = sum(len(f"{use['ref']} {use['line']}".split()) for use in brief["uses"])
    rest = " ".join(str(brief[key]) for key in ("path", "plan", "next", "actor", "controls"))
    assert lines + len(rest.split()) <= BRIEF_WORDS


def test_brief_without_a_plan_record_names_no_plan_and_no_uses(board: Board) -> None:
    setup(board)
    brief = board.call("brief", {"path": PATH})
    assert (brief["plan"], brief["uses"], brief["left_out"], brief["next"]) == (None, [], 0, "plan")
    assert brief["controls"] == CONTROLS


def test_brief_keeps_the_newest_plan_when_a_document_it_uses_is_deleted(board: Board) -> None:
    setup(board)
    board.write("standards/a.md", "# A\n")
    board.write("standards/b.md", "# B\n")
    board.call("plan", {"path": PATH, "text": PLAN, "uses": ["/standards/a.md"]}, stdin=True)
    newest = board.call("plan", {"path": PATH, "text": PLAN, "uses": ["/standards/b.md"]}, stdin=True)
    (board.root / "standards/b.md").unlink()
    brief = board.call("brief", {"path": PATH})
    assert brief["plan"] == newest["text"]
    assert brief["uses"] == [{"ref": "/standards/b.md", "line": "unreadable: reaches nothing or is degraded"}]
