"""The `available` capability through the real `board`: sections, scope by remote, sync and degraded files.

Dates are counted from the real today, at least three days apart, so a call
that crosses midnight answers the same. test_sweep.py holds the clock rules.
"""

import datetime as dt
import json
import shutil
import time
from pathlib import Path
from typing import Any
from zoneinfo import ZoneInfo

import pytest
from conftest import Board, git, notes_repo, task_v2, zone_at_one_in_the_morning

TODAY = dt.date.today()  # noqa: DTZ011 - the plugin reads the local day too
SOON = (TODAY + dt.timedelta(days=3)).isoformat()
LATER = (TODAY + dt.timedelta(days=30)).isoformat()
PAST = (TODAY - dt.timedelta(days=3)).isoformat()
RECENT = TODAY.isoformat()


def call(board: Board, *flags: str, cwd: Path | None = None, tz: str | None = None) -> tuple[int, dict[str, Any]]:
    """`board --json call tasks.available <flags>` from `cwd`, by default outside any git repository.

    It returns the exit code, and the output or failure.
    """
    outside = board.root.parent / "outside"
    outside.mkdir(exist_ok=True)
    ran = board.run("--json", "call", "tasks.available", *flags, cwd=cwd or outside, tz=tz)
    document: dict[str, Any] = json.loads(ran.stdout)
    return ran.returncode, document["data"]["output"] if ran.returncode == 0 else document


def titles(output: dict[str, Any], section: str = "available") -> list[str]:
    """The titles in one section of `available`'s output."""
    return [item["title"] for item in output[section]["tasks"]]


@pytest.fixture
def estate(board: Board) -> Board:
    """The board, with a task of its own, a personal notes repo and a work one covering example-org/*."""
    board.write("tasks/board.md", task_v2("Board task", reviewed=RECENT))
    notes_repo(board, "personal", {"covers": [], "personal": True}, {"home": task_v2("Home task", reviewed=RECENT)})
    notes_repo(board, "work", {"covers": ["example-org/*"]}, {"work": task_v2("Work task", reviewed=RECENT)})
    notes_repo(board, "other", {"covers": ["acme/*"]}, {"acme": task_v2("Acme task", reviewed=RECENT)})
    return board


def test_available_lists_open_tasks_neither_deferred_nor_blocked_by_title(board: Board) -> None:
    board.write("tasks/b.md", task_v2("B", reviewed=RECENT))
    board.write("tasks/a.md", task_v2("A", reviewed=RECENT, defer=PAST))
    board.write("tasks/later.md", task_v2("Later", reviewed=RECENT, defer=LATER))
    board.write("tasks/soon.md", task_v2("Soon", reviewed=RECENT, defer="soon"))
    board.write("tasks/done.md", task_v2("Done", status="done"))
    board.write("tasks/dropped.md", task_v2("Dropped", status="dropped"))
    code, output = call(board)
    assert code == 0, output
    assert titles(output) == ["A", "B"]
    assert (output["repos"], output["degraded"], output["unsynced"]) == (["/"], 0, [])


def test_a_task_waits_while_a_task_in_its_after_is_not_done_and_the_blocker_counts_it(board: Board) -> None:
    board.write("tasks/blocker.md", task_v2("Blocker", reviewed=RECENT))
    board.write("tasks/waiter.md", task_v2("Waiter", reviewed=RECENT, after=["/tasks/blocker.md", "/tasks/gone.md"]))
    board.write("tasks/finished.md", task_v2("Finished", status="done"))
    board.write("tasks/released.md", task_v2("Released", reviewed=RECENT, after=["/tasks/finished.md"]))
    code, output = call(board)
    assert code == 0, output
    shown = {item["title"]: (item["waiting"], item["blocked_by"]) for item in output["available"]["tasks"]}
    assert shown == {"Blocker": (1, []), "Released": (0, [])}
    board.write("tasks/blocker.md", task_v2("Blocker", status="done"))
    board.write("tasks/waiter.md", task_v2("Waiter", reviewed=RECENT, after=["/tasks/blocker.md"]))
    assert titles(call(board)[1]) == ["Released", "Waiter"]


def test_due_soon_and_stale_are_sections_of_their_own(board: Board) -> None:
    board.write("tasks/due.md", task_v2("Due", reviewed=RECENT, due=SOON, defer=LATER))
    board.write("tasks/far.md", task_v2("Far", reviewed=RECENT, due=LATER))
    board.write("tasks/unreviewed.md", task_v2("Unreviewed", after=["/tasks/far.md"]))
    board.write("tasks/deferred.md", task_v2("Deferred", defer=LATER))
    output = call(board)[1]
    assert (titles(output), titles(output, "due_soon"), titles(output, "stale")) == (["Far"], ["Due"], ["Unreviewed"])


def test_context_narrows_every_section_to_tasks_with_that_tag(board: Board) -> None:
    board.write("tasks/milk.md", task_v2("Milk", reviewed=RECENT, contexts=["errands"], due=SOON))
    board.write("tasks/call.md", task_v2("Call", reviewed=RECENT, contexts=["phone"], due=SOON))
    board.write("tasks/think.md", task_v2("Think", reviewed=RECENT))
    output = call(board, "--context", "errands")[1]
    assert (titles(output), titles(output, "due_soon")) == (["Milk"], ["Milk"])
    assert titles(call(board)[1]) == ["Call", "Milk", "Think"]


def test_a_task_file_that_is_not_a_readable_task_is_counted_as_degraded(board: Board) -> None:
    board.write("tasks/good.md", task_v2("Good", reviewed=RECENT))
    assert call(board)[1]["degraded"] == 0
    board.write("tasks/broken.md", "---\n$type: tasks/task/v2\ntitle: [Broken\n---\n")
    board.write("tasks/invalid.md", task_v2("Invalid", status="waiting"))
    board.write("tasks/untyped.md", "# A note, not a task\n")
    board.write("tasks/dangling.md", task_v2("Dangling", reviewed=RECENT, after=["/tasks/gone.md"]))
    board.write("notes/elsewhere.md", task_v2("Elsewhere", reviewed=RECENT))
    output = call(board)[1]
    assert (output["degraded"], titles(output)) == (3, ["Good"])


def test_a_done_task_check_degrades_counts_as_degraded_and_its_valid_done_twin_neither_shows_nor_counts(
    board: Board,
) -> None:
    board.write("tasks/good.md", task_v2("Good", reviewed=RECENT))
    board.write("tasks/finished.md", task_v2("Finished", status="done", contexts=["home"]))
    output = call(board)[1]
    assert (output["degraded"], titles(output)) == (0, ["Good"])
    board.write("tasks/finished.md", task_v2("Finished", status="done", contexts=["Not A Tag"]))
    output = call(board)[1]
    assert (output["degraded"], titles(output)) == (1, ["Good"])


def test_a_done_task_the_schema_admits_and_the_model_refuses_counts_as_degraded_and_its_valid_twin_does_not(
    board: Board,
) -> None:
    # JSON Schema's date-time admits a leap second, which the plugin's model refuses.
    board.write("tasks/good.md", task_v2("Good", reviewed=RECENT))
    board.write("tasks/finished.md", task_v2("Finished", status="done", defer="2026-06-30T23:59:59Z"))
    output = call(board)[1]
    assert (output["degraded"], titles(output)) == (0, ["Good"])
    board.write("tasks/finished.md", task_v2("Finished", status="done", defer="2026-06-30T23:59:60Z"))
    output = call(board)[1]
    assert (output["degraded"], titles(output)) == (1, ["Good"])


def test_the_remote_chooses_its_covering_repo_with_the_personal_repo_and_the_boards_own_tree(estate: Board) -> None:
    code, output = call(estate, "--remote", "git@github.com:example-org/home.git")
    assert code == 0, output
    assert output["repos"] == ["/", "/repos/personal", "/repos/work"]
    assert titles(output) == ["Board task", "Home task", "Work task"]
    assert [item["repo"] for item in output["available"]["tasks"]] == ["/", "/repos/personal", "/repos/work"]
    assert call(estate, "--remote", "crumbkit/viitta")[1]["repos"] == ["/", "/repos/personal"]


def test_without_a_remote_every_repo_is_read_and_repo_names_one(estate: Board) -> None:
    assert call(estate)[1]["repos"] == ["/", "/repos/other", "/repos/personal", "/repos/work"]
    output = call(estate, "--repo", "/repos/work/", "--remote", "acme/x")[1]
    assert (output["repos"], titles(output)) == (["/repos/work"], ["Work task"])
    code, refused = call(estate, "--repo", "/repos/elsewhere")
    assert (code, refused["code"]) == (1, "tasks::repo::not_found"), refused
    assert refused["fields"]["repos"] == ["/", "/repos/other", "/repos/personal", "/repos/work"]


def phone(board: Board, name: str) -> Path:
    """A second clone of the notes repo `name`'s remote, as a phone holds it."""
    clone = board.root.parent / f"{name}-phone"
    git(board.root.parent, "clone", "-q", str(board.root.parent / f"{name}.git"), str(clone))
    git(clone, "config", "user.name", "t")
    git(clone, "config", "user.email", "t@t")
    return clone


def push(clone: Path, files: dict[str, str]) -> None:
    """Commit `files` in `clone` and push them."""
    for path, text in files.items():
        (clone / path).parent.mkdir(parents=True, exist_ok=True)
        (clone / path).write_text(text)
    git(clone, "add", ".")
    git(clone, "commit", "-qm", "from the phone")
    git(clone, "push", "-q")


def test_a_call_syncs_the_repos_in_scope_and_an_after_in_a_mount_resolves_in_that_mount(board: Board) -> None:
    clone = notes_repo(board, "personal", {"covers": [], "personal": True}, {})
    board.write("tasks/first.md", task_v2("Board first", reviewed=RECENT))
    second = task_v2("Second", reviewed=RECENT, after=["/tasks/first.md"])
    mobile = phone(board, "personal")
    push(mobile, {"tasks/first.md": task_v2("First", reviewed=RECENT), "tasks/second.md": second})
    code, output = call(board, "--repo", "/repos/personal")
    assert code == 0, output
    assert (output["unsynced"], titles(output)) == ([], ["First"])
    [first] = output["available"]["tasks"]
    assert (first["path"], first["waiting"]) == ("/repos/personal/tasks/first.md", 1)
    assert (clone / "tasks" / "second.md").exists(), "the clone synced"
    push(mobile, {"tasks/first.md": task_v2("First", status="done")})
    assert titles(call(board, "--repo", "/repos/personal")[1]) == ["Second"], "the open /tasks/first.md is the board's"


def test_a_pin_that_cannot_move_lists_its_repo_as_unsynced_with_the_code_and_the_call_still_answers(
    estate: Board,
) -> None:
    push(phone(estate, "work"), {"tasks/more.md": task_v2("More", reviewed=RECENT)})
    (estate.root / "repos").chmod(0o555)
    try:
        code, output = call(estate, "--repo", "/repos/work")
    finally:
        (estate.root / "repos").chmod(0o755)
    assert code == 0, output
    [unsynced] = output["unsynced"]
    assert unsynced["repo"] == "/repos/work"
    assert unsynced["code"].startswith("board::"), unsynced
    assert titles(output) == ["Work task"], "read at the pin"
    code, output = call(estate, "--repo", "/repos/work")
    assert (code, output["unsynced"], titles(output)) == (0, [], ["More", "Work task"])


def test_a_sync_past_its_deadline_is_left_to_finish_and_its_repo_read_at_its_pin(estate: Board) -> None:
    clone = estate.root.parent / "work"
    git(clone, "config", "core.sshCommand", "sleep 6 #")
    git(clone, "remote", "set-url", "origin", "ssh://git@example.invalid/work.git")
    code, output = call(estate, "--repo", "/repos/work")
    assert code == 0, output
    assert output["unsynced"] == [{"repo": "/repos/work", "code": "tasks::repo::sync_timeout"}]
    assert titles(output) == ["Work task"]
    git(clone, "remote", "set-url", "origin", str(estate.root.parent / "work.git"))
    lock = clone / ".git" / "board.lock"
    deadline = time.monotonic() + 30
    while lock.exists() and time.monotonic() < deadline:
        time.sleep(0.2)
    assert not lock.exists(), "the sync left running finished, and released Board's lock"
    assert call(estate, "--repo", "/repos/work")[1]["unsynced"] == []


def test_a_repo_that_cannot_sync_is_listed_with_its_code_and_read_as_its_clone_holds_it(estate: Board) -> None:
    shutil.rmtree(estate.root.parent / "work.git")
    code, output = call(estate, "--remote", "example-org/home")
    assert code == 0, output
    assert output["unsynced"] == [{"repo": "/repos/work", "code": "board::sync::failed"}]
    assert titles(output) == ["Board task", "Home task", "Work task"]


def test_available_answers_the_same_from_a_cold_cache(estate: Board) -> None:
    warm = call(estate)[1]
    shutil.rmtree(estate.cache)
    assert call(estate)[1] == warm


def test_a_board_with_no_task_answers_with_empty_sections(board: Board) -> None:
    code, output = call(board)
    assert code == 0, output
    assert [output[name]["total"] for name in ("available", "due_soon", "stale", "held")] == [0, 0, 0, 0]
    board.write("tasks/first.md", task_v2("First", reviewed=RECENT))
    assert titles(call(board)[1]) == ["First"]


def test_a_mount_whose_clone_has_not_moved_keeps_its_pin_unwritten(estate: Board) -> None:
    pointer = estate.root / "repos" / "work.json"
    written = pointer.stat().st_mtime_ns
    code, output = call(estate, "--repo", "/repos/work")
    assert (code, titles(output)) == (0, ["Work task"]), output
    assert pointer.stat().st_mtime_ns == written
    clone = estate.root.parent / "work"
    (clone / "tasks" / "more.md").write_text(task_v2("More", reviewed=RECENT))
    git(clone, "add", ".")
    git(clone, "commit", "-qm", "more")
    assert titles(call(estate, "--repo", "/repos/work")[1]) == ["More", "Work task"]
    assert pointer.stat().st_mtime_ns != written


def test_inside_a_git_repository_without_origin_only_the_boards_tree_and_the_personal_repo_are_read_and_why(
    estate: Board,
) -> None:
    checkout = estate.root.parent / "checkout"
    git(estate.root.parent, "init", "-q", str(checkout))
    code, output = call(estate, cwd=checkout)
    assert code == 0, output
    assert output["repos"] == ["/", "/repos/personal"]
    assert output["origin_unread"].startswith(f"git gave no origin for {checkout.resolve()}: "), output
    git(checkout, "remote", "add", "origin", "git@github.com:example-org/home.git")
    output = call(estate, cwd=checkout)[1]
    assert (output["repos"], output["origin_unread"]) == (["/", "/repos/personal", "/repos/work"], None)
    output = call(estate)[1]
    assert (output["repos"], output["origin_unread"]) == (["/", "/repos/other", "/repos/personal", "/repos/work"], None)


def test_an_after_naming_a_task_in_another_repo_is_listed_and_never_blocks_and_one_in_its_own_repo_does(
    estate: Board,
) -> None:
    after = ["/repos/work/tasks/work.md", "/tasks/board.md"]
    estate.write("tasks/crossing.md", task_v2("Crossing", after=after))
    output = call(estate)[1]
    assert output["cross_repo"] == [{"path": "/tasks/crossing.md", "after": "/repos/work/tasks/work.md"}]
    [crossing] = [item for item in output["stale"]["tasks"] if item["title"] == "Crossing"]
    assert crossing["blocked_by"] == ["/tasks/board.md"]
    estate.write("tasks/board.md", task_v2("Board task", status="done"))
    output = call(estate)[1]
    assert "Crossing" in titles(output)
    assert output["cross_repo"] == [{"path": "/tasks/crossing.md", "after": "/repos/work/tasks/work.md"}]


def test_a_task_deferred_to_today_waits_before_the_configured_day_start_and_not_without_one(board: Board) -> None:
    zone = zone_at_one_in_the_morning()
    today = dt.datetime.now(ZoneInfo(zone)).date().isoformat()
    board.write("tasks/today.md", task_v2("Today", reviewed=RECENT, defer=today))
    (board.config / "board").mkdir(parents=True)
    (board.config / "board" / "config.json").write_text(json.dumps({"day_start": "07:00"}))
    code, output = call(board, tz=zone)
    assert (code, titles(output)) == (0, []), output
    (board.config / "board" / "config.json").write_text("{}")
    assert titles(call(board, tz=zone)[1]) == ["Today"]


def test_owner_keeps_the_tasks_it_or_nobody_holds_and_lists_its_unavailable_ones_in_held(board: Board) -> None:
    board.write("tasks/mine.md", task_v2("Mine", reviewed=RECENT, owner="lead-1"))
    board.write("tasks/free.md", task_v2("Free", reviewed=RECENT))
    board.write("tasks/theirs.md", task_v2("Theirs", reviewed=RECENT, owner="worker-2"))
    board.write("tasks/parked.md", task_v2("Parked", reviewed=RECENT, owner="lead-1", defer=LATER))
    code, output = call(board, "--owner", "lead-1")
    assert code == 0, output
    assert (titles(output), titles(output, "held"), output["held"]["total"]) == (["Free", "Mine"], ["Parked"], 1)
    assert [item["owner"] for item in output["available"]["tasks"]] == [None, "lead-1"]
    output = call(board)[1]
    assert (titles(output), titles(output, "held")) == (["Free", "Mine", "Theirs"], [])
