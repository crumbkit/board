"""`changes.brief`: what an agent needs to start, from the change's newest plan."""

import json
from typing import Any

from conftest import PATHS, PLAN, Board, change, mount_repo, record

from board_changes.plan import BRIEF_WORDS, LEFT_OUT_NAMED

PATH = "/changes/t"
CONTROL = "controls/control/v1"


def spent(brief: dict[str, Any]) -> int:
    """The words `brief` holds, counted as the brief counts them."""
    command = brief["rules_command"]
    counted = [str(brief[key]) for key in ("brief_line", "path", "plan", "phase", "instruction", "controls")]
    if command is not None:
        counted += [*command["argv"], command["stdin"]]
    counted += [f"{rule['control']} {rule['summary']} {' '.join(rule.get('matched', []))}" for rule in brief["rules"]]
    counted += [*brief["rules_left_out"], *(f"{use['ref']} {use['line']}" for use in brief["uses"])]
    return len(" ".join(counted).split())


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
    assert spent(brief) <= BRIEF_WORDS


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


def test_brief_lists_each_control_on_the_planned_paths_once_matched_ones_first_and_what_they_matched(
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
        {"control": "/controls/docs", "summary": "Say what exists.", "matched": ["/repos/x/docs/new.md"]},
        {"control": "/controls/rust", "summary": "Errors are typed.", "matched": ["/repos/x/src/**"]},
        {"control": "/controls/all", "summary": "Write the test first."},
    ]
    assert (brief["rules_left_out"], brief["rules_unnamed"]) == ([], 0)


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


def test_brief_stays_within_600_words_with_100_rules_names_20_it_left_out_and_counts_the_rest(board: Board) -> None:
    board.write("changes/t.json", change())
    mount_repo(board, {"src/a.rs": "a\n"})
    count = 100
    for number in range(count):
        board.write(f"controls/c{number:03}.json", control("word " * 20 + "end.", ["repos/x/*"]))
    board.call("plan", {"path": PATH, "text": PLAN, "uses": [], "paths": PATHS}, stdin=True)
    brief = board.call("brief", {"path": PATH})
    shown = [rule["control"] for rule in brief["rules"]]
    assert 0 < len(shown) < count
    every = [f"/controls/c{number:03}" for number in range(count)]
    assert shown + brief["rules_left_out"] == every[: len(shown) + LEFT_OUT_NAMED]
    assert brief["rules_unnamed"] == count - len(shown) - LEFT_OUT_NAMED
    assert spent(brief) <= BRIEF_WORDS
    command = brief["rules_command"]
    assert command == {"argv": ["board", "call", "controls.for-paths"], "stdin": '{"paths": ["/repos/x/**"]}'}
    listed = board.json(*command["argv"][1:], stdin=command["stdin"])["data"]["output"]["controls"]
    assert [rule["control"] for rule in listed] == every


def test_the_rules_command_needs_no_shell_quoting_for_a_path_holding_a_quote(board: Board) -> None:
    board.write("changes/t.json", change())
    mount_repo(board, {"src/a.rs": "a\n"})
    board.write("controls/quote.json", control("Quote it.", ["repos/x/src/*"]))
    paths = ["/repos/x/src/it's.rs"]
    board.call("plan", {"path": PATH, "text": PLAN, "uses": [], "paths": paths}, stdin=True)
    command = board.call("brief", {"path": PATH})["rules_command"]
    assert json.loads(command["stdin"]) == {"paths": paths}
    listed = board.json(*command["argv"][1:], stdin=command["stdin"])["data"]["output"]["controls"]
    assert [rule["control"] for rule in listed] == ["/controls/quote"]


def test_a_rule_too_long_to_fit_leaves_room_for_the_shorter_rules_after_it(board: Board) -> None:
    board.write("changes/t.json", change())
    mount_repo(board, {"src/a.rs": "a\n"})
    count = 120
    for number in range(count):
        requirement = "word " * 24 + "end." if number % 6 == 0 else "Do."
        board.write(f"controls/c{number:03}.json", control(requirement, ["repos/x/*"]))
    board.call("plan", {"path": PATH, "text": PLAN, "uses": [], "paths": PATHS}, stdin=True)
    brief = board.call("brief", {"path": PATH})
    shown = [rule["control"] for rule in brief["rules"]]
    every = [f"/controls/c{number:03}" for number in range(count)]
    assert brief["rules_left_out"]
    assert int(brief["rules_left_out"][0].removeprefix("/controls/c")) % 6 == 0
    assert every.index(shown[-1]) > every.index(brief["rules_left_out"][0])
    assert spent(brief) <= BRIEF_WORDS


def test_every_rule_gets_its_summary_before_any_rule_gets_its_matched_paths(board: Board) -> None:
    board.write("changes/t.json", change())
    mount_repo(board, {"src/a.rs": "a\n"})
    count = 100
    for number in range(count):
        board.write(f"controls/c{number:03}.json", control("Do it.", ["repos/x/src/*"]))
    paths = [f"/repos/x/src/{name}.rs" for name in ("a", "b", "c")] + ["/repos/x/docs/d.md"]
    board.call("plan", {"path": PATH, "text": PLAN, "uses": [], "paths": paths}, stdin=True)
    brief = board.call("brief", {"path": PATH})
    assert [rule["control"] for rule in brief["rules"]] == [f"/controls/c{number:03}" for number in range(count)]
    assert brief["rules_left_out"] == []
    with_matched = ["matched" in rule for rule in brief["rules"]]
    assert with_matched[0]
    assert not with_matched[-1]
    assert with_matched == sorted(with_matched, reverse=True)
    assert spent(brief) <= BRIEF_WORDS


def test_many_rules_leave_room_for_the_lines_of_the_records_the_plan_uses(board: Board) -> None:
    board.write("changes/t.json", change())
    mount_repo(board, {"src/a.rs": "a\n"})
    for number in range(100):
        board.write(f"controls/c{number:03}.json", control("word " * 20 + "end.", ["repos/x/*"]))
    uses = [f"/standards/s{number}.md" for number in range(5)]
    for use in uses:
        board.write(use.removeprefix("/"), "# " + "word " * 20 + "\n")
    board.call("plan", {"path": PATH, "text": PLAN, "uses": uses, "paths": PATHS}, stdin=True)
    brief = board.call("brief", {"path": PATH})
    assert [use["ref"] for use in brief["uses"]] == uses
    assert brief["rules"]
    assert spent(brief) <= BRIEF_WORDS


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


def test_rules_that_matched_some_planned_paths_come_before_rules_that_matched_them_all(board: Board) -> None:
    board.write("changes/t.json", change())
    mount_repo(board, {"src/a.rs": "a\n"})
    count = 300
    for number in range(count):
        board.write(f"controls/a-extra/e{number:03}.json", control("word " * 10 + "end.", ["repos/x/*"]))
    real = [f"/controls/z-real/r{number}" for number in range(5)]
    for path in real:
        board.write(f"{path.removeprefix('/')}.json", control("Errors are typed.", ["repos/x/src/*"]))
    paths = ["/repos/x/src/a.rs", "/repos/x/docs/b.md"]
    board.call("plan", {"path": PATH, "text": PLAN, "uses": [], "paths": paths}, stdin=True)
    brief = board.call("brief", {"path": PATH})
    shown = [rule["control"] for rule in brief["rules"]]
    assert shown[: len(real)] == real
    assert shown[len(real) :] == sorted(shown[len(real) :])
    assert spent(brief) <= BRIEF_WORDS


def test_the_words_kept_for_left_out_names_go_to_rules_when_every_rule_fits(board: Board) -> None:
    board.write("changes/t.json", change())
    mount_repo(board, {"src/a.rs": "a\n"})
    board.call("plan", {"path": PATH, "text": PLAN, "uses": [], "paths": PATHS}, stdin=True)
    room = BRIEF_WORDS - spent(board.call("brief", {"path": PATH}))
    # Rules of two words each that fill the room, with no word kept for a name.
    count = room // 2
    for number in range(count):
        board.write(f"controls/c{number:03}.json", control("Do.", ["repos/x/*"]))
    brief = board.call("brief", {"path": PATH})
    assert len(brief["rules"]) == count
    assert (brief["rules_left_out"], brief["rules_unnamed"]) == ([], 0)
    assert spent(brief) <= BRIEF_WORDS


def test_a_plan_that_leaves_five_words_stays_within_the_limit_and_counts_the_rules_it_cannot_name(board: Board) -> None:
    board.write("changes/t.json", change())
    mount_repo(board, {"src/a.rs": "a\n"})
    count = 30
    for number in range(count):
        board.write(f"controls/c{number:03}.json", control("Do.", ["repos/x/*"]))
    board.call("plan", {"path": PATH, "text": PLAN, "uses": [], "paths": PATHS}, stdin=True)
    # More planned paths lengthen `rules_command`'s stdin until the fixed parts leave five words.
    room = BRIEF_WORDS - spent(board.call("brief", {"path": PATH})) + 2 * count
    paths = PATHS + [f"/repos/x/f{number}.rs" for number in range(room - 5)]
    board.call("plan", {"path": PATH, "text": PLAN, "uses": [], "paths": paths}, stdin=True)
    brief = board.call("brief", {"path": PATH})
    assert len(brief["rules"]) + len(brief["rules_left_out"]) + brief["rules_unnamed"] == count
    assert brief["rules_unnamed"] > 0
    assert spent(brief) <= BRIEF_WORDS


def test_a_use_too_long_to_fit_leaves_room_for_the_shorter_uses_after_it(board: Board) -> None:
    board.write("changes/t.json", change())
    uses = ["/standards/a.md", "/standards/long.md", "/standards/b.md"]
    board.write("standards/a.md", "# Short.\n")
    board.write("standards/long.md", "# " + "word " * BRIEF_WORDS + "\n")
    board.write("standards/b.md", "# Short.\n")
    board.call("plan", {"path": PATH, "text": PLAN, "uses": uses, "paths": PATHS}, stdin=True)
    brief = board.call("brief", {"path": PATH})
    assert [use["ref"] for use in brief["uses"]] == ["/standards/a.md", "/standards/b.md"]
    assert brief["left_out"] == 1
    assert spent(brief) <= BRIEF_WORDS
