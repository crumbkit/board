import json
import subprocess
import sys
from pathlib import Path

import pytest

HERE = Path(__file__).parent
SCRIPT = HERE.parent / "export_old_board.py"
FAKE = HERE / "fake_board.py"
GOLDEN = HERE / "golden"


def state(subject: str, **fields: object) -> dict:
    return {
        "subject": subject,
        "standing": "active",
        "authority_limitation": None,
        "head_ids": ["0" * 64],
        "known_state_head_ids": ["0" * 64],
        "opaque_record_ids": [],
        "head_kind": "object.declare",
        "summary": f"summary of {subject}",
        "body": None,
        "metadata": {},
        "relationships": [],
        "recorded_at_milliseconds": 1_700_000_000_000,
        "state_record_count": 1,
        "observation_count": 0,
        "outcomes": {"used": 0, "stale": 0, "insufficient": 0},
        "contract_violation": None,
        **fields,
    }


def export(tmp_path: Path, objects: list[dict], paging: str = "after", fail: tuple[str, ...] = ()) -> tuple[Path, dict]:
    tmp_path.mkdir(exist_ok=True)
    fixture = tmp_path / "fixture.json"
    fixture.write_text(json.dumps({"paging": paging, "fail": list(fail), "objects": objects}))
    out = tmp_path / "out"
    (out / ".git").mkdir(parents=True)
    done = subprocess.run(
        [sys.executable, SCRIPT, out, "--board", FAKE],
        capture_output=True,
        check=False,
        env={"FAKE_BOARD_FIXTURE": str(fixture), "PATH": "/usr/bin:/bin"},
    )
    assert done.returncode == 0, done.stderr
    return out, json.loads((out / "export-report.json").read_text())


def reasons(report: dict) -> list[tuple[str, str]]:
    return [(entry["subject"], entry["reason"]) for entry in report["unmappable"]]


def records(out: Path) -> list[str]:
    return sorted(
        str(path.relative_to(out))
        for path in out.rglob("*.json")
        if ".board" not in path.parts and path.name != "export-report.json"
    )


def test_a_subject_becomes_a_record_at_its_logical_path(tmp_path: Path) -> None:
    out, report = export(tmp_path, [state("work:board/core/x"), state("project:board")])
    assert records(out) == ["project/board.json", "work/board/core/x.json"]
    record = json.loads((out / "work/board/core/x.json").read_text())
    assert record["$type"] == "legacy/work/v1"
    assert record["subject"] == "work:board/core/x"
    assert report["record_count"] == 2
    assert report["per_type"] == {"project": 1, "work": 1}
    assert report["unmappable"] == []


def test_each_type_gets_a_permissive_schema_and_the_board_gets_a_config(tmp_path: Path) -> None:
    out, _ = export(tmp_path, [state("work:a"), state("note:b")])
    for name in ("work", "note"):
        assert (out / f".board/types/legacy/{name}/v1.schema.json").read_bytes() == b'{\n  "type": "object"\n}\n'
    assert (out / ".board/board.json").read_bytes() == (
        b'{\n  "$type": "board/config/v1",\n  "commit_on_put": true,\n  "plugins": []\n}\n'
    )


def test_a_record_is_written_in_canonical_bytes(tmp_path: Path) -> None:
    subject = "work:board/core/x"
    tricky = state(
        subject,
        summary='café “quoted” \U0001f600 tab\t"q" back\\slash',
        body="line one\nline two\u0001",
        metadata={"z-key": ["2", "1"], "a-key": [], "é": ["e"], "B": ["upper"]},
        relationships=[{"target": "project:board", "relation": "part-of"}],
        outcomes={"used": 3, "stale": 0, "insufficient": 1},
    )
    out, _ = export(tmp_path, [tricky])
    assert (out / "work/board/core/x.json").read_bytes() == (GOLDEN / "work/board/core/x.json").read_bytes()


@pytest.mark.parametrize(
    ("refused", "admitted", "reason"),
    [
        ("Work:a", "work:a", "type_invalid"),
        ("wo_rk:a", "wo-rk:a", "type_invalid"),
        ("work-a", "work:a", "subject_malformed"),
        ("work:a//b", "work:a/b", "path_malformed"),
        ("work:a/..", "work:a/..b", "path_malformed"),
        ("work:a/.", "work:a/.b", "path_malformed"),
        ("work:a\u0001b", "work:aéb", "path_malformed"),
        ("work:" + "x" * 256 + "/y", "work:" + "x" * 255 + "/y", "path_malformed"),
    ],
)
def test_a_subject_that_is_not_a_logical_path_is_unmappable(
    tmp_path: Path, refused: str, admitted: str, reason: str
) -> None:
    out, report = export(tmp_path / "refused", [state(refused)])
    assert reasons(report) == [(refused, reason)]
    assert report["record_count"] == 0
    assert records(out) == []
    out, report = export(tmp_path / "admitted", [state(admitted)])
    assert reasons(report) == []
    assert report["record_count"] == 1


def test_drops_hidden_by_the_default_list_are_exported(tmp_path: Path) -> None:
    out, report = export(tmp_path, [state("work:a"), state("drop:2026/x")])
    assert records(out) == ["drop/2026/x.json", "work/a.json"]
    assert (report["listed_count"], report["drop_listed_count"], report["subject_count"]) == (1, 1, 2)
    assert (out / ".board/types/legacy/drop/v1.schema.json").exists()


def test_a_subject_listed_twice_is_one_subject(tmp_path: Path) -> None:
    out, report = export(tmp_path, [state("work:a"), state("work:a")])
    assert (report["listed_count"], report["subject_count"], report["record_count"]) == (2, 1, 1)
    assert reasons(report) == []
    assert records(out) == ["work/a.json"]


def test_a_record_and_a_directory_of_the_same_name_are_both_exported(tmp_path: Path) -> None:
    out, report = export(tmp_path, [state("project:board"), state("project:board/core"), state("work:a/b/c")])
    assert reasons(report) == []
    assert records(out) == ["project/board.json", "project/board/core.json", "work/a/b/c.json"]


def test_names_that_differ_only_by_case_collide(tmp_path: Path) -> None:
    out, report = export(
        tmp_path / "refused", [state("work:d/Ab"), state("work:d/ab"), state("work:D/x"), state("work:z")]
    )
    assert reasons(report) == [
        ("work:D/x", "case_collision"),
        ("work:d/Ab", "case_collision"),
        ("work:d/ab", "case_collision"),
    ]
    assert records(out) == ["work/z.json"]
    out, report = export(
        tmp_path / "admitted", [state("work:d/Ab"), state("work:d/ac"), state("work:E/x"), state("work:z")]
    )
    assert reasons(report) == []
    assert report["record_count"] == 4


def test_a_record_file_and_a_directory_differing_by_case_do_not_collide(tmp_path: Path) -> None:
    # On disk they are `x.json` and `X/`, which a case-insensitive filesystem holds together.
    out, report = export(tmp_path, [state("work:x"), state("work:X/y")])
    assert reasons(report) == []
    assert records(out) == ["work/X/y.json", "work/x.json"]


def test_the_report_counts_each_reason(tmp_path: Path) -> None:
    _, report = export(tmp_path, [state("Bad:x"), state("Bad:y"), state("work-a"), state("work:ok")])
    assert report["per_reason"] == {"type_invalid": 2, "subject_malformed": 1}


def test_an_object_the_old_board_cannot_show_is_reported_with_its_error(tmp_path: Path) -> None:
    out, report = export(tmp_path / "refused", [state("work:a"), state("work:b")], fail=("work:a",))
    assert reasons(report) == [("work:a", "show_failed")]
    assert "object show failed" in report["unmappable"][0]["detail"]
    assert records(out) == ["work/b.json"]
    out, report = export(tmp_path / "admitted", [state("work:a"), state("work:b")])
    assert reasons(report) == []


@pytest.mark.parametrize("paging", ["after", "cursor"])
def test_records_equal_distinct_listed_subjects_minus_unmappable_ones_across_pages(tmp_path: Path, paging: str) -> None:
    good = [state(f"work:n{index:04d}") for index in range(600)]
    drops = [state(f"drop:d{index:04d}") for index in range(300)]
    bad = [state("Bad:x"), state("work:N0001"), state("work:n0002")]
    out, report = export(tmp_path, [*good, *drops, *bad], paging=paging, fail=("work:n0003", "drop:d0004"))
    assert (report["listed_count"], report["drop_listed_count"], report["subject_count"]) == (603, 300, 902)
    assert report["per_reason"] == {"type_invalid": 1, "case_collision": 2, "show_failed": 2}
    assert report["record_count"] == len(records(out)) == 902 - 5


def test_the_output_must_hold_nothing_but_git(tmp_path: Path) -> None:
    fixture = tmp_path / "fixture.json"
    fixture.write_text(json.dumps({"paging": "after", "objects": [state("work:a")]}))
    env = {"FAKE_BOARD_FIXTURE": str(fixture), "PATH": "/usr/bin:/bin"}
    out = tmp_path / "out"
    (out / ".git").mkdir(parents=True)
    (out / "stray.txt").write_text("x")
    done = subprocess.run([sys.executable, SCRIPT, out, "--board", FAKE], capture_output=True, check=False, env=env)
    assert done.returncode == 2
    assert b"must be an existing directory holding nothing but .git" in done.stderr
    (out / "stray.txt").unlink()
    done = subprocess.run([sys.executable, SCRIPT, out, "--board", FAKE], capture_output=True, check=False, env=env)
    assert done.returncode == 0, done.stderr
