"""A failed `boundary.check` asks the owner to override it, and the owner's approval lets that exact content pass."""

import json
import os
import subprocess
from pathlib import Path
from typing import Any

from conftest import Board, boundary, private_names

PR = {"action": "github.pr-create", "to": "github:octo/board"}
FAILING = {"pr.title": "Fine", "pr.body": "See acme-123 here."}


def covered(board: Board) -> None:
    """A boundary that refuses private names in every part of a pull request to octo."""
    private_names(board)
    checks = [{"control": "/controls/names", "parts": ["*"]}]
    board.write("boundaries/octo.json", boundary(to="github:octo/*", actions=["github.*"], checks=checks))


def pending(board: Board) -> list[Path]:
    return sorted((board.root / "effects" / "pending").glob("*.json"))


def test_a_failed_check_asks_the_owner_once_and_names_the_effect_after_the_next_step(personal: Board) -> None:
    covered(personal)
    ran, document = personal.call("check", PR | {"parts": FAILING})
    assert ran.returncode == 1
    assert document["code"] == "boundary::check::failed"
    fields = document["fields"]
    effect = fields["effect"]
    [asked] = pending(personal)
    assert asked.stem == effect
    record = json.loads(asked.read_text())
    assert (record["plugin"], record["op"], record["path"]) == ("boundary", "boundary.override", fields["crossing"])
    assert (
        record["text"] == "github.pr-create to github:octo/board\n\npr.body:\nSee acme-123 here.\n\npr.title:\nFine\n"
    )
    owner = f"The owner may approve this exact content instead with `board answer {effect} approve`; then check again."
    assert fields["recovery"]["text"].startswith("Next: change pr.body:")
    assert fields["recovery"]["text"].endswith(owner)
    assert document["message"].endswith(owner)
    ran, again = personal.call("check", PR | {"parts": FAILING})
    assert ran.returncode == 1
    assert again["fields"]["effect"] == effect
    assert pending(personal) == [asked]


def test_a_passing_check_asks_the_owner_nothing(personal: Board) -> None:
    covered(personal)
    ran, document = personal.call("check", PR | {"parts": {"pr.body": "See here."}})
    assert ran.returncode == 0, ran.stderr
    assert document["data"]["output"]["override"] is None
    assert pending(personal) == []
    assert "override" not in (personal.record(document["data"]["output"]["crossing"]) or {})


def test_an_approved_override_passes_that_exact_content_and_records_the_effect(personal: Board) -> None:
    covered(personal)
    _, refused = personal.call("check", PR | {"parts": FAILING})
    crossing, effect = refused["fields"]["crossing"], refused["fields"]["effect"]
    personal.approve(crossing, effect)
    ran, document = personal.call("check", PR | {"parts": FAILING})
    assert ran.returncode == 0, ran.stderr
    result = document["data"]["output"]
    assert (result["crossing"], result["override"]) == (crossing, effect)
    assert [(verdict["part"], verdict["status"]) for verdict in result["verdicts"]] == [
        ("pr.body", "failed"),
        ("pr.title", "passed"),
    ]
    record = personal.record(crossing)
    assert record is not None
    assert (record["passed"], record["override"]) == (True, effect)
    verified = personal.run("--json", "call", "boundary.verify", "--digest", result["digest"])
    assert json.loads(verified.stdout)["data"]["output"]["passed"] is True
    recorded = personal.run("--json", "call", "boundary.record", "--digest", result["digest"], "--result", "u")
    assert recorded.returncode == 0, recorded.stdout


def test_an_approval_does_not_let_through_content_that_differs_by_one_character(personal: Board) -> None:
    covered(personal)
    _, refused = personal.call("check", PR | {"parts": FAILING})
    personal.approve(refused["fields"]["crossing"], refused["fields"]["effect"])
    changed = FAILING | {"pr.title": "Fine."}
    ran, document = personal.call("check", PR | {"parts": changed})
    assert ran.returncode == 1
    assert document["code"] == "boundary::check::failed"
    assert document["fields"]["crossing"] != refused["fields"]["crossing"]
    assert document["fields"]["effect"] != refused["fields"]["effect"]
    assert len(pending(personal)) == 2  # noqa: PLR2004 - one ask for each content


def test_an_override_the_owner_refused_stays_refused_and_does_not_offer_approval(personal: Board) -> None:
    covered(personal)
    _, asked = personal.call("check", PR | {"parts": FAILING})
    effect = asked["fields"]["effect"]
    seen = personal.root / "effects" / "seen" / f"{effect}.json"
    seen.parent.mkdir(parents=True)
    # The owner's refusal needs no signature, so the test writes it as `board answer` would.
    seen.write_text(json.dumps({"$type": "board/seen/v1", "effect": f"/effects/pending/{effect}", "answer": "refuse"}))
    git = ["git", "-C", str(personal.root), "-c", "user.name=t", "-c", "user.email=t@t"]
    isolated = os.environ | {"GIT_CONFIG_GLOBAL": os.devnull, "GIT_CONFIG_NOSYSTEM": "1"}
    for arguments in (["add", "."], ["commit", "-qm", "refuse"]):
        subprocess.run([*git, *arguments], env=isolated, check=True, capture_output=True)
    ran, document = personal.call("check", PR | {"parts": FAILING})
    assert ran.returncode == 1
    assert document["code"] == "boundary::check::failed"
    fields: dict[str, Any] = document["fields"]
    assert fields["effect"] == effect
    refused = f"The owner refused to let this exact content through ({effect})."
    assert fields["recovery"]["text"].endswith(refused)
    assert "board answer" not in document["message"]
    record = personal.record(fields["crossing"])
    assert record is not None
    assert record["passed"] is False
    assert "override" not in record
