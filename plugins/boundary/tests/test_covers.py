"""`boundary.covers` names the boundaries a check would apply, by the same rules, and checks nothing."""

import json
import subprocess
from typing import Any

from conftest import Board, boundary, private_names

CHECKS = [{"control": "/controls/names", "parts": ["*"]}]


def call(board: Board, given: dict[str, str]) -> tuple[subprocess.CompletedProcess[str], dict[str, Any]]:
    """Run `board --json call boundary.covers` with `given` as flags, and read its one document."""
    flags = [part for name, value in given.items() for part in (f"--{name}", value)]
    ran = board.run("--json", "call", "boundary.covers", *flags)
    document: dict[str, Any] = json.loads(ran.stdout)
    return ran, document


def covered(board: Board, given: dict[str, str]) -> list[str]:
    """The boundaries `boundary.covers` names for `given`."""
    ran, document = call(board, given)
    assert ran.returncode == 0, document
    names: list[str] = document["data"]["output"]["boundaries"]
    return names


def test_a_boundary_covers_its_destination_and_actions_and_not_another(personal: Board) -> None:
    private_names(personal)
    personal.write("boundaries/octo.json", boundary(to="github:Octo/*", actions=["github.pr-*"], checks=CHECKS))
    assert covered(personal, {"action": "github.pr-create", "to": "github:octo/widgets"}) == ["/boundaries/octo"]
    assert covered(personal, {"action": "github.push", "to": "github:octo/widgets"}) == []
    assert covered(personal, {"action": "github.pr-create", "to": "github:other/widgets"}) == []
    assert personal.record("/crossings") is None


def test_a_boundary_limited_by_from_covers_a_checkout_under_it_and_one_that_names_no_checkout(personal: Board) -> None:
    private_names(personal)
    personal.write(
        "boundaries/octo.json", boundary(to="github:*", actions=["github.*"], checks=CHECKS, **{"from": ["/repos/a"]})
    )
    given = {"action": "github.push", "to": "github:octo/widgets"}
    assert covered(personal, given | {"from": "/repos/a/sub"}) == ["/boundaries/octo"]
    assert covered(personal, given) == ["/boundaries/octo"]
    assert covered(personal, given | {"from": "/repos/b"}) == []


def test_an_invalid_boundary_refuses_covers_as_it_refuses_check(personal: Board) -> None:
    personal.write("boundaries/broken.json", "{")
    ran, document = call(personal, {"action": "github.push", "to": "github:octo/widgets"})
    assert (ran.returncode, document["code"]) == (1, "boundary::boundary::invalid")
    (personal.root / "boundaries" / "broken.json").unlink()
    assert covered(personal, {"action": "github.push", "to": "github:octo/widgets"}) == []
