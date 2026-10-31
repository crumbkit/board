"""A brief and a report can name a `subject`, a record pinned by its blob, in place of a commit range."""

import json
from typing import TYPE_CHECKING, Any, cast

from conftest import Repo

from board_review.post import comment

if TYPE_CHECKING:
    from board_review.shapes import ReportRecord

INVALID = "board::record::invalid"
PLAN = "notes/plan.md"
COMMITS = "a" * 40 + ".." + "b" * 40
FOCUS = "Check the plan states only what is new."


def pinned(repo: Repo, path: str = PLAN, text: str = "# Plan\n\nShip it.\n") -> dict[str, str]:
    """Commit `text` at `path` and return a subject pinning it at its blob."""
    repo.commit(path, text)
    return {"record": f"/{path}", "blob": repo.git("hash-object", path)}


def checked(repo: Repo) -> dict[str, str | None]:
    """Each review record's code from `board check`, by its path; `None` when it is valid."""
    report = json.loads(repo.run("--json", "check", "--all").stdout)
    return {entry["logical"]: entry.get("code") for entry in report["data"] if entry["logical"].startswith("/reviews/")}


def brief(repo: Repo, name: str, **fields: object) -> str:
    """Commit a brief at /reviews/<name>/brief holding a focus and `fields`, and return its path."""
    repo.commit(f"reviews/{name}/brief.json", json.dumps({"$type": "review/brief/v1", "focus": FOCUS} | fields))
    return f"/reviews/{name}/brief"


def report(repo: Repo, name: str, **fields: object) -> str:
    """Commit a person's approving report at /reviews/<name>/report holding `fields`, and return its path."""
    record: dict[str, object] = {"$type": "review/report/v1", "findings": [], "verdict": "approve", "by": "a person"}
    record |= fields
    repo.commit(f"reviews/{name}/report.json", json.dumps(record))
    return f"/reviews/{name}/report"


def test_a_brief_names_a_range_or_a_subject_and_one_with_both_or_neither_is_refused(repo: Repo) -> None:
    subject = pinned(repo)
    brief(repo, "range", range=COMMITS)
    brief(repo, "subject", subject=subject)
    brief(repo, "both", range=COMMITS, subject=subject)
    brief(repo, "neither")
    assert checked(repo) == {
        "/reviews/range/brief": None,
        "/reviews/subject/brief": None,
        "/reviews/both/brief": INVALID,
        "/reviews/neither/brief": INVALID,
    }


def test_a_report_holds_a_range_with_patch_ids_or_a_subject_and_no_mix_of_them(repo: Repo) -> None:
    subject = pinned(repo)
    report(repo, "range", range=COMMITS, patch_ids=[])
    report(repo, "subject", subject=subject)
    report(repo, "both", range=COMMITS, patch_ids=[], subject=subject)
    report(repo, "range-alone", range=COMMITS)
    report(repo, "range-subject", range=COMMITS, subject=subject)
    report(repo, "ids-subject", patch_ids=[], subject=subject)
    report(repo, "neither")
    assert checked(repo) == {
        "/reviews/range/report": None,
        "/reviews/subject/report": None,
        "/reviews/both/report": INVALID,
        "/reviews/range-alone/report": INVALID,
        "/reviews/range-subject/report": INVALID,
        "/reviews/ids-subject/report": INVALID,
        "/reviews/neither/report": INVALID,
    }


def test_a_subject_holds_a_hex_blob_and_nothing_else(repo: Repo) -> None:
    subject = pinned(repo)
    brief(repo, "whole", subject=subject)
    brief(repo, "short", subject=subject | {"blob": subject["blob"][:12]})
    brief(repo, "extra", subject=subject | {"range": COMMITS})
    brief(repo, "no-blob", subject={"record": subject["record"]})
    assert checked(repo) == {
        "/reviews/whole/brief": None,
        "/reviews/short/brief": INVALID,
        "/reviews/extra/brief": INVALID,
        "/reviews/no-blob/brief": INVALID,
    }


def test_a_run_gives_the_agent_the_pinned_record_and_its_references_and_the_report_names_the_subject(
    repo: Repo,
) -> None:
    first = report(repo, "first", range=COMMITS, patch_ids=[], brief="/reviews/plan/brief")
    subject = {"record": first, "blob": repo.git("hash-object", "reviews/first/report.json")}
    path = brief(repo, "plan", subject=subject)
    code, output = repo.call("run", "--brief", path)
    assert code == 0, output
    assert output == {"report": "/reviews/plan/report", "subject": subject, "verdict": "approve", "findings": 0}
    written = repo.get(output["report"])
    assert (written["subject"], written["brief"]) == (subject, path)
    assert "range" not in written
    assert "patch_ids" not in written
    prompt = repo.prompt()
    assert f"Record: {first} at {subject['blob']}" in prompt
    assert "- /brief: /reviews/plan/brief" in prompt
    assert (repo.root / "reviews/first/report.json").read_text() in prompt
    assert "## Diff" not in prompt
    assert repo.run("check").returncode == 0


def test_a_subject_that_moved_after_its_brief_is_refused_and_one_at_its_pinned_blob_runs(repo: Repo) -> None:
    subject = pinned(repo)
    path = brief(repo, "plan", subject=subject)
    repo.commit(PLAN, "# Plan\n\nShip it later.\n")
    code, refused = repo.call("run", "--brief", path)
    assert (code, refused["code"]) == (1, "review::subject::moved"), refused
    current = repo.git("hash-object", PLAN)
    fields = {name: refused["fields"][name] for name in ("record", "pinned", "current")}
    assert fields == {"record": subject["record"], "pinned": subject["blob"], "current": current}
    repinned = brief(repo, "plan-2", subject={"record": subject["record"], "blob": current})
    code, output = repo.call("run", "--brief", repinned)
    assert code == 0, output
    assert "Ship it later." in repo.prompt()


def test_a_subject_with_nothing_there_is_refused_and_one_that_is_there_runs(repo: Repo) -> None:
    subject = pinned(repo)
    gone = brief(repo, "gone", subject=subject | {"record": "/notes/missing.md"})
    code, refused = repo.call("run", "--brief", gone)
    assert (code, refused["code"]) == (1, "review::subject::not_found"), refused
    assert repo.call("run", "--brief", brief(repo, "there", subject=subject))[0] == 0


def test_a_subject_report_never_covers_a_commit_and_a_range_report_of_the_same_commit_does(repo: Repo) -> None:
    base = repo.git("rev-parse", "HEAD")
    repo.git("checkout", "-q", "-b", "change")
    subject = pinned(repo)
    head = repo.git("rev-parse", "HEAD")
    report(repo, "plan", subject=subject)
    code, refused = repo.call("covered", "--base", "main")
    assert (code, refused["code"]) == (1, "review::head::uncovered"), refused
    assert (refused["fields"]["reports"], refused["fields"]["uncovered"]) == (0, [head])
    path = report(repo, "code", range=f"{base}..{head}", patch_ids=[])
    code, output = repo.call("covered", "--base", "main")
    assert (code, output["reports"]) == (0, [path]), output


def test_a_posted_subject_report_names_the_record_and_its_blob_where_a_range_report_names_its_commits() -> None:
    subject = {"record": "/changes/x/plan.md", "blob": "c" * 40}
    base: dict[str, Any] = {"$type": "review/report/v1", "findings": [], "verdict": "approve", "by": "p"}
    of_subject = comment(
        "/reviews/r/report", "https://github.com/o/r/pull/7", cast("ReportRecord", base | {"subject": subject})
    )
    of_range = comment(
        "/reviews/r/report",
        "https://github.com/o/r/pull/7",
        cast("ReportRecord", base | {"range": COMMITS, "patch_ids": []}),
    )
    assert f"Record `/changes/x/plan.md` at `{'c' * 40}`." in of_subject
    assert f"Commits `{COMMITS}`." in of_range
