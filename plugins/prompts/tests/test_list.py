"""The `list` capability through the real `board`: order, scope by remote, code mounts and degraded files."""

import json

from conftest import Board, git, mark, mount, prompt


def titles(output: dict[str, object]) -> list[tuple[str, str]]:
    prompts = output["prompts"]
    assert isinstance(prompts, list)
    return [(item["repo"], item["title"]) for item in prompts]  # pyright: ignore[reportUnknownVariableType]


def record(covers: list[str], *, personal: bool = False) -> str:
    """A `notes/repo/v1` record covering `covers`."""
    return json.dumps({"$type": "notes/repo/v1", "covers": covers, "personal": personal})


def test_list_shows_each_prompt_as_written_by_repo_then_title(board: Board) -> None:
    board.write("prompts/b.md", prompt("B", inputs={"pr": "The number."}, body="{pr}"))
    board.write("projects/x/prompts/a.md", prompt("A", runs_in={"directory": "~/Code"}))
    mount(board, "notes", {"prompts/c.md": prompt("C")})
    code, output = board.call("list")
    assert code == 0, output
    assert titles(output) == [("/", "A"), ("/", "B"), ("/repos/notes", "C")]
    assert output["prompts"][1] == {
        "repo": "/",
        "path": "/prompts/b.md",
        "title": "B",
        "purpose": "A clean-context review of one pull request.",
        "runs_in": {"remote": "example-org/widgets"},
        "inputs": {"pr": "The number."},
    }
    assert (output["total"], output["degraded"], output["repos"]) == (3, 0, ["/", "/repos/notes"])


def test_a_prompt_record_under_prompts_that_is_not_valid_is_counted_as_degraded(board: Board) -> None:
    board.write("prompts/good.md", prompt("Good"))
    board.write("prompts/linked.md", prompt("Linked", body="[plan](/plan.md)"))
    board.write("prompts/invalid.md", prompt("Invalid", purpose=None))
    board.write("notes/elsewhere.md", prompt("Elsewhere"))
    code, output = board.call("list")
    assert code == 0, output
    assert (titles(output), output["degraded"]) == ([("/", "Good"), ("/", "Linked")], 1)


def test_a_file_under_prompts_whose_frontmatter_does_not_parse_or_names_an_unknown_type_is_counted(
    board: Board,
) -> None:
    board.write("prompts/good.md", prompt("Good"))
    board.write("prompts/unparsed.md", '---\n$type: "prompts/prompt/v1\ntitle: [Broken\n---\n')
    board.write("prompts/misspelt.md", prompt("Misspelt", **{"$type": "prompts/promt/v1"}))
    board.write("prompts/later.md", prompt("Later", **{"$type": "prompts/prompt/v2"}))
    code, output = board.call("list")
    assert code == 0, output
    assert (titles(output), output["degraded"]) == ([("/", "Good")], 3)


def test_a_markdown_file_under_prompts_that_is_no_prompt_record_is_not_counted(board: Board) -> None:
    board.write("prompts/good.md", prompt("Good"))
    board.write("prompts/untyped.md", "# A note\n")
    board.write("prompts/linking.md", "# A note\n\n[gone](/gone.md)\n")
    board.write("evals/x/evidence/prompts/scorer.md", "# A scorer's prompt, kept as evidence\n")
    code, output = board.call("list")
    assert code == 0, output
    assert (titles(output), output["degraded"]) == ([("/", "Good")], 0)


def test_a_mount_the_board_does_not_mark_is_skipped_and_its_files_are_not_counted(board: Board) -> None:
    code_files = {"prompts/a.md": prompt("Code"), "prompts/broken.md": "# Not a prompt\n", "daily/x.md": "x"}
    mount(board, "widgets", code_files, marked=False)
    mount(board, "notes", {"prompts/a.md": prompt("Notes")})
    output = board.call("list")[1]
    assert (titles(output), output["repos"]) == ([("/repos/notes", "Notes")], ["/", "/repos/notes"])
    assert output["degraded"] == 0


def test_list_refuses_a_list_of_marked_repos_naming_a_mount_the_board_lacks_and_answers_once_it_is_mounted(
    board: Board,
) -> None:
    mount(board, "notes", {"prompts/a.md": prompt("Notes")})
    mark(board, "/repos/gone")
    code, refused = board.call("list")
    assert (code, refused["code"]) == (1, "prompts::repos::invalid"), refused
    mount(board, "gone", {"README.md": "x"}, marked=False)
    code, output = board.call("list")
    assert (code, output["repos"]) == (0, ["/", "/repos/gone", "/repos/notes"]), output


def test_the_remote_chooses_its_covering_repo_with_the_personal_repo_and_repo_names_one(board: Board) -> None:
    board.write("prompts/own.md", prompt("Own"))
    mount(board, "personal", {"prompts/a.md": prompt("Personal"), "notes-repo.json": record([], personal=True)})
    mount(board, "work", {"prompts/a.md": prompt("Work"), "notes-repo.json": record(["example-org/*"])})
    mount(board, "other", {"prompts/a.md": prompt("Other"), "notes-repo.json": record(["acme/*"])})
    checkout = board.root.parent / "checkout"
    git(board.root.parent, "init", "-q", str(checkout))
    git(checkout, "remote", "add", "origin", "git@github.com:example-org/home.git")
    output = board.call("list", cwd=checkout)[1]
    assert titles(output) == [("/", "Own"), ("/repos/personal", "Personal"), ("/repos/work", "Work")]
    assert titles(board.call("list", "--remote", "acme/x")[1]) == [
        ("/", "Own"),
        ("/repos/other", "Other"),
        ("/repos/personal", "Personal"),
    ]
    assert titles(board.call("list", "--repo", "/repos/work")[1]) == [("/repos/work", "Work")]
    code, refused = board.call("list", "--repo", "/repos/elsewhere")
    assert (code, refused["code"]) == (1, "prompts::repo::not_found"), refused


def test_list_shows_at_most_100_prompts_and_counts_them_all(board: Board) -> None:
    for number in range(101):
        board.write(f"prompts/p{number:03}.md", prompt(f"P{number:03}"))
    output = board.call("list")[1]
    assert (len(output["prompts"]), output["total"], output["prompts"][-1]["title"]) == (100, 101, "P099")
