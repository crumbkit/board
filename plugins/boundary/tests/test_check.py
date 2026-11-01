"""`boundary.check` evaluates content against matching boundaries and records the crossing in the personal board."""

import hashlib
import json
from pathlib import Path
from typing import Any

from conftest import PRIVATE, Agent, Board, boundary, control, make_board, private_names

PR = {"action": "github.pr-create", "to": "github:octo/board", "from": "/repos/board"}


def output(document: dict[str, Any]) -> dict[str, Any]:
    """The capability's output in a `--json` report."""
    assert "data" in document, document
    result: dict[str, Any] = document["data"]["output"]
    return result


def names_boundary(board: Board, **fields: Any) -> None:  # noqa: ANN401 - the boundary's other fields
    """A boundary at /boundaries/octo that checks every part of a pull request to octo against /controls/names."""
    record = {
        "to": "github:octo/*",
        "actions": ["github.pr-*"],
        "checks": [{"control": "/controls/names", "parts": ["*"]}],
    }
    board.write("boundaries/octo.json", boundary(**(record | fields)))


def test_content_that_no_boundary_covers_passes_unchecked_and_its_crossing_says_so(personal: Board) -> None:
    ran, document = personal.call("check", PR | {"parts": {"pr.body": "Fine."}})
    assert ran.returncode == 0, ran.stderr
    result = output(document)
    assert result["boundaries"] == []
    assert result["verdicts"] == []
    assert (
        result["note"]
        == "no boundary matches github.pr-create to github:octo/board from /repos/board; nothing was checked"
    )
    crossing = personal.record(result["crossing"])
    assert crossing is not None
    assert crossing["passed"] is True
    assert crossing["boundaries"] == []


def test_a_private_match_refuses_naming_control_part_and_line_and_never_the_pattern(personal: Board) -> None:
    private_names(personal)
    names_boundary(personal)
    ran, document = personal.call("check", PR | {"parts": {"pr.title": "Fine", "pr.body": "Fine.\nSee acme-123 here."}})
    assert ran.returncode == 1
    assert document["code"] == "boundary::check::failed"
    fields = document["fields"]
    assert fields["failures"] == [{"control": "/controls/names", "part": "pr.body", "text": "acme-123", "line": 2}]
    assert fields["recovery"]["kind"] == "hint"
    assert fields["recovery"]["text"].startswith("Next: change pr.body:")
    assert (
        "run `board call github.pr-create` again, or `board --personal call boundary.check`"
        in fields["recovery"]["text"]
    )
    assert '/controls/names on pr.body line 2: "acme-123"' in document["message"]
    assert PRIVATE not in ran.stdout + ran.stderr
    crossing = personal.record(fields["crossing"])
    assert crossing is not None
    assert crossing["passed"] is False


def test_the_same_content_without_the_match_passes_and_its_crossing_holds_every_verdict(personal: Board) -> None:
    private_names(personal)
    names_boundary(personal)
    ran, document = personal.call("check", PR | {"parts": {"pr.title": "Fine", "pr.body": "Fine.\nSee here."}})
    assert ran.returncode == 0, ran.stderr
    result = output(document)
    assert result["boundaries"] == ["/boundaries/octo"]
    assert result["note"] is None
    assert [(verdict["part"], verdict["status"]) for verdict in result["verdicts"]] == [
        ("pr.body", "passed"),
        ("pr.title", "passed"),
    ]
    crossing = personal.record(result["crossing"])
    assert crossing is not None
    assert crossing["passed"] is True
    assert crossing["verdicts"] == result["verdicts"]
    assert crossing["from"] == "/repos/board"


def test_the_digest_is_the_sha256_of_the_canonical_json_of_destination_action_and_parts(personal: Board) -> None:
    parts = {"pr.body": "Fïne.", "pr.title": "T"}
    _, document = personal.call("check", PR | {"parts": parts})
    canonical = json.dumps(
        {"action": PR["action"], "parts": parts, "to": PR["to"]},
        sort_keys=True,
        separators=(",", ":"),
        ensure_ascii=False,
    )
    expected = hashlib.sha256(canonical.encode()).hexdigest()
    assert output(document)["digest"] == expected
    assert output(document)["crossing"] == f"/crossings/{expected}"


def test_a_check_applies_only_to_the_parts_its_globs_name(personal: Board) -> None:
    personal.write(*control("/controls/short", {"budget": "words", "max": 2}))
    checks = [{"control": "/controls/short", "parts": ["pr.title"]}]
    personal.write("boundaries/octo.json", boundary(to="github:octo/*", actions=["*"], checks=checks))
    ran, document = personal.call("check", PR | {"parts": {"pr.title": "Two words", "pr.body": "Many more words."}})
    assert ran.returncode == 0, ran.stderr
    assert [verdict["part"] for verdict in output(document)["verdicts"]] == ["pr.title"]
    assert output(document)["unchecked"] == ["pr.body"]
    ran, document = personal.call("check", PR | {"parts": {"pr.body": "Many more words."}})
    assert output(document)["boundaries"] == ["/boundaries/octo"]
    assert output(document)["note"] == "the boundaries that match name no control for these parts; nothing was checked"
    ran, document = personal.call("check", PR | {"parts": {"pr.title": "Three words here"}})
    assert ran.returncode == 1
    assert document["fields"]["failures"] == [
        {"control": "/controls/short", "part": "pr.title", "text": "3 words, more than the 2 allowed"}
    ]


def test_a_boundary_matches_by_destination_action_and_checkout(personal: Board) -> None:
    private_names(personal)
    names_boundary(personal, **{"from": ["/repos/board"]})
    parts = {"parts": {"pr.body": "acme-123"}}
    covered = [
        PR,
        PR | {"from": "/repos/board/docs"},
        PR | {"to": "github:Octo/Board"},
        {"action": PR["action"], "to": PR["to"]},
    ]
    uncovered = [
        PR | {"to": "github:other/board"},
        PR | {"action": "github.push"},
        PR | {"from": "/repos/boarding"},
    ]
    for given in covered:
        ran, document = personal.call("check", given | parts)
        assert document.get("code") == "boundary::check::failed", given
    for given in uncovered:
        ran, document = personal.call("check", given | parts)
        assert ran.returncode == 0, given
        assert output(document)["note"] is not None


def test_a_boundary_without_from_covers_a_checkout_no_board_mounts(personal: Board) -> None:
    private_names(personal)
    names_boundary(personal)
    ran, document = personal.call("check", {"action": PR["action"], "to": PR["to"], "parts": {"pr.body": "acme-123"}})
    assert ran.returncode == 1
    assert document["code"] == "boundary::check::failed"


def test_a_judged_rule_is_skipped_for_a_part_another_boundary_fails_on_a_pattern(personal: Board, agent: Agent) -> None:
    private_names(personal)
    names_boundary(personal)
    personal.write("rubrics/plain.md", "Write plainly.\n")
    personal.write(*control("/controls/plain", {"judged": "/rubrics/plain.md"}))
    checks = [{"control": "/controls/plain", "parts": ["pr.body"]}]
    personal.write("boundaries/prose.json", boundary(to="github:*", actions=["github.*"], checks=checks))
    agent.answers([{"line": 1, "text": "Clever.", "rewrite": "Say it plainly."}])
    ran, document = personal.call("check", PR | {"parts": {"pr.body": "Clever. acme-123"}}, agent.variables)
    assert document["code"] == "boundary::check::failed"
    assert agent.calls() == 0
    crossing = personal.record(document["fields"]["crossing"])
    assert crossing is not None
    verdicts = {verdict["control"]: verdict["status"] for verdict in crossing["verdicts"]}
    assert verdicts == {"/controls/names": "failed", "/controls/plain": "skipped"}
    ran, document = personal.call("check", PR | {"parts": {"pr.body": "Clever."}}, agent.variables)
    assert ran.returncode == 1
    assert agent.calls() == 1
    assert document["fields"]["failures"] == [
        {"control": "/controls/plain", "part": "pr.body", "text": "Clever.", "line": 1, "rewrite": "Say it plainly."}
    ]
    assert '"Clever." (Say it plainly.)' in document["message"]


def test_an_invalid_boundary_refuses_every_check_naming_it_and_the_same_boundary_made_valid_is_used(
    personal: Board,
) -> None:
    private_names(personal)
    names_boundary(personal)
    personal.write("boundaries/broken.json", boundary(to="github:*", action=["github.*"], checks=[]))
    ran, document = personal.call("check", PR | {"parts": {"pr.body": "Fine."}})
    assert ran.returncode == 1
    assert document["code"] == "boundary::boundary::invalid"
    assert document["fields"]["boundaries"] == {"/boundaries/broken": "board::record::invalid"}
    assert "run `board --personal check /boundaries/broken`" in document["message"]
    checks = [{"control": "/controls/names", "parts": ["*"]}]
    personal.write("boundaries/broken.json", boundary(to="github:*", actions=["github.*"], checks=checks))
    ran, document = personal.call("check", PR | {"parts": {"pr.body": "Fine."}})
    assert ran.returncode == 0, ran.stderr
    assert output(document)["boundaries"] == ["/boundaries/broken", "/boundaries/octo"]


def test_a_boundary_file_that_does_not_parse_or_has_another_type_refuses_and_made_valid_is_used(
    personal: Board,
) -> None:
    private_names(personal)
    names_boundary(personal)
    for text in ("{", json.dumps({"$type": "boundary/boundry/v1"})):
        personal.write("boundaries/broken.json", text)
        ran, document = personal.call("check", PR | {"parts": {"pr.body": "Fine."}})
        assert ran.returncode == 1, text
        assert document["code"] == "boundary::boundary::invalid", text
        assert list(document["fields"]["boundaries"]) == ["/boundaries/broken"], text
    checks = [{"control": "/controls/names", "parts": ["*"]}]
    personal.write("boundaries/broken.json", boundary(to="github:*", actions=["github.*"], checks=checks))
    ran, document = personal.call("check", PR | {"parts": {"pr.body": "Fine."}})
    assert ran.returncode == 0, ran.stderr
    assert output(document)["boundaries"] == ["/boundaries/broken", "/boundaries/octo"]


def test_an_entry_under_boundaries_that_is_not_a_boundary_refuses_with_the_step_to_move_it_out(
    personal: Board,
) -> None:
    private_names(personal)
    names_boundary(personal)
    personal.write("boundaries/README.md", "# Boundaries\n")
    ran, document = personal.call("check", PR | {"parts": {"pr.body": "Fine."}})
    assert ran.returncode == 1
    assert document["fields"]["boundaries"] == {"/boundaries/README.md": "not a boundary"}
    assert "move each entry that is not a boundary out of /boundaries/" in document["message"]
    (personal.root / "boundaries/README.md").rename(personal.root / "README.md")
    ran, _ = personal.call("check", PR | {"parts": {"pr.body": "Fine."}})
    assert ran.returncode == 0, ran.stderr


def test_a_boundary_outside_boundaries_is_used(personal: Board) -> None:
    private_names(personal)
    checks = [{"control": "/controls/names", "parts": ["*"]}]
    personal.write("policies/octo.json", boundary(to="github:octo/*", actions=["github.*"], checks=checks))
    ran, document = personal.call("check", PR | {"parts": {"pr.body": "acme-123"}})
    assert ran.returncode == 1
    assert document["code"] == "boundary::check::failed"
    ran, document = personal.call("check", PR | {"parts": {"pr.body": "Fine."}})
    assert output(document)["boundaries"] == ["/policies/octo"]


def test_a_symlink_at_boundaries_refuses_and_the_same_files_in_a_directory_pass(personal: Board) -> None:
    private_names(personal)
    checks = [{"control": "/controls/names", "parts": ["*"]}]
    personal.write("policies/octo.json", boundary(to="github:octo/*", actions=["github.*"], checks=checks))
    (personal.root / "boundaries").symlink_to("policies")
    ran, document = personal.call("check", PR | {"parts": {"pr.body": "Fine."}})
    assert ran.returncode == 1
    assert document["code"] == "boundary::boundary::invalid"
    assert document["fields"]["boundaries"]["/boundaries"] == "is not a directory"
    assert "make /boundaries/ a directory" in document["message"]
    (personal.root / "boundaries").unlink()
    (personal.root / "policies").rename(personal.root / "boundaries")
    ran, document = personal.call("check", PR | {"parts": {"pr.body": "Fine."}})
    assert ran.returncode == 0, ran.stderr
    assert output(document)["boundaries"] == ["/boundaries/octo"]


def test_a_control_without_a_rule_fails_the_crossing_and_the_same_control_with_a_rule_passes(personal: Board) -> None:
    written = {"$type": "controls/control/v1", "requirement": "Names.", "purpose": "A test."}
    personal.write("controls/names.json", json.dumps(written))
    names_boundary(personal)
    ran, document = personal.call("check", PR | {"parts": {"pr.body": "Fine."}})
    assert ran.returncode == 1
    assert document["code"] == "boundary::check::failed"
    [failure] = document["fields"]["failures"]
    assert failure["control"] == "/controls/names"
    assert failure["text"].startswith("this control has no rule, so it cannot check content")
    assert document["fields"]["recovery"]["text"].startswith("Next: give each control without a rule a rule")
    private_names(personal)
    ran, _ = personal.call("check", PR | {"parts": {"pr.body": "Fine."}})
    assert ran.returncode == 0, ran.stderr


def test_a_boundary_naming_a_missing_control_refuses_with_the_controls_code(personal: Board) -> None:
    private_names(personal)
    names_boundary(personal, checks=[{"control": "/controls/gone", "parts": ["*"]}])
    ran, document = personal.call("check", PR | {"parts": {"pr.body": "Fine."}})
    assert ran.returncode == 1
    assert document["code"] == "boundary::board::refused"
    assert document["fields"]["code"] == "controls::control::not_found"
    names_boundary(personal)
    ran, _ = personal.call("check", PR | {"parts": {"pr.body": "Fine."}})
    assert ran.returncode == 0, ran.stderr


def test_a_check_called_in_another_board_records_the_crossing_in_the_personal_board(
    personal: Board, tmp_path: Path
) -> None:
    work = make_board(tmp_path / "work", personal.home, ["boundary"])
    private_names(personal)
    names_boundary(personal)
    ran, document = work.call("check", PR | {"parts": {"pr.body": "Fine."}})
    assert ran.returncode == 0, ran.stderr
    path = output(document)["crossing"]
    assert output(document)["boundaries"] == ["/boundaries/octo"]
    assert personal.record(path) is not None
    assert work.record(path) is None
