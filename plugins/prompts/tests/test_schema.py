"""The `prompts/prompt/v1` schema the plugin ships: what `board check` admits and refuses in a prompt."""

import json

from conftest import Board, mount, prompt


def checked(board: Board) -> dict[str, str | None]:
    """Each prompt's code from `board check`, by its path; `None` when it is valid."""
    report = json.loads(board.run("--json", "check", "--all").stdout)
    return {entry["logical"]: entry.get("code") for entry in report["data"] if entry["logical"].startswith("/prompts/")}


def test_a_prompt_with_its_required_fields_and_inputs_is_admitted_and_one_without_purpose_is_refused(
    board: Board,
) -> None:
    board.write("prompts/full.md", prompt(inputs={"repo": "The repository.", "pr_2": "The number."}))
    board.write("prompts/bare.md", prompt(runs_in={"directory": "~/Code/example-org/home"}))
    board.write("prompts/purposeless.md", prompt(purpose=None))
    assert checked(board) == {
        "/prompts/full.md": None,
        "/prompts/bare.md": None,
        "/prompts/purposeless.md": "board::record::invalid",
    }


def test_a_misspelt_field_is_refused(board: Board) -> None:
    board.write("prompts/spelt.md", prompt(inputs={"pr": "The number."}))
    board.write("prompts/misspelt.md", prompt(input={"pr": "The number."}))
    assert checked(board) == {"/prompts/spelt.md": None, "/prompts/misspelt.md": "board::record::invalid"}


def test_runs_in_holds_exactly_one_of_remote_and_directory(board: Board) -> None:
    board.write("prompts/remote.md", prompt(runs_in={"remote": "example-org/widgets"}))
    board.write("prompts/directory.md", prompt(runs_in={"directory": "/srv/{name}"}, inputs={"name": "A name."}))
    board.write("prompts/both.md", prompt(runs_in={"remote": "example-org/widgets", "directory": "/srv"}))
    board.write("prompts/neither.md", prompt(runs_in={}))
    board.write("prompts/other.md", prompt(runs_in={"host": "x"}))
    assert checked(board) == {
        "/prompts/remote.md": None,
        "/prompts/directory.md": None,
        "/prompts/both.md": "board::record::invalid",
        "/prompts/neither.md": "board::record::invalid",
        "/prompts/other.md": "board::record::invalid",
    }


def test_a_runs_in_naming_its_remote_repo_is_refused_and_its_twin_naming_it_remote_is_admitted(board: Board) -> None:
    board.write("prompts/remote.md", prompt(runs_in={"remote": "{repo}"}, inputs={"repo": "The repository."}))
    board.write("prompts/repo.md", prompt(runs_in={"repo": "{repo}"}, inputs={"repo": "The repository."}))
    assert checked(board) == {"/prompts/remote.md": None, "/prompts/repo.md": "board::record::invalid"}


def test_a_remote_is_owner_slash_name_or_an_input_and_a_directory_is_absolute_or_under_home(board: Board) -> None:
    board.write("prompts/owner.md", prompt(runs_in={"remote": "example-org/{name}"}, inputs={"name": "A name."}))
    board.write("prompts/bare-name.md", prompt(runs_in={"remote": "widgets"}))
    board.write("prompts/home.md", prompt(runs_in={"directory": "~/Code"}))
    board.write("prompts/relative.md", prompt(runs_in={"directory": "Code/home"}))
    assert checked(board) == {
        "/prompts/owner.md": None,
        "/prompts/bare-name.md": "board::record::invalid",
        "/prompts/home.md": None,
        "/prompts/relative.md": "board::record::invalid",
    }


def test_an_input_name_is_lowercase_and_starts_with_a_letter(board: Board) -> None:
    board.write("prompts/lower.md", prompt(inputs={"pr": "The number."}))
    board.write("prompts/upper.md", prompt(inputs={"PR": "The number."}))
    board.write("prompts/digit.md", prompt(inputs={"2pr": "The number."}))
    assert checked(board) == {
        "/prompts/lower.md": None,
        "/prompts/upper.md": "board::record::invalid",
        "/prompts/digit.md": "board::record::invalid",
    }


def test_a_directory_starting_with_an_input_is_the_input_alone_or_followed_by_a_slash(board: Board) -> None:
    named = {"base": "A directory."}
    board.write("prompts/alone.md", prompt(runs_in={"directory": "{base}"}, inputs=named))
    board.write("prompts/under.md", prompt(runs_in={"directory": "{base}/work"}, inputs=named))
    board.write("prompts/glued.md", prompt(runs_in={"directory": "{base}work"}, inputs=named))
    assert checked(board) == {
        "/prompts/alone.md": None,
        "/prompts/under.md": None,
        "/prompts/glued.md": "board::record::invalid",
    }


def marks_code(board: Board, field: str, at: str) -> str | None:
    """`board check`'s code for a list of marked repos holding `at` under `field`; `None` when it is valid."""
    board.write("prompts/repos.json", json.dumps({"$type": "prompts/repos/v1", field: [at]}))
    return checked(board)["/prompts/repos"]


def test_the_list_of_marked_repos_references_mounts_and_refuses_a_path_that_is_not_a_reference(board: Board) -> None:
    mount(board, "notes", {"prompts/a.md": prompt("Notes")}, marked=False)
    assert marks_code(board, "mounts", "/repos/notes") is None
    assert marks_code(board, "mounts", "repos/notes") == "board::ref::malformed"
    assert marks_code(board, "repos", "/repos/notes") == "board::record::invalid"
