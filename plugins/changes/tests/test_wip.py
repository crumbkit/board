"""The soft WIP limit on active changes in build per mount, and the paused phase that holds a change outside it."""

import dataclasses
from datetime import date
from typing import TYPE_CHECKING, Any

import pytest
from conftest import PLAN, TODAY, Board, change, commit, days_ago, git, mount_repo, record

from board_changes.wip import WIP_LIMIT, building, split, stored

if TYPE_CHECKING:
    from board_changes.change import Change

PATH = "/changes/t"
# An active-days window wide enough to hold changes touched ten days ago.
WIDE = 30


def held(phase: str, **fields: Any) -> "Change":  # noqa: ANN401 - the change's other fields
    """A change in `phase` as the cap reads it."""
    return {"$type": "changes/change/v1", "title": "T", "mount": "/repos/x", "phase": phase, **fields}  # type: ignore[typeddict-item]


def test_the_cap_counts_build_changes_in_build_apart_from_prototypes_exploring() -> None:
    changes = [
        held("build"),
        held("build", kind="build"),
        held("paused"),
        held("review"),
        held("build", kind="prototype"),
        held("paused", kind="prototype"),
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
    assert refused["fields"]["active"] == [f"/changes/busy-{number}" for number in range(WIP_LIMIT)]
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


def test_split_counts_a_change_active_on_any_of_the_last_n_days_today_included_and_none_without_a_day() -> None:
    newest = {"/a": date(2026, 11, 2), "/b": date(2026, 11, 1), "/c": None, "/d": date(2026, 11, 4)}
    assert split(newest, date(2026, 11, 4), 3) == (["/a", "/d"], ["/b", "/c"])


def test_advance_past_the_limit_with_a_reason_goes_ahead_records_it_and_warns(board: Board) -> None:
    ready(board)
    fill(board, WIP_LIMIT)
    refused = board.refusal("advance", {"path": PATH})
    override = f"board call changes.advance --path {PATH} --over_limit '<why>'"
    assert (refused["code"], refused["fields"]["override"]) == ("changes::wip::full", override)
    output = board.call("advance", {"path": PATH, "over_limit": "The owner wants this before the others."})
    assert output["to"] == "build"
    warning = output["warning"]
    assert (warning["code"], warning["mount"], warning["limit"], warning["active_days"]) == (
        "changes::wip::over_limit",
        "/repos/x",
        WIP_LIMIT,
        3,
    )
    assert warning["active"] == [f"/changes/busy-{number}" for number in range(WIP_LIMIT)]
    recorded = board.read("changes/t.json")
    assert (recorded["over_limit"], recorded["touched"]) == ("The owner wants this before the others.", TODAY)


def idle(board: Board) -> None:
    """Three changes in build touched ten days ago: one with a pull request, one with a head, one with neither."""
    board.write(
        "changes/idle-pr.json", change("build", touched=days_ago(10), pull_request="https://github.com/o/r/pull/1")
    )
    board.write("changes/idle-head.json", change("build", touched=days_ago(10), head="a3f9c0d"))
    board.write("changes/idle-bare.json", change("build", touched=days_ago(10)))


def test_idle_changes_do_not_count_and_the_refusal_lists_each_with_its_action(board: Board) -> None:
    ready(board)
    idle(board)
    assert board.call("advance", {"path": PATH})["to"] == "build"
    board.write("changes/t.json", change(plan="/changes/t/plan.md", route="owner"))
    fill(board, WIP_LIMIT)
    refused = board.refusal("advance", {"path": PATH})
    assert refused["code"] == "changes::wip::full"
    assert refused["fields"]["idle"] == [
        {
            "path": "/changes/idle-bare",
            "action": "drop",
            "command": "board call changes.drop --path /changes/idle-bare --reason '<why>'",
        },
        {
            "path": "/changes/idle-head",
            "action": "resume",
            "command": "board call changes.next --path /changes/idle-head",
        },
        {"path": "/changes/idle-pr", "action": "sync", "command": "board call changes.sync"},
    ]


def test_the_limit_counts_only_changes_for_the_same_mount(board: Board) -> None:
    ready(board)
    fill(board, WIP_LIMIT, mount="/repos/y")
    assert board.call("advance", {"path": PATH})["to"] == "build"


def test_a_run_that_finished_today_keeps_an_untouched_change_active(board: Board) -> None:
    ready(board)
    board.write("suites/s/runs/r.json", record("runs/run/v1", finished=f"{TODAY}T09:00:00Z"))
    fill(board, WIP_LIMIT - 1)
    board.write("changes/ran.json", change("build", touched=days_ago(10), records=["/suites/s/runs/r"]))
    assert board.refusal("advance", {"path": PATH})["code"] == "changes::wip::full"
    board.write("suites/s/runs/r.json", record("runs/run/v1", finished=f"{days_ago(10)}T09:00:00Z"))
    assert board.call("advance", {"path": PATH})["to"] == "build"


def test_a_head_committed_today_keeps_an_untouched_change_active(board: Board) -> None:
    ready(board)
    mount_repo(board, {"README.md": "x\n"})
    head = commit(board, {"a.txt": "a\n"})
    fill(board, WIP_LIMIT - 1)
    board.write("changes/committed.json", change("build", touched=days_ago(10), head=head))
    assert board.refusal("advance", {"path": PATH})["code"] == "changes::wip::full"
    board.write("changes/committed.json", change("build", touched=days_ago(10), head="b" * 40))
    assert board.call("advance", {"path": PATH})["to"] == "build"


def test_board_changes_active_days_widens_the_window_and_refuses_a_value_that_is_not_a_number_of_days(
    board: Board,
) -> None:
    ready(board)
    fill(board, WIP_LIMIT, touched=days_ago(10))
    wide = dataclasses.replace(board, env=(("BOARD_CHANGES_ACTIVE_DAYS", str(WIDE)),))
    assert wide.refusal("advance", {"path": PATH})["fields"]["active_days"] == WIDE
    bad = dataclasses.replace(board, env=(("BOARD_CHANGES_ACTIVE_DAYS", "soon"),))
    refused = bad.refusal("advance", {"path": PATH})
    assert (refused["code"], refused["fields"]["value"]) == ("changes::wip::bad_active_days", "soon")
    assert board.call("advance", {"path": PATH})["to"] == "build"


def test_resume_past_the_limit_with_a_reason_goes_ahead_and_warns(board: Board) -> None:
    board.write("changes/t.json", change("paused"))
    fill(board, WIP_LIMIT)
    output = board.call("resume", {"path": PATH, "over_limit": "Blocked work elsewhere."})
    assert (output["to"], output["warning"]["code"]) == ("build", "changes::wip::over_limit")
    assert board.read("changes/t.json")["over_limit"] == "Blocked work elsewhere."


def test_attach_touches_a_change_so_it_counts_as_active_again(board: Board) -> None:
    board.write("changes/t.json", change("build", touched=days_ago(10)))
    board.call("attach", {"path": PATH, "head": "a3f9c0d"})
    assert board.read("changes/t.json")["touched"] == TODAY


def repository(board: Board, at: str, origin: str | None) -> str:
    """A clone mounted at `/<at>` whose origin is `origin`, or a clone with no origin; its mount."""
    mount = mount_repo(board, {"README.md": "x\n"}, at=at)
    if origin is not None:
        git(board.root.parent / at.replace("/", "-"), "remote", "add", "origin", origin)
    return mount


def building_in(board: Board, mount: str, count: int, prefix: str) -> list[str]:
    """`count` active changes in build for the repository at `mount`, named `<prefix>-<n>`; their paths."""
    for number in range(count):
        board.write(f"changes/{prefix}-{number}.json", change("build", mount=mount))
    return [f"/changes/{prefix}-{number}" for number in range(count)]


def ready_in(board: Board, mount: str) -> None:
    """A change in plan for the repository at `mount` whose gate passes."""
    board.write("changes/t.json", change(plan="/changes/t/plan.md", route="owner", mount=mount))
    board.write("changes/t/plan.md", PLAN)


def test_mounts_of_one_repository_count_together_however_its_origin_is_written(board: Board) -> None:
    first = repository(board, "repos/a", "https://github.com/crumbkit/a.git")
    second = repository(board, "repos/a-again", "git@github.com:crumbkit/a.git")
    third = repository(board, "repos/a-www", "https://www.github.com/Crumbkit/A")
    fourth = repository(board, "repos/a-port", "ssh://git@github.com:22/crumbkit/a.git")
    other = repository(board, "repos/c", "git@github.com:crumbkit/c.git")
    holding = building_in(board, first, 1, "a") + building_in(board, second, 1, "b") + building_in(board, third, 1, "c")
    ready_in(board, fourth)
    refused = board.refusal("advance", {"path": PATH})
    assert refused["code"] == "changes::wip::full"
    assert (refused["fields"]["repository"], refused["fields"]["mount"]) == ("crumbkit/a", fourth)
    assert refused["fields"]["active"] == holding
    assert refused["fields"]["unread"] == {}
    ready_in(board, other)
    assert board.call("advance", {"path": PATH})["to"] == "build"


def test_a_clone_whose_origin_git_cannot_read_counts_by_its_clone_and_the_refusal_says_why(board: Board) -> None:
    a = repository(board, "repos/a", None)
    building_in(board, a, WIP_LIMIT, "a")
    ready_in(board, a)
    refused = board.refusal("advance", {"path": PATH})
    assert refused["code"] == "changes::wip::full"
    assert refused["fields"]["repository"] == str(board.root.parent / "repos-a")
    assert list(refused["fields"]["unread"]) == ["/repos/a"]
    assert "origin" in refused["fields"]["unread"]["/repos/a"]
    ready_in(board, repository(board, "repos/b", None))
    assert board.call("advance", {"path": PATH})["to"] == "build"


def test_the_over_limit_warning_names_the_repository_it_counted(board: Board) -> None:
    a = repository(board, "repos/a", "https://github.com/crumbkit/a.git")
    again = repository(board, "repos/a-again", "git@github.com:crumbkit/a.git")
    building_in(board, a, WIP_LIMIT, "a")
    ready_in(board, again)
    warning = board.call("advance", {"path": PATH, "over_limit": "The owner asked for it."})["warning"]
    assert (warning["code"], warning["repository"], warning["mount"]) == (
        "changes::wip::over_limit",
        "crumbkit/a",
        again,
    )


def test_prototypes_of_one_repository_count_together_apart_from_build_changes(board: Board) -> None:
    a = repository(board, "repos/a", "https://github.com/crumbkit/a.git")
    again = repository(board, "repos/a-again", "git@github.com:crumbkit/a.git")
    building_in(board, a, WIP_LIMIT, "a")
    for number in range(WIP_LIMIT):
        board.write(f"changes/p-{number}.json", change("build", mount=a, kind="prototype", state="exploring"))
    board.write("changes/t.json", change("paused", mount=again, kind="prototype", state="exploring"))
    refused = board.refusal("resume", {"path": PATH})
    assert (refused["code"], refused["fields"]["kind"]) == ("changes::wip::full", "prototype")
    assert refused["fields"]["active"] == [f"/changes/p-{number}" for number in range(WIP_LIMIT)]
    board.write("changes/t.json", change("paused", mount=repository(board, "repos/b", None), kind="prototype"))
    assert board.call("resume", {"path": PATH})["to"] == "build"


@pytest.mark.parametrize(
    "origin",
    [
        "https://github.com/o/r.git",
        "git@github.com:o/r.git",
        "git@GitHub.com:o/r.git",
        "git@www.github.com:o/r",
        "HTTPS://github.com/o/r",
        "https://www.github.com/O/R",
        "ssh://git@github.com:22/o/r.git",
        "git+ssh://git@github.com/o/r",
        "SSH://git@GITHUB.COM/o/r.git",
    ],
)
def test_each_way_of_writing_a_github_origin_has_one_stored_form(origin: str) -> None:
    assert stored(origin) == "o/r"


def test_an_origin_off_github_keeps_its_written_form() -> None:
    assert stored("https://gitlab.com/o/r.git") == "https://gitlab.com/o/r.git"


def clone_of(board: Board, at: str, origin: str) -> str:
    """A plain git repository at `at` beside the board, whose origin is `origin`; its path."""
    root = board.root.parent / at
    root.mkdir()
    git(root, "init", "-q")
    git(root, "remote", "add", "origin", origin)
    return str(root)


@pytest.mark.parametrize("prefix", ["", "file://"])
def test_a_clone_of_a_local_clone_counts_with_the_repository_that_clone_follows(board: Board, prefix: str) -> None:
    a = repository(board, "repos/a", "https://github.com/crumbkit/a.git")
    candidate = repository(board, "repos/a-candidate", prefix + str(board.root.parent / "repos-a"))
    holding = building_in(board, a, WIP_LIMIT, "a")
    ready_in(board, candidate)
    refused = board.refusal("advance", {"path": PATH})
    assert refused["code"] == "changes::wip::full"
    assert (refused["fields"]["repository"], refused["fields"]["active"]) == ("crumbkit/a", holding)
    assert refused["fields"]["unread"] == {}
    other = repository(board, "repos/b-candidate", prefix + clone_of(board, "b", "https://github.com/crumbkit/b.git"))
    ready_in(board, other)
    assert board.call("advance", {"path": PATH})["to"] == "build"


def test_a_chain_of_local_clones_past_three_hops_counts_by_the_last_and_says_so(board: Board) -> None:
    end = clone_of(board, "end", "https://github.com/crumbkit/a.git")
    third = clone_of(board, "third", end)
    second = clone_of(board, "second", third)
    first = clone_of(board, "first", second)
    long = repository(board, "repos/long", first)
    building_in(board, long, WIP_LIMIT, "l")
    ready_in(board, long)
    refused = board.refusal("advance", {"path": PATH})
    assert refused["code"] == "changes::wip::full"
    assert refused["fields"]["repository"] == end
    assert "3" in refused["fields"]["unread"]["/repos/long"]
    short = repository(board, "repos/short", second)
    ready_in(board, short)
    assert board.call("advance", {"path": PATH})["to"] == "build"
