"""`changes.brief`: what an agent needs to start, from the change's newest plan."""

import json

from conftest import PATHS, PLAN, Board, change, mount_repo, record

from board_changes.plan import BRIEF_WORDS

PATH = "/changes/t"
CONTROL = "controls/control/v1"


def test_brief_gives_one_line_for_each_record_the_newest_plan_uses(board: Board) -> None:
    board.write("changes/t.json", change())
    board.write(".board/types/evals/eval/v1.schema.json", '{"type": "object"}')
    board.write("controls/a.json", record(CONTROL, requirement="Refusals carry a code.", purpose="p"))
    board.write("controls/b.json", record(CONTROL, summary="Stay small.", requirement="Long text.", purpose="p"))
    board.write("decisions/d.json", record("plans/decision/v1", decision="Plans are records."))
    board.write("evals/e.json", record("evals/eval/v1", question="Do trees help?", verdict="supported"))
    board.write("standards/errors.md", "Intro.\n\n## Refusal codes\n\nText.\n")
    board.call("plan", {"path": PATH, "text": PLAN, "uses": [], "paths": PATHS}, stdin=True)
    uses = ["/controls/a", "/controls/b", "/decisions/d", "/evals/e", "/standards/errors.md"]
    newest = board.call("plan", {"path": PATH, "text": PLAN, "uses": uses, "paths": PATHS}, stdin=True)
    brief = board.call("brief", {"path": PATH})
    assert brief["plan"] == newest["text"]
    assert brief["uses"] == [
        {"ref": "/controls/a", "line": "Refusals carry a code."},
        {"ref": "/controls/b", "line": "Stay small."},
        {"ref": "/decisions/d", "line": "Plans are records."},
        {"ref": "/evals/e", "line": "Do trees help? Verdict: supported."},
        {"ref": "/standards/errors.md", "line": "Refusal codes"},
    ]
    assert (brief["left_out"], brief["phase"]) == (0, "plan")
    assert brief["controls"] == "board call controls.check --path /repos/x"


def test_brief_stays_within_600_words_with_100_uses_and_counts_what_it_left_out(board: Board) -> None:
    board.write("changes/t.json", change())
    count = 100
    for number in range(count):
        board.write(f"standards/s{number:03}.md", "# " + "word " * 20 + "\n")
    uses = [f"/standards/s{number:03}.md" for number in range(count)]
    board.call("plan", {"path": PATH, "text": PLAN, "uses": uses, "paths": PATHS}, stdin=True)
    brief = board.call("brief", {"path": PATH})
    shown = len(brief["uses"])
    assert brief["left_out"] == count - shown
    assert 0 < shown < count
    lines = sum(len(f"{use['ref']} {use['line']}".split()) for use in brief["uses"])
    keys = ("brief_line", "path", "plan", "phase", "instruction", "controls", "rules_command")
    rest = " ".join(str(brief[key]) for key in keys)
    assert lines + len(rest.split()) <= BRIEF_WORDS


def test_brief_without_a_plan_record_names_no_plan_and_no_uses(board: Board) -> None:
    board.write("changes/t.json", change())
    brief = board.call("brief", {"path": PATH})
    assert (brief["plan"], brief["uses"], brief["left_out"]) == (None, [], 0)


def test_brief_keeps_the_newest_plan_when_a_document_it_uses_is_deleted(board: Board) -> None:
    board.write("changes/t.json", change())
    board.write("standards/a.md", "# A\n")
    board.write("standards/b.md", "# B\n")
    board.call("plan", {"path": PATH, "text": PLAN, "uses": ["/standards/a.md"], "paths": PATHS}, stdin=True)
    newest = board.call("plan", {"path": PATH, "text": PLAN, "uses": ["/standards/b.md"], "paths": PATHS}, stdin=True)
    (board.root / "standards/b.md").unlink()
    brief = board.call("brief", {"path": PATH})
    assert brief["plan"] == newest["text"]
    assert brief["uses"] == [{"ref": "/standards/b.md", "line": "unreadable: reaches nothing or is degraded"}]


def test_brief_starts_with_the_line_the_write_guard_reads(board: Board) -> None:
    board.write("changes/t.json", change())
    brief = board.call("brief", {"path": PATH})
    assert next(iter(brief)) == "brief_line"
    assert brief["brief_line"] == f"board-brief: {PATH}"


def control(requirement: str, paths: list[str], **fields: str) -> str:
    """A control on `paths` with `requirement`."""
    return record(CONTROL, requirement=requirement, purpose="p", paths=paths, **fields)


def test_brief_lists_each_control_on_the_planned_paths_once_and_what_it_matched_unless_it_matched_them_all(
    board: Board,
) -> None:
    board.write("changes/t.json", change())
    mount_repo(board, {"src/a.rs": "a\n", "docs/a.md": "a\n"})
    board.write("controls/rust.json", control("Errors are typed. Always.", ["repos/x/*.rs"]))
    board.write("controls/all.json", control("Test first.", ["repos/x/*"], summary="Write the test first."))
    board.write("controls/docs.json", control("Say what exists.", ["repos/x/docs/*"]))
    board.write("controls/other.json", control("Elsewhere.", ["repos/y/*"]))
    paths = ["/repos/x/src/**", "/repos/x/docs/new.md"]
    board.call("plan", {"path": PATH, "text": PLAN, "uses": [], "paths": paths}, stdin=True)
    brief = board.call("brief", {"path": PATH})
    assert brief["rules"] == [
        {"control": "/controls/all", "summary": "Write the test first."},
        {"control": "/controls/docs", "summary": "Say what exists.", "matched": ["/repos/x/docs/new.md"]},
        {"control": "/controls/rust", "summary": "Errors are typed.", "matched": ["/repos/x/src/**"]},
    ]
    assert brief["rules_left_out"] == []


def test_a_rule_lists_at_most_three_matched_paths_and_counts_the_rest(board: Board) -> None:
    board.write("changes/t.json", change())
    mount_repo(board, {"a.rs": "a\n"})
    board.write("controls/rust.json", control("Errors are typed.", ["repos/x/*.rs"]))
    paths = [f"/repos/x/{name}" for name in ("a.rs", "b.rs", "c.rs", "d.rs", "e.md")]
    board.call("plan", {"path": PATH, "text": PLAN, "uses": [], "paths": paths}, stdin=True)
    [rule] = board.call("brief", {"path": PATH})["rules"]
    assert (rule["matched"], rule["matched_left_out"]) == (paths[:3], 1)
    board.call("plan", {"path": PATH, "text": PLAN, "uses": [], "paths": paths[2:]}, stdin=True)
    [rule] = board.call("brief", {"path": PATH})["rules"]
    assert (rule["matched"], "matched_left_out" in rule) == (paths[2:4], False)


def test_brief_stays_within_600_words_with_100_rules_and_names_each_rule_it_left_out(board: Board) -> None:
    board.write("changes/t.json", change())
    mount_repo(board, {"src/a.rs": "a\n"})
    count = 100
    for number in range(count):
        board.write(f"controls/c{number:03}.json", control("word " * 20 + "end.", ["repos/x/*"]))
    board.call("plan", {"path": PATH, "text": PLAN, "uses": [], "paths": PATHS}, stdin=True)
    brief = board.call("brief", {"path": PATH})
    shown = [rule["control"] for rule in brief["rules"]]
    assert 0 < len(shown) < count
    assert shown + brief["rules_left_out"] == [f"/controls/c{number:03}" for number in range(count)]
    assert brief["rules_command"] == 'board call controls.for-paths <<< \'{"paths": ["/repos/x/**"]}\''
    lines = sum(len(f"{r['control']} {r['summary']}".split()) for r in brief["rules"])
    keys = ("brief_line", "path", "plan", "phase", "instruction", "controls", "rules_command")
    rest = " ".join(str(brief[key]) for key in keys)
    assert lines + len(brief["rules_left_out"]) + len(rest.split()) <= BRIEF_WORDS


def test_brief_without_the_controls_plugin_is_refused_and_with_it_is_admitted(board: Board) -> None:
    board.write("changes/t.json", change())
    board.call("plan", {"path": PATH, "text": PLAN, "uses": [], "paths": PATHS}, stdin=True)
    config = board.read(".board/board.json")
    board.write(".board/board.json", json.dumps({**config, "plugins": ["changes"]}))
    refusal = board.refusal("brief", {"path": PATH})
    assert refusal["code"] == "changes::brief::controls_missing", refusal
    board.write(".board/board.json", json.dumps(config))
    assert board.call("brief", {"path": PATH})["rules"] == []


def test_brief_of_a_plan_without_paths_lists_no_rules(board: Board) -> None:
    board.write("changes/t.json", change())
    brief = board.call("brief", {"path": PATH})
    assert (brief["rules"], brief["rules_left_out"], brief["rules_command"]) == ([], [], None)
