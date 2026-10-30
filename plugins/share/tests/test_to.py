"""The `to` capability, through the real `board`, from a personal board into a mounted team board."""

import json
import subprocess
from pathlib import Path

from conftest import EVAL, EVAL_SCHEMA, RUN, RUN_SCHEMA, SUITE_SCHEMA, Boards, commit, experiment, git, repository

from board_share.to import Line, same

SHARED = ["/evals/x", "/evals/x/evidence/log.txt", "/notes/cases.md", "/suites/x", "/suites/x/runs/1"]
FILES = sorted(
    [
        EVAL_SCHEMA,
        RUN_SCHEMA,
        SUITE_SCHEMA,
        "evals/x.json",
        "evals/x/evidence/log.txt",
        "notes/cases.md",
        "suites/x.json",
        "suites/x/runs/1.json",
    ]
)
TYPES = ["evals/eval/v1", "runs/run/v1", "runs/suite/v1"]


def changed(root: Path, commit: str) -> list[str]:
    """The files `commit` changed in the repository at `root`, sorted."""
    return sorted(git(root, "show", "--name-only", "--format=", commit).splitlines())


def test_one_call_copies_an_eval_its_runs_their_evidence_and_its_schemas_in_one_commit_and_a_clone_checks_clean(
    boards: Boards, tmp_path: Path
) -> None:
    experiment(boards)
    ran = boards.run(boards.personal, "--json", "call", "share.to", "--path", "/evals/x", "--to", "/team")
    assert ran.returncode == 0, ran.stdout
    output = json.loads(ran.stdout)["data"]["output"]
    head = git(boards.team, "rev-parse", "HEAD")
    assert output == {
        "repository": str(boards.team),
        "commit": head,
        "types": TYPES,
        "copied": SHARED,
        "total": 5,
        "truncated": False,
        "unchanged": 0,
    }
    assert changed(boards.team, head) == FILES
    assert git(boards.team, "rev-list", "--count", "HEAD") == "2", "the fixture's commit and the share's"
    assert boards.pin() == head
    assert (boards.personal / "evals" / "x.json").exists(), "the source keeps its record"
    clone = tmp_path / "clone"
    subprocess.run(["git", "clone", "-q", str(boards.team), str(clone)], check=True)
    checked = boards.run(clone, "check")
    assert checked.returncode == 0, checked.stdout
    location = json.loads(boards.run(clone, "--json", "get", "/evals/x").stdout)["data"][0]["location"]
    assert (location["repository"], location["path"]) == (str(clone), "evals/x.json")


def test_a_schema_the_team_holds_is_not_added_again_and_one_it_lacks_lands_with_the_records(
    boards: Boards,
) -> None:
    experiment(boards)
    boards.write(boards.team, RUN_SCHEMA, (boards.personal / RUN_SCHEMA).read_text())
    before = boards.publish()
    code, document = boards.share(path="/evals/x")
    assert code == 0, document
    assert document["data"]["output"]["types"] == ["evals/eval/v1", "runs/suite/v1"]
    assert git(boards.team, "rev-list", "--count", f"{before}..HEAD") == "1"
    assert EVAL_SCHEMA in changed(boards.team, "HEAD")
    assert RUN_SCHEMA not in changed(boards.team, "HEAD")


def test_a_path_the_team_holds_differently_refuses_the_share_and_writes_nothing_and_one_it_holds_equally_is_left(
    boards: Boards,
) -> None:
    experiment(boards)
    boards.write(boards.team, "suites/x/runs/1.json", json.dumps({**RUN, "by": "someone else"}))
    before = boards.publish()
    code, document = boards.share(path="/evals/x")
    assert (code, document["code"]) == (1, "share::target::differs"), document
    assert document["fields"]["paths"] == ["/suites/x/runs/1"]
    assert git(boards.team, "rev-parse", "HEAD") == before
    assert not (boards.team / ".board" / "types" / "evals").exists()
    boards.write(boards.team, "suites/x/runs/1.json", json.dumps(RUN, indent=4))
    boards.publish()
    code, document = boards.share(path="/evals/x")
    assert code == 0, document
    assert (document["data"]["output"]["total"], document["data"]["output"]["unchanged"]) == (4, 1)


def test_a_schema_the_team_holds_differently_refuses_the_share_and_an_equal_one_is_admitted(boards: Boards) -> None:
    experiment(boards)
    schema = json.loads((boards.personal / RUN_SCHEMA).read_text())
    boards.write(boards.team, RUN_SCHEMA, json.dumps({**schema, "title": "Another run"}))
    before = boards.publish()
    code, document = boards.share(path="/evals/x")
    assert (code, document["code"]) == (1, "share::target::differs")
    assert document["fields"]["paths"] == ["/.board/types/runs/run/v1.schema"]
    assert git(boards.team, "rev-parse", "HEAD") == before
    boards.write(boards.team, RUN_SCHEMA, json.dumps(schema))
    boards.publish()
    assert boards.share(path="/evals/x")[0] == 0


def test_sharing_again_writes_nothing(boards: Boards) -> None:
    experiment(boards)
    first = boards.share(path="/evals/x")[1]["data"]["output"]["commit"]
    code, document = boards.share(path="/evals/x")
    assert code == 0, document
    output = document["data"]["output"]
    assert (output["commit"], output["copied"], output["unchanged"], output["types"]) == (None, [], 5, [])
    assert git(boards.team, "rev-parse", "HEAD") == first
    assert boards.pin() == first


def test_a_share_killed_before_the_pin_moved_is_finished_by_sharing_again(boards: Boards) -> None:
    experiment(boards)
    first = boards.share(path="/evals/x")[1]["data"]["output"]["commit"]
    pointer = json.loads((boards.personal / "team.json").read_text())
    behind = git(boards.team, "rev-parse", "HEAD~1")
    boards.write(boards.personal, "team.json", json.dumps({**pointer, "commit": behind}))
    code, document = boards.share(path="/evals/x")
    assert code == 0, document
    assert document["data"]["output"]["commit"] == first
    assert git(boards.team, "rev-parse", "HEAD") == first
    assert boards.pin() == first


def test_a_share_killed_before_the_mounted_board_committed_is_finished_by_sharing_again(boards: Boards) -> None:
    experiment(boards)
    assert boards.share(path="/evals/x")[0] == 0
    git(boards.team, "reset", "-q", "--mixed", "HEAD~1")
    git(boards.personal, "reset", "-q", "--hard", "HEAD~1")
    before = git(boards.team, "rev-parse", "HEAD")
    assert boards.pin() == before, "the copies are written in the team checkout and neither board committed"
    code, document = boards.share(path="/evals/x")
    assert code == 0, document
    assert git(boards.team, "rev-list", "--count", f"{before}..HEAD") == "1"
    assert changed(boards.team, "HEAD") == FILES
    assert boards.pin() == git(boards.team, "rev-parse", "HEAD")
    assert git(boards.team, "status", "--porcelain") == ""


def test_a_shared_path_holding_other_uncommitted_bytes_is_refused_naming_it_and_admitted_once_they_are_removed(
    boards: Boards,
) -> None:
    experiment(boards)
    boards.write(boards.team, "notes/cases.md", "# Edited\n")
    before = git(boards.team, "rev-parse", "HEAD")
    code, document = boards.share(path="/evals/x")
    assert (code, document["code"], document["fields"]["code"]) == (1, "share::board::refused", "board::put::invalid")
    [refusal] = document["fields"]["refusals"]
    assert refusal["code"] == "board::mount::stale"
    assert "/team/notes/cases.md" in refusal["message"]
    assert (git(boards.team, "rev-parse", "HEAD"), boards.pin()) == (before, before)
    assert git(boards.team, "status", "--porcelain") == "?? notes/"
    (boards.team / "notes" / "cases.md").unlink()
    assert boards.share(path="/evals/x")[0] == 0


def test_the_documented_command_line_runs_with_flags_named_after_the_input_and_a_dashed_flag_is_refused(
    boards: Boards,
) -> None:
    experiment(boards)
    before = git(boards.team, "rev-parse", "HEAD")
    call = ["--json", "call", "share.to", "--path", "/evals/x", "--to", "/team"]
    ran = boards.run(boards.personal, *call, "--dry_run", "true", "--local_attachments", "true")
    assert ran.returncode == 0, ran.stdout
    assert json.loads(ran.stdout)["data"]["output"]["copied"] == SHARED
    assert git(boards.team, "rev-parse", "HEAD") == before
    refused = boards.run(boards.personal, *call, "--dry-run", "true")
    assert (refused.returncode, json.loads(refused.stdout)["code"]) == (2, "board::cli::usage"), refused.stdout
    ran = boards.run(boards.personal, *call)
    assert ran.returncode == 0, ran.stdout
    assert boards.pin() == git(boards.team, "rev-parse", "HEAD") != before


def test_a_dry_run_lists_what_it_would_copy_and_writes_nothing(boards: Boards) -> None:
    experiment(boards)
    before = git(boards.team, "rev-parse", "HEAD")
    code, document = boards.share(path="/evals/x", dry_run=True)
    assert code == 0, document
    output = document["data"]["output"]
    assert (output["commit"], output["copied"], output["types"]) == (None, SHARED, TYPES)
    assert git(boards.team, "rev-parse", "HEAD") == before
    assert boards.pin() == before


def test_a_path_with_nothing_in_the_boards_own_tree_is_refused_and_one_with_a_record_is_shared(
    boards: Boards,
) -> None:
    experiment(boards)
    code, document = boards.share(path="/evals/y")
    assert (code, document["code"]) == (1, "share::path::not_found")
    assert boards.share(path="/evals/x")[0] == 0


def test_a_reference_through_a_mount_the_team_lacks_is_refused_and_one_the_team_mounts_too_is_kept(
    boards: Boards, tmp_path: Path
) -> None:
    library = tmp_path / "library"
    repository(library)
    head = commit(library)
    pointer = json.dumps({"$type": "board/mount/v1", "repo": str(library), "follow": "main", "commit": head})
    boards.write(boards.personal, "repos/library.json", pointer)
    experiment(boards, mounts=["/repos/library"])
    code, document = boards.share(path="/evals/x")
    assert (code, document["code"]) == (1, "share::closure::unresolved")
    assert document["fields"]["paths"] == ["/repos/library"]
    boards.write(boards.team, "repos/library.json", pointer)
    boards.publish()
    code, document = boards.share(path="/evals/x")
    assert code == 0, document
    assert "/repos/library" not in document["data"]["output"]["copied"]


def attachment(location: str) -> str:
    """A `board/attachment/v1` record whose bytes are at `location`."""
    digest = "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08"
    record = {"$type": "board/attachment/v1", "digest": f"sha256:{digest}", "size": 4, "media_type": "image/png"}
    return json.dumps({**record, "location": location})


def test_an_attachment_kept_on_this_machine_is_refused_unless_asked_and_one_in_a_remote_store_is_copied(
    boards: Boards,
) -> None:
    experiment(boards)
    boards.write(boards.personal, "evals/x/evidence/shot.png.json", attachment("/Users/me/objects/9f86"))
    code, document = boards.share(path="/evals/x")
    assert (code, document["code"]) == (1, "share::attachment::local")
    assert document["fields"]["paths"] == ["/evals/x/evidence/shot.png"]
    code, document = boards.share(path="/evals/x", local_attachments=True, dry_run=True)
    assert code == 0, document
    assert "/evals/x/evidence/shot.png" in document["data"]["output"]["copied"]
    boards.write(boards.personal, "evals/x/evidence/shot.png.json", attachment("s3://objects/9f86"))
    code, document = boards.share(path="/evals/x")
    assert code == 0, document
    assert (boards.team / "evals" / "x" / "evidence" / "shot.png.json").exists()


def test_a_binary_file_is_refused_because_a_batch_writes_only_text_and_a_text_file_is_copied(boards: Boards) -> None:
    experiment(boards)
    (boards.personal / "evals" / "x" / "evidence" / "raw.bin").write_bytes(b"\xff\x00")
    code, document = boards.share(path="/evals/x")
    assert (code, document["code"]) == (1, "share::file::unsupported")
    assert document["fields"]["paths"] == ["/evals/x/evidence/raw.bin"]
    (boards.personal / "evals" / "x" / "evidence" / "raw.bin").write_bytes(b"ok\n")
    assert boards.share(path="/evals/x")[0] == 0


def test_the_eval_record_is_copied_as_its_value(boards: Boards) -> None:
    experiment(boards)
    assert boards.share(path="/evals/x")[0] == 0
    assert json.loads((boards.team / "evals" / "x.json").read_text()) == EVAL


def test_a_mount_that_reaches_no_board_is_refused_and_one_that_reaches_the_team_is_admitted(boards: Boards) -> None:
    experiment(boards)
    code, document = boards.share(path="/evals/x", to="/nowhere", dry_run=True)
    assert (code, document["code"]) == (1, "share::mount::not_found"), document
    code, document = boards.share(path="/evals/x", to="/evals", dry_run=True)
    assert (code, document["code"]) == (1, "share::mount::not_found"), document
    assert boards.share(path="/evals/x", to="/team", dry_run=True)[0] == 0


def test_a_path_inside_the_mount_is_refused_and_the_mount_itself_is_admitted(boards: Boards) -> None:
    experiment(boards)
    boards.write(boards.team, "sub/a.md", "# A\n")
    boards.publish()
    code, document = boards.share(path="/evals/x", to="/team/sub", dry_run=True)
    assert (code, document["code"]) == (1, "share::mount::not_found"), document
    assert boards.share(path="/evals/x", to="/team", dry_run=True)[0] == 0


def test_a_mount_inside_the_mounted_board_is_refused_and_its_repository_mounted_here_is_admitted(
    boards: Boards, tmp_path: Path
) -> None:
    inner = tmp_path / "inner"
    repository(inner)
    pointer = json.dumps({"$type": "board/mount/v1", "repo": str(inner), "follow": "main", "commit": commit(inner)})
    boards.write(boards.team, "inner.json", pointer)
    boards.publish()
    experiment(boards)
    code, document = boards.share(path="/evals/x", to="/team/inner", dry_run=True)
    assert (code, document["code"]) == (1, "share::mount::not_found"), document
    boards.write(boards.personal, "inner.json", pointer)
    assert boards.share(path="/evals/x", to="/inner", dry_run=True)[0] == 0


def test_a_link_to_the_mount_is_refused_and_the_mount_it_links_to_is_admitted(boards: Boards) -> None:
    experiment(boards)
    boards.write(boards.personal, "alias.json", json.dumps({"$type": "board/mount/v1", "path": "/team"}))
    code, document = boards.share(path="/evals/x", to="/alias", dry_run=True)
    assert (code, document["code"]) == (1, "share::mount::not_found"), document
    assert boards.share(path="/evals/x", to="/team", dry_run=True)[0] == 0


def test_a_record_its_schema_refuses_is_refused_before_any_write_and_a_dangling_reference_is_admitted(
    boards: Boards,
) -> None:
    experiment(boards, status="sideways")
    before = git(boards.team, "rev-parse", "HEAD")
    code, document = boards.share(path="/evals/x")
    assert (code, document["code"]) == (1, "share::record::invalid"), document
    assert document["fields"]["paths"] == ["/evals/x"]
    assert git(boards.team, "rev-parse", "HEAD") == before
    experiment(boards, cases_file="/notes/missing.md")
    code, document = boards.share(path="/evals/x")
    assert code == 0, document
    assert "/notes/cases.md" not in document["data"]["output"]["copied"]


def test_a_link_inside_the_set_is_refused_and_one_outside_it_is_left_alone(boards: Boards, tmp_path: Path) -> None:
    library = tmp_path / "library"
    repository(library)
    head = commit(library)
    pointer = json.dumps({"$type": "board/mount/v1", "repo": str(library), "follow": "main", "commit": head})
    experiment(boards)
    boards.write(boards.personal, "evals/x/library.json", pointer)
    code, document = boards.share(path="/evals/x")
    assert (code, document["code"]) == (1, "share::link::unsupported"), document
    assert document["fields"]["paths"] == ["/evals/x/library"]
    (boards.personal / "evals" / "x" / "library.json").rename(boards.personal / "library.json")
    code, document = boards.share(path="/evals/x")
    assert code == 0, document
    assert document["data"]["output"]["copied"] == SHARED


def test_a_reference_to_a_directory_copies_the_entries_under_it(boards: Boards) -> None:
    experiment(boards, cases_file="/notes")
    boards.write(boards.personal, "notes/more/other.md", "# Other\n")
    code, document = boards.share(path="/evals/x")
    assert code == 0, document
    assert document["data"]["output"]["copied"] == sorted([*SHARED, "/notes/more/other.md"])
    assert (boards.team / "notes" / "more" / "other.md").read_text() == "# Other\n"


def test_a_note_a_record_references_is_copied_and_the_notes_and_directories_it_links_to_are_not(
    boards: Boards,
) -> None:
    experiment(boards)
    boards.write(boards.personal, "notes/cases.md", "# Cases\n\nSee [other](other.md) and [more](more/).\n")
    boards.write(boards.personal, "notes/other.md", "# Other\n")
    boards.write(boards.personal, "notes/more/third.md", "# Third\n")
    code, document = boards.share(path="/evals/x")
    assert code == 0, document
    assert document["data"]["output"]["copied"] == SHARED
    assert (boards.team / "notes" / "cases.md").exists()
    assert not (boards.team / "notes" / "other.md").exists()


# A task written as Markdown: its frontmatter names a type, which makes it a record.
TASK_SCHEMA = ".board/types/lr/task/v1.schema.json"
TASK_TYPE = json.dumps(
    {
        "type": "object",
        "properties": {
            "eval": {"type": "string", "format": "board-ref"},
            "see": {"type": "string", "format": "board-ref"},
        },
    }
)


def test_a_markdown_records_frontmatter_references_and_schema_are_copied_and_its_body_links_are_not(
    boards: Boards,
) -> None:
    experiment(boards)
    boards.write(boards.personal, TASK_SCHEMA, TASK_TYPE)
    task = "---\n$type: lr/task/v1\neval: /evals/x\n---\n# Task\n\nSee [other](../notes/other.md).\n"
    boards.write(boards.personal, "tasks/t.md", task)
    boards.write(boards.personal, "notes/other.md", "# Other\n")
    code, document = boards.share(path="/tasks/t.md")
    assert code == 0, document
    output = document["data"]["output"]
    assert output["copied"] == sorted([*SHARED, "/tasks/t.md"])
    assert output["types"] == sorted([*TYPES, "lr/task/v1"])
    assert (boards.team / "tasks" / "t.md").read_text() == task
    assert (boards.team / TASK_SCHEMA).exists()
    assert not (boards.team / "notes" / "other.md").exists()


def test_a_body_link_written_as_a_frontmatter_pointer_is_not_followed_and_the_reference_at_that_pointer_is(
    boards: Boards,
) -> None:
    experiment(boards)
    boards.write(boards.personal, TASK_SCHEMA, TASK_TYPE)
    boards.write(boards.personal, "see/x.md", "# Linked\n")
    boards.write(boards.personal, "other/y.md", "# Referenced\n")
    # The reference at the pointer `/see` reaches /other, and the link written `/see` reaches /see.
    boards.write(boards.personal, "tasks/t.md", "---\n$type: lr/task/v1\nsee: /other\n---\n[s](/see)\n")
    code, document = boards.share(path="/tasks/t.md", dry_run=True)
    assert code == 0, document
    assert document["data"]["output"]["copied"] == ["/other/y.md", "/tasks/t.md"]
    boards.write(boards.personal, "tasks/t.md", "---\n$type: lr/task/v1\nsee: /see\n---\n[s](/see)\n")
    code, document = boards.share(path="/tasks/t.md", dry_run=True)
    assert code == 0, document
    assert document["data"]["output"]["copied"] == ["/see/x.md", "/tasks/t.md"]


def test_a_markdown_record_its_schema_refuses_is_refused_before_any_write_and_its_valid_twin_is_copied(
    boards: Boards,
) -> None:
    experiment(boards)
    boards.write(boards.personal, TASK_SCHEMA, TASK_TYPE)
    boards.write(boards.personal, "tasks/t.md", "---\n$type: lr/task/v1\neval: 1\n---\n")
    code, document = boards.share(path="/tasks/t.md")
    assert (code, document["code"]) == (1, "share::record::invalid"), document
    boards.write(boards.personal, "tasks/t.md", "---\n$type: lr/task/v1\neval: /evals/x\n---\n")
    assert boards.share(path="/tasks/t.md")[0] == 0


def test_typed_frontmatter_that_does_not_parse_is_refused_on_a_dry_run_and_its_parsing_twin_is_listed(
    boards: Boards,
) -> None:
    experiment(boards)
    boards.write(boards.personal, TASK_SCHEMA, TASK_TYPE)
    boards.write(boards.personal, "tasks/t.md", "---\n$type: lr/task/v1\ntitle: [\n---\n")
    code, document = boards.share(path="/tasks/t.md", dry_run=True)
    assert (code, document["code"]) == (1, "share::record::invalid"), document
    boards.write(boards.personal, "tasks/t.md", "---\n$type: lr/task/v1\ntitle: '['\n---\n")
    code, document = boards.share(path="/tasks/t.md", dry_run=True)
    assert code == 0, document
    assert document["data"]["output"]["copied"] == ["/tasks/t.md"]


def test_a_json_file_without_a_type_is_refused_and_the_same_text_in_a_text_file_is_copied(boards: Boards) -> None:
    experiment(boards)
    boards.write(boards.personal, "evals/x/evidence/data.json", '{"a": 1}')
    code, document = boards.share(path="/evals/x")
    assert (code, document["code"]) == (1, "share::file::unsupported"), document
    assert document["fields"]["paths"] == ["/evals/x/evidence/data"]
    (boards.personal / "evals" / "x" / "evidence" / "data.json").rename(
        boards.personal / "evals" / "x" / "evidence" / "data.txt"
    )
    assert boards.share(path="/evals/x")[0] == 0


def test_a_record_held_with_true_where_the_source_has_1_is_different_and_one_in_another_key_order_is_the_same() -> None:
    line: Line = {"record": {"a": 1, "b": "c"}}
    assert not same(line, b'{"a": true, "b": "c"}')
    assert same(line, b'{"b": "c", "a": 1}')


def test_the_output_lists_at_most_100_paths_and_counts_them_all(boards: Boards) -> None:
    experiment(boards)
    for number in range(101):
        boards.write(boards.personal, f"evals/x/evidence/{number:03}.txt", f"{number}\n")
    code, document = boards.share(path="/evals/x", dry_run=True)
    assert code == 0, document
    output = document["data"]["output"]
    assert (len(output["copied"]), output["total"], output["truncated"]) == (100, 106, True)


def test_a_team_checkout_on_another_branch_refuses_the_share_and_one_on_the_followed_branch_takes_it(
    boards: Boards,
) -> None:
    experiment(boards)
    git(boards.team, "switch", "-qc", "feature")
    before = git(boards.team, "rev-parse", "HEAD")
    code, document = boards.share(path="/evals/x")
    assert (code, document["code"]) == (1, "share::board::refused"), document
    assert (git(boards.team, "rev-parse", "HEAD"), boards.pin()) == (before, before)
    git(boards.team, "switch", "-q", "main")
    code, document = boards.share(path="/evals/x")
    assert code == 0, document
    assert boards.pin() == git(boards.team, "rev-parse", "main")


def test_a_team_board_that_does_not_commit_on_put_refuses_the_share_and_one_that_does_takes_it(boards: Boards) -> None:
    experiment(boards)
    boards.write(boards.team, ".board/board.json", json.dumps({"$type": "board/config/v1", "commit_on_put": False}))
    before = boards.publish()
    code, document = boards.share(path="/evals/x")
    assert (code, document["code"]) == (1, "share::board::refused"), document
    assert git(boards.team, "status", "--porcelain") == ""
    boards.write(boards.team, ".board/board.json", json.dumps({"$type": "board/config/v1", "commit_on_put": True}))
    boards.publish()
    code, document = boards.share(path="/evals/x")
    assert code == 0, document
    assert git(boards.team, "rev-parse", "HEAD") != before
