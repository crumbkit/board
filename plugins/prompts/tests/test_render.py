"""Rendering as a pure function: the body, one pass of substitution, unused inputs and the rules for values."""

import pytest
from board_plugin.protocol import PluginError

from board_prompts.render import body, checked, placed, render, unused

DECLARED = {"repo": "The repository.", "pr": "The pull request."}


def test_the_body_is_the_text_after_the_line_closing_the_frontmatter() -> None:
    assert body("---\ntitle: x\n---\nYou are {pr}.\n") == "You are {pr}.\n"
    assert body("\ufeff---\r\ntitle: x\r\n...\r\nBody\r\n") == "Body\r\n"
    assert body("---\ntitle: x\n---\n") == ""


def test_each_declared_input_is_replaced_and_every_other_brace_is_left_as_written() -> None:
    text = 'Review #{pr} in {repo}; keep {prr}, {"a": 1} and {Pr}.'
    assert render(text, {"repo": "example-org/widgets", "pr": "2140"}) == (
        'Review #2140 in example-org/widgets; keep {prr}, {"a": 1} and {Pr}.'
    )


def test_rendering_is_one_pass_so_a_value_naming_another_input_stays_literal() -> None:
    assert render("{repo} then {pr}", {"repo": "{pr}", "pr": "2140"}) == "{pr} then 2140"


def test_unused_lists_each_declared_input_the_body_never_names() -> None:
    assert unused(DECLARED, "Review #{pr}.") == ["repo"]
    assert unused(DECLARED, "Review #{pr} in {repo}.") == []


def refused(given: dict[str, str]) -> tuple[str, dict[str, object]]:
    with pytest.raises(PluginError) as raised:
        checked(DECLARED, given)
    return raised.value.code, raised.value.fields


def test_every_declared_input_needs_a_value_and_each_missing_one_is_named() -> None:
    assert refused({}) == ("prompts::input::missing", {"inputs": ["pr", "repo"]})
    assert checked(DECLARED, {"repo": "a/b", "pr": "1"}) == {"repo": "a/b", "pr": "1"}


def test_a_value_naming_no_declared_input_is_refused_by_name() -> None:
    assert refused({"repo": "a/b", "pr": "1", "focus": "x"}) == ("prompts::input::unknown", {"inputs": ["focus"]})


@pytest.mark.parametrize("value", ["x" * 201, "two\nlines", "carriage\rreturn", "tab\there", "bell\x07", "sep\u2028"])
def test_a_value_over_200_characters_spanning_lines_or_holding_a_control_character_is_refused(value: str) -> None:
    assert refused({"repo": "a/b", "pr": value}) == ("prompts::input::bad_value", {"inputs": ["pr"]})


def test_a_value_of_200_characters_on_one_line_is_admitted() -> None:
    assert checked(DECLARED, {"repo": "a/b", "pr": "\u00e9" * 200})["pr"] == "\u00e9" * 200


def test_a_format_character_such_as_a_direction_override_or_zero_width_space_is_a_bad_value() -> None:
    for value in ("21\u202e40", "21\u200b40", "\ufeff2140"):
        assert refused({"repo": "a/b", "pr": value}) == ("prompts::input::bad_value", {"inputs": ["pr"]})
    assert checked(DECLARED, {"repo": "a/b", "pr": "2140 \u00e9"}) == {"repo": "a/b", "pr": "2140 \u00e9"}


@pytest.mark.parametrize(
    "runs_in",
    [
        {"remote": "example-org/.."},
        {"remote": "example-org/."},
        {"directory": "/srv/\u202eexe.txt"},
        {"directory": "/a\r/b"},
        {"directory": "/a\tb"},
    ],
)
def test_a_runs_in_naming_dot_or_dot_dot_or_holding_a_control_or_format_character_is_refused(
    runs_in: dict[str, str],
) -> None:
    with pytest.raises(PluginError) as raised:
        placed(runs_in, {})
    assert raised.value.code == "prompts::runs_in::invalid"


def test_a_runs_in_with_dots_inside_a_name_or_path_is_admitted() -> None:
    assert placed({"remote": "example-org/.github"}, {}) == {"remote": "example-org/.github"}
    assert placed({"remote": "example-org/a..b"}, {}) == {"remote": "example-org/a..b"}
    assert placed({"directory": "/srv/a..b/.config"}, {}) == {"directory": "/srv/a..b/.config"}
