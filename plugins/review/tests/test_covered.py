"""`review.covered` passes when committed reports cover each commit of a pull request, by range or by patch id."""

import ast
import json
import os
import subprocess
from pathlib import Path

from conftest import ISOLATED, PLUGIN, Repo, library

from board_review import covered
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


def test_a_commit_setting_only_a_reports_source_is_skipped_and_one_also_changing_its_findings_is_not(
    repo: Repo,
) -> None:
    _, _, path = reviewed(repo)
    source = {"source": "https://github.com/o/r/pull/7#issuecomment-1"}
    finding = {"severity": "low", "text": "Later."}
    changed = repo.commit("reviews/change/report.json", json.dumps(repo.get(path) | source | {"findings": [finding]}))
    assert uncovered(repo)["uncovered"] == [changed]
    repo.git("reset", "-q", "--hard", "HEAD~1")
    repo.commit("reviews/change/report.json", json.dumps(repo.get(path) | source))
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


def mounted(repo: Repo, tmp_path: Path) -> tuple[str, str]:
    """Mount a project at /project whose second commit no report covers yet, with changes enabled: its base and head.

    The project's commits are only in its own clone, so `covered` finds them only through the mount.
    """
    (repo.data / "board" / "plugins" / "changes").symlink_to(PLUGIN.parent / "changes")
    enabled = repo.run("plugin", "enable", "changes")
    assert enabled.returncode == 0, enabled.stderr
    pointer = library(tmp_path / "project", {"README.md": "A project.\n"})
    repo.commit("project.json", json.dumps(pointer))
    return str(pointer["commit"]), project_commit(tmp_path, "src.txt")


def project_commit(tmp_path: Path, name: str, text: str | None = None) -> str:
    """Commit a new file `name`, holding `text` or else its name, to the mounted project's clone; return the commit."""
    clone = tmp_path / "project"
    (clone / name).write_text(f"{name}\n" if text is None else text)
    git = ["git", "-c", "user.name=t", "-c", "user.email=t@t"]
    for arguments in (["add", "."], ["commit", "-qm", f"add {name}"]):
        subprocess.run([*git, *arguments], cwd=clone, env=os.environ | ISOLATED, check=True)
    return subprocess.run(
        ["git", "rev-parse", "HEAD"], cwd=clone, env=os.environ | ISOLATED, check=True, capture_output=True, text=True
    ).stdout.strip()


def change(repo: Repo, head: str | None, records: list[str], mount: str = "/project") -> str:
    """Write the change /changes/work on `mount` at `head`, or with no head, attaching `records`; return its path."""
    record: dict[str, object] = {"$type": "changes/change/v1", "title": "Work", "mount": mount, "phase": "review"}
    at = {} if head is None else {"head": head}
    repo.write("changes/work.json", json.dumps(record | at | {"records": records}))
    return "/changes/work"


def attached_report(  # noqa: PLR0913 - each field a test varies
    repo: Repo,
    base: str,
    head: str,
    severity: str | None = None,
    *,
    verdict: str = "approve",
    patch_ids: list[str] | None = None,
    name: str = "one",
) -> str:
    """Write, without committing, a person's report of `base..head` beside the change, and return its path.

    It approves with no findings and no patch ids unless `verdict`, `severity` or `patch_ids` say otherwise.
    """
    record: dict[str, object] = {
        "$type": "review/report/v1",
        "range": f"{base}..{head}",
        "patch_ids": patch_ids or [],
        "findings": [] if severity is None else [{"severity": severity, "text": "A finding."}],
    }
    repo.write(f"changes/work/reviews/{name}.json", json.dumps(record | {"verdict": verdict, "by": "a person"}))
    return f"/changes/work/reviews/{name}"


def test_a_report_the_change_does_not_attach_is_refused_and_an_attached_one_covers_the_mounts_commits(
    repo: Repo, tmp_path: Path
) -> None:
    base, head = mounted(repo, tmp_path)
    report_path = attached_report(repo, base, head)
    path = change(repo, head, [])
    code, document = repo.call("covered", "--base", base, "--change", path)
    assert (code, document["code"]) == (1, "review::head::uncovered"), document
    assert document["fields"]["uncovered"] == [head]
    assert "changes.attach --path /changes/work" in document["message"]
    change(repo, head, [report_path])
    code, output = repo.call("covered", "--base", base, "--change", path)
    assert code == 0, output
    assert output == {"head": head, "merge_base": base, "skipped": 0, "commits": 1, "reports": [report_path]}


def test_a_change_that_is_not_there_is_refused_and_one_that_is_admitted(repo: Repo, tmp_path: Path) -> None:
    base, head = mounted(repo, tmp_path)
    path = change(repo, head, [attached_report(repo, base, head)])
    code, document = repo.call("covered", "--base", base, "--change", "/changes/other")
    assert (code, document["code"]) == (1, "review::change::not_found"), document
    assert repo.call("covered", "--base", base, "--change", path)[0] == 0


def test_a_change_whose_mount_names_no_clone_is_refused_and_one_on_a_mount_admitted(repo: Repo, tmp_path: Path) -> None:
    base, head = mounted(repo, tmp_path)
    report_path = attached_report(repo, base, head)
    for mount in ("/nowhere", "/changes"):
        path = change(repo, head, [report_path], mount=mount)
        code, document = repo.call("covered", "--base", base, "--change", path)
        assert (code, document["code"]) == (1, "review::mount::not_found"), (mount, document)
    change(repo, head, [report_path])
    assert repo.call("covered", "--base", base, "--change", path)[0] == 0


def test_a_change_with_no_head_is_refused_and_one_with_a_head_admitted(repo: Repo, tmp_path: Path) -> None:
    base, head = mounted(repo, tmp_path)
    report_path = attached_report(repo, base, head)
    path = change(repo, None, [report_path])
    code, document = repo.call("covered", "--base", base, "--change", path)
    assert (code, document["code"]) == (1, "review::change::no_head"), document
    assert "changes.attach --path /changes/work --head" in document["message"]
    change(repo, head, [report_path])
    assert repo.call("covered", "--base", base, "--change", path)[0] == 0


def test_the_change_head_past_the_report_is_refused_and_a_head_flag_naming_the_reviewed_one_admitted(
    repo: Repo, tmp_path: Path
) -> None:
    base, head = mounted(repo, tmp_path)
    later = project_commit(tmp_path, "later.txt")
    path = change(repo, later, [attached_report(repo, base, head)])
    code, document = repo.call("covered", "--base", base, "--change", path)
    assert (code, document["code"], document["fields"]["uncovered"]) == (1, "review::head::uncovered", [later])
    code, output = repo.call("covered", "--base", base, "--change", path, "--head", head)
    assert (code, output["head"]) == (0, head), output


def test_an_attached_report_with_a_high_finding_is_refused_and_one_with_a_low_finding_admitted(
    repo: Repo, tmp_path: Path
) -> None:
    base, head = mounted(repo, tmp_path)
    path = change(repo, head, [attached_report(repo, base, head, severity="high")])
    code, document = repo.call("covered", "--base", base, "--change", path)
    assert (code, document["code"], document["fields"]["reports"]) == (1, "review::head::uncovered", 1)
    assert "of 1 reports, 0 ask for changes and 1 have a critical or high finding" in document["message"]
    attached_report(repo, base, head, severity="low")
    assert repo.call("covered", "--base", base, "--change", path)[0] == 0


def test_a_project_commit_adding_only_a_report_is_checked_with_a_change_and_covered_once_reviewed(
    repo: Repo, tmp_path: Path
) -> None:
    base, head = mounted(repo, tmp_path)
    (tmp_path / "project" / "reviews" / "x").mkdir(parents=True)
    record: dict[str, object] = {"$type": "review/report/v1", "range": f"{base}..{head}", "patch_ids": []}
    text = json.dumps(record | {"findings": [], "verdict": "approve", "by": "a person"})
    added = project_commit(tmp_path, "reviews/x/report.json", text)
    assert Repository(str(tmp_path / "project")).review_only(added)
    path = change(repo, added, [attached_report(repo, base, head)])
    code, document = repo.call("covered", "--base", base, "--change", path)
    assert (code, document["code"], document["fields"]["uncovered"]) == (1, "review::head::uncovered", [added])
    attached_report(repo, base, added)
    code, output = repo.call("covered", "--base", base, "--change", path)
    assert (code, output["skipped"], output["commits"]) == (0, 0, 2), output


def constants(path: Path) -> dict[str, object]:
    """Every module-level constant `path` assigns a literal, by name."""
    tree = ast.parse(path.read_text())
    return {
        target.id: ast.literal_eval(node.value)
        for node in tree.body
        if isinstance(node, ast.Assign) and isinstance(node.value, ast.Constant | ast.Tuple)
        for target in node.targets
        if isinstance(target, ast.Name)
    }


def test_the_changes_plugins_constants_that_covered_copies_agree_with_its_own() -> None:
    changes = PLUGIN.parent / "changes" / "src" / "board_changes"
    assert constants(changes / "gates.py")["BLOCKING"] == covered.BLOCKING
    assert constants(changes / "change.py")["CHANGE_TYPE"] == covered.CHANGE_TYPE


def test_a_mount_named_by_a_file_below_its_root_is_refused_and_the_root_admitted(repo: Repo, tmp_path: Path) -> None:
    base, head = mounted(repo, tmp_path)
    report_path = attached_report(repo, base, head)
    path = change(repo, head, [report_path], mount="/project/README.md")
    code, document = repo.call("covered", "--base", base, "--change", path)
    assert (code, document["code"]) == (1, "review::mount::not_found"), document
    change(repo, head, [report_path])
    assert repo.call("covered", "--base", base, "--change", path)[0] == 0


def test_a_report_attached_twice_is_counted_once_when_refused_and_listed_once_when_admitted(
    repo: Repo, tmp_path: Path
) -> None:
    base, head = mounted(repo, tmp_path)
    later = project_commit(tmp_path, "later.txt")
    report_path = attached_report(repo, base, head)
    path = change(repo, later, [report_path, report_path])
    code, document = repo.call("covered", "--base", base, "--change", path)
    assert (code, document["code"], document["fields"]["reports"]) == (1, "review::head::uncovered", 1), document
    code, output = repo.call("covered", "--base", base, "--change", path, "--head", head)
    assert (code, output["reports"]) == (0, [report_path]), output


def test_a_report_asking_for_changes_with_a_high_finding_is_counted_once_and_an_approving_low_one_admitted(
    repo: Repo, tmp_path: Path
) -> None:
    base, head = mounted(repo, tmp_path)
    path = change(repo, head, [attached_report(repo, base, head, "high", verdict="changes")])
    code, document = repo.call("covered", "--base", base, "--change", path)
    assert (code, document["code"]) == (1, "review::head::uncovered"), document
    assert "of 1 reports, 1 ask for changes and 0 have a critical or high finding" in document["message"]
    attached_report(repo, base, head, "low")
    assert repo.call("covered", "--base", base, "--change", path)[0] == 0


def test_an_attached_report_asking_for_changes_is_refused_and_an_approving_one_admitted(
    repo: Repo, tmp_path: Path
) -> None:
    base, head = mounted(repo, tmp_path)
    path = change(repo, head, [attached_report(repo, base, head, verdict="changes")])
    code, document = repo.call("covered", "--base", base, "--change", path)
    assert (code, document["code"]) == (1, "review::head::uncovered"), document
    attached_report(repo, base, head, verdict="approve")
    assert repo.call("covered", "--base", base, "--change", path)[0] == 0


def test_an_attached_report_without_the_heads_patch_id_is_refused_and_one_listing_it_admitted(
    repo: Repo, tmp_path: Path
) -> None:
    base, head = mounted(repo, tmp_path)
    [patch_id] = Repository(str(tmp_path / "project")).patch_ids([head]).values()
    path = change(repo, head, [attached_report(repo, base, base)])
    code, document = repo.call("covered", "--base", base, "--change", path)
    assert (code, document["code"], document["fields"]["uncovered"]) == (1, "review::head::uncovered", [head])
    attached_report(repo, base, base, patch_ids=[patch_id])
    code, output = repo.call("covered", "--base", base, "--change", path)
    assert (code, output["reports"]) == (0, ["/changes/work/reviews/one"]), output


def test_an_attached_report_that_is_not_there_is_refused_and_admitted_once_written(repo: Repo, tmp_path: Path) -> None:
    base, head = mounted(repo, tmp_path)
    missing = "/changes/work/reviews/later"
    path = change(repo, head, [missing])
    code, document = repo.call("covered", "--base", base, "--change", path)
    assert (code, document["code"], document["fields"]["reports"]) == (1, "review::head::uncovered", 0), document
    attached_report(repo, base, head, name="later")
    code, output = repo.call("covered", "--base", base, "--change", path)
    assert (code, output["reports"]) == (0, [missing]), output
