"""`review.covered` passes when committed reports cover each commit of a pull request, by range or by patch id."""

import json
from pathlib import Path

from conftest import Repo, library

from board_review.git import Repository


def reviewed(repo: Repo) -> tuple[str, str, str]:
    """Two commits on a branch, reviewed: the base, the head and the report, which `board put` commits."""
    base = repo.git("rev-parse", "HEAD")
    repo.git("checkout", "-q", "-b", "change")
    repo.commit("src/a.txt", "one\n")
    head = repo.commit("src/b.txt", "two\n")
    code, output = repo.call("run", "--brief", repo.brief("change", f"{base}..{head}"))
    assert code == 0, output
    assert repo.git("status", "--porcelain") == ""
    return base, head, output["report"]


def report(repo: Repo, name: str, base: str, head: str, **fields: object) -> str:
    """Commit a person's report at /reviews/<name>/report for `base..head`, approving unless `fields` differ."""
    record: dict[str, object] = {
        "$type": "review/report/v1",
        "range": f"{base}..{head}",
        "patch_ids": [],
        "findings": [],
        "verdict": "approve",
        "by": "a person",
    }
    repo.commit(f"reviews/{name}/report.json", json.dumps(record | fields))
    return f"/reviews/{name}/report"


def uncovered(repo: Repo, *flags: str) -> dict[str, object]:
    """The fields of a `review::head::uncovered` refusal of `covered --base main <flags>`."""
    code, document = repo.call("covered", "--base", "main", *flags)
    assert (code, document["code"]) == (1, "review::head::uncovered"), document
    return document["fields"]


def test_a_report_whose_range_ends_at_the_head_covers_it_past_the_commits_that_record_it(repo: Repo) -> None:
    base, head, path = reviewed(repo)
    code, output = repo.call("covered", "--base", "main")
    assert code == 0, output
    # The brief's commit and the report's commit only add review records.
    tip = repo.git("rev-parse", "HEAD")
    assert output == {"head": tip, "merge_base": base, "skipped": 2, "commits": 2, "reports": [path]}
    code, output = repo.call("covered", "--base", "main", "--head", head)
    assert (code, output["skipped"], output["reports"]) == (0, 0, [path])


def test_a_head_with_a_change_after_the_report_is_refused_and_the_reviewed_head_admitted(repo: Repo) -> None:
    _, head, _ = reviewed(repo)
    later = repo.commit("src/c.txt", "three\n")
    fields = uncovered(repo)
    assert (fields["head"], fields["skipped"], fields["reports"], fields["uncovered"]) == (later, 2, 1, [later])
    assert repo.call("covered", "--base", "main", "--head", head)[0] == 0


def replayed(repo: Repo, head: str, name: str, b: str) -> None:
    """Branch `name` from main, remake the reviewed commits with `src/b.txt` as `b`, and copy the review commits."""
    records = repo.git("rev-list", "--reverse", f"{head}..change").split()
    repo.git("checkout", "-q", "-b", name, "main")
    repo.commit("src/a.txt", "one\n")
    repo.commit("src/b.txt", b)
    repo.git("cherry-pick", *records)


def test_a_rebased_branch_is_covered_by_patch_id_and_a_changed_one_is_not(repo: Repo) -> None:
    _, head, path = reviewed(repo)
    repo.git("checkout", "-q", "main")
    repo.commit("other.txt", "unrelated\n")
    replayed(repo, head, "edited", "two, edited\n")
    fields = uncovered(repo)
    # The report lists a patch id the edited branch lacks, so it covers neither commit by patch id.
    assert (fields["reports"], fields["uncovered"]) == (
        1,
        [repo.git("rev-parse", "HEAD~3"), repo.git("rev-parse", "HEAD~2")],
    )
    replayed(repo, head, "same", "two\n")
    code, output = repo.call("covered", "--base", "main")
    assert (code, output["reports"]) == (0, [path]), output
    assert output["head"] != head


def test_a_rebased_commit_whose_indentation_changed_is_refused_and_an_unchanged_one_admitted(repo: Repo) -> None:
    _, head, _ = reviewed(repo)
    repo.git("checkout", "-q", "main")
    repo.commit("other.txt", "unrelated\n")
    replayed(repo, head, "reindented", "    two\n")
    uncovered(repo)
    replayed(repo, head, "same", "two\n")
    assert repo.call("covered", "--base", "main")[0] == 0


def test_a_merge_is_refused_by_patch_ids_and_admitted_by_a_range_that_contains_it(repo: Repo) -> None:
    base = repo.git("rev-parse", "HEAD")
    repo.git("checkout", "-q", "-b", "change")
    repo.commit("src/a.txt", "one\n")
    head = repo.commit("src/b.txt", "two\n")
    commits = repo.git("rev-list", f"{base}..{head}").split()
    patch_ids = list(Repository(str(repo.root)).patch_ids(commits).values())
    repo.git("checkout", "-q", "main")
    repo.commit("other.txt", "unrelated\n")
    repo.git("checkout", "-q", "change")
    repo.git("merge", "-q", "--no-ff", "-m", "merge main", "main")
    merge = repo.git("rev-parse", "HEAD")
    branch = report(repo, "branch", base, base, patch_ids=patch_ids)
    assert uncovered(repo)["uncovered"] == [merge]
    merged = report(repo, "merged", head, merge)
    code, output = repo.call("covered", "--base", "main")
    assert (code, output["reports"]) == (0, [branch, merged])


def test_an_empty_commit_is_refused_by_patch_ids_and_admitted_by_a_range_that_contains_it(repo: Repo) -> None:
    base = repo.git("rev-parse", "HEAD")
    repo.git("checkout", "-q", "-b", "change")
    repo.commit("src/a.txt", "one\n")
    repo.git("commit", "-q", "--allow-empty", "-m", "nothing")
    head = repo.commit("src/b.txt", "two\n")
    code, output = repo.call("run", "--brief", repo.brief("change", f"{base}..{head}"))
    assert code == 0, output
    assert len(repo.get(output["report"])["patch_ids"]) == len(["src/a.txt", "src/b.txt"])
    repo.git("checkout", "-q", "main")
    repo.commit("other.txt", "unrelated\n")
    repo.git("checkout", "-q", "change")
    repo.git("rebase", "-q", "--empty=keep", "main")
    empty = repo.git("rev-list", "-n1", "--grep=nothing", "HEAD")
    assert uncovered(repo)["uncovered"] == [empty]
    report(repo, "empty", repo.git("rev-parse", f"{empty}~1"), empty)
    code, output = repo.call("covered", "--base", "main")
    assert (code, output["reports"]) == (0, [output["reports"][0], "/reviews/empty/report"]), output


def test_reports_together_cover_a_range_none_covers_alone(repo: Repo) -> None:
    base = repo.git("rev-parse", "HEAD")
    repo.git("checkout", "-q", "-b", "change")
    first = repo.commit("src/a.txt", "one\n")
    head = repo.commit("src/b.txt", "two\n")
    tip = report(repo, "tip", first, head)
    assert uncovered(repo, "--head", head)["uncovered"] == [first]
    start = report(repo, "start", base, first)
    code, output = repo.call("covered", "--base", "main", "--head", head)
    assert (code, output["reports"]) == (0, [start, tip])


def test_a_base_sharing_no_commit_with_the_head_is_refused_and_a_related_one_admitted(repo: Repo) -> None:
    reviewed(repo)
    empty_tree = repo.git("hash-object", "-t", "tree", "/dev/null")
    orphan = repo.git("commit-tree", empty_tree, "-m", "orphan")
    code, document = repo.call("covered", "--base", orphan)
    assert (code, document["code"]) == (1, "review::range::unrelated")
    assert "fetch-depth: 0" in document["message"]
    assert repo.call("covered", "--base", "main")[0] == 0


def test_a_report_reached_through_a_mount_is_ignored_and_the_same_one_in_the_own_tree_admitted(
    repo: Repo, tmp_path: Path
) -> None:
    base = repo.git("rev-parse", "HEAD")
    repo.git("checkout", "-q", "-b", "change")
    head = repo.commit("src/a.txt", "one\n")
    record: dict[str, object] = {"$type": "review/report/v1", "range": f"{base}..{head}", "patch_ids": []}
    text = json.dumps(record | {"findings": [], "verdict": "approve", "by": "a person"})
    repo.commit("shared.json", json.dumps(library(tmp_path / "shared", {"report.json": text})))
    assert repo.run("get", "/shared/report").returncode == 0
    assert uncovered(repo, "--head", head)["reports"] == 0
    repo.commit("reviews/own/report.json", text)
    code, output = repo.call("covered", "--base", "main", "--head", head)
    assert (code, output["reports"]) == (0, ["/reviews/own/report"])


def test_a_head_git_cannot_resolve_is_refused_and_one_it_can_is_checked(repo: Repo) -> None:
    code, document = repo.call("covered", "--base", "main", "--head", "no-such-branch")
    assert (code, document["code"]) == (1, "review::range::unknown")
    code, document = repo.call("covered", "--base", "no-such-branch", "--head", "main")
    assert (code, document["code"]) == (1, "review::range::unknown")
    repo.git("checkout", "-q", "-b", "change")
    repo.commit("src/a.txt", "one\n")
    uncovered(repo, "--head", "HEAD")


def test_a_call_without_a_base_is_refused_and_one_with_it_is_checked(repo: Repo) -> None:
    reviewed(repo)
    ran = repo.run("--json", "call", "review.covered")
    assert (ran.returncode, json.loads(ran.stdout)["code"]) == (1, "board::plugin::bad_input")
    assert repo.call("covered", "--base", "main")[0] == 0


def test_a_report_of_only_the_tip_is_refused_and_one_of_every_commit_since_the_base_admitted(repo: Repo) -> None:
    base = repo.git("rev-parse", "HEAD")
    repo.git("checkout", "-q", "-b", "change")
    first = repo.commit("src/a.txt", "one\n")
    head = repo.commit("src/b.txt", "two\n")
    report(repo, "tip", first, head)
    assert uncovered(repo)["merge_base"] == base
    whole = report(repo, "whole", base, head)
    code, output = repo.call("covered", "--base", "main")
    assert (code, output["reports"][-1]) == (0, whole)


def test_a_report_asking_for_changes_is_refused_and_one_approving_admitted(repo: Repo) -> None:
    base = repo.git("rev-parse", "HEAD")
    repo.git("checkout", "-q", "-b", "change")
    head = repo.commit("src/a.txt", "one\n")
    report(repo, "first", base, head, verdict="changes")
    uncovered(repo)
    second = report(repo, "second", base, head, verdict="approve")
    code, output = repo.call("covered", "--base", "main")
    assert (code, output["reports"]) == (0, [second])


def test_a_commit_adding_a_file_that_is_not_a_record_is_not_skipped_and_one_adding_only_records_is(
    repo: Repo,
) -> None:
    _, head, _ = reviewed(repo)
    note = repo.commit("reviews/notes.md", "A note.\n")
    assert uncovered(repo)["uncovered"] == [note]
    repo.git("rm", "-q", "reviews/notes.md")
    repo.git("commit", "-qm", "drop the note")
    uncovered(repo)
    repo.git("reset", "-q", "--hard", "HEAD~2")
    repo.brief("later", f"{head}..{head}")
    code, output = repo.call("covered", "--base", "main")
    assert (code, output["skipped"]) == (0, 3)


def test_a_commit_changing_a_review_record_is_not_skipped_and_one_adding_a_record_is(repo: Repo) -> None:
    _, head, path = reviewed(repo)
    brief = repo.get("/reviews/change/brief") | {"focus": "Something else."}
    changed = repo.commit("reviews/change/brief.json", json.dumps(brief))
    assert uncovered(repo)["uncovered"] == [changed]
    repo.git("reset", "-q", "--hard", "HEAD~1")
    repo.brief("later", f"{head}..{head}")
    code, output = repo.call("covered", "--base", "main")
    assert (code, output["skipped"], output["reports"]) == (0, 3, [path])


def test_a_commit_adding_a_record_of_another_type_or_at_another_path_is_not_skipped_and_a_brief_at_its_path_is(
    repo: Repo,
) -> None:
    _, head, _ = reviewed(repo)
    brief = {"$type": "review/brief/v1", "range": f"{head}..{head}", "focus": "Later."}
    other_type = repo.commit("reviews/later/brief.json", json.dumps(brief | {"$type": "review/report/v1"}))
    assert uncovered(repo)["uncovered"] == [other_type]
    repo.git("reset", "-q", "--hard", "HEAD~1")
    other_path = repo.commit("reviews/later.json", json.dumps(brief))
    assert uncovered(repo)["uncovered"] == [other_path]
    repo.git("reset", "-q", "--hard", "HEAD~1")
    repo.commit("reviews/later/brief.json", json.dumps(brief))
    code, output = repo.call("covered", "--base", "main")
    assert (code, output["skipped"]) == (0, 3)


def test_a_branch_that_drops_a_reviewed_commit_is_refused_and_one_that_keeps_both_admitted(repo: Repo) -> None:
    base = repo.git("rev-parse", "HEAD")
    repo.git("checkout", "-q", "-b", "change")
    added = repo.commit("src/a.txt", "one\nextra\n")
    undone = repo.commit("src/a.txt", "one\n")
    ids = Repository(str(repo.root)).patch_ids([added, undone])
    path = report(repo, "both", base, base, patch_ids=[ids[added], ids[undone]])
    record = repo.git("rev-parse", "HEAD")
    repo.git("checkout", "-q", "main")
    repo.commit("other.txt", "unrelated\n")
    repo.git("checkout", "-q", "-b", "dropped")
    kept = repo.commit("src/a.txt", "one\nextra\n")
    repo.git("cherry-pick", record)
    assert uncovered(repo)["uncovered"] == [kept]
    repo.git("checkout", "-q", "-b", "both", "main")
    repo.commit("src/a.txt", "one\nextra\n")
    repo.commit("src/a.txt", "one\n")
    repo.git("cherry-pick", record)
    code, output = repo.call("covered", "--base", "main")
    assert (code, output["reports"]) == (0, [path]), output


def test_a_report_changed_on_disk_is_ignored_and_the_committed_one_counts(repo: Repo) -> None:
    base = repo.git("rev-parse", "HEAD")
    repo.git("checkout", "-q", "-b", "change")
    head = repo.commit("src/a.txt", "one\n")
    path = report(repo, "manual", base, head)
    file = repo.root / "reviews/manual/report.json"
    committed = file.read_text()
    repo.write("reviews/manual/report.json", json.dumps(json.loads(committed) | {"by": "someone else"}))
    assert uncovered(repo)["uncovered"] == [head]
    repo.write("reviews/manual/report.json", committed)
    code, output = repo.call("covered", "--base", "main")
    assert code == 0, output
    assert output["reports"] == [path]
