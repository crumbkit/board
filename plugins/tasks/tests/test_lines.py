"""The line editor that the write calls change a task's frontmatter with."""

import pytest
from board_plugin.protocol import PluginError

from board_tasks.lines import with_fields

TASK = '---\n$type: tasks/task/v2\ntitle: "Call the bank"  # by phone\nstatus: open\nid: abc\n---\n\nBody.\n'


def test_a_plain_line_is_replaced_a_missing_key_is_added_last_and_every_other_byte_is_kept() -> None:
    edited = with_fields(TASK, {"status": "done", "reviewed": "2026-10-25"}, "/tasks/bank.md")
    assert edited == (
        '---\n$type: tasks/task/v2\ntitle: "Call the bank"  # by phone\nstatus: "done"\nid: abc\n'
        'reviewed: "2026-10-25"\n---\n\nBody.\n'
    )


def test_windows_line_endings_are_kept() -> None:
    text = TASK.replace("\n", "\r\n")
    assert with_fields(text, {"status": "done"}, "/t.md") == text.replace("status: open", 'status: "done"')


@pytest.mark.parametrize(
    ("line", "plain"),
    [
        ("defer: |\n  soon\n", False),
        ("defer:\n  - soon\n", False),
        ("defer: [soon]\n", False),
        ('defer: "soon\n  later"\n', False),
        ("defer: soon\ndefer: someday\n", False),
        ("defer: soon\n", True),
        ("defer: 2026-11-01  # after the trip\n", True),
        ('defer: "soon"\n', True),
    ],
)
def test_a_key_whose_line_is_not_one_plain_scalar_is_refused_and_a_plain_one_is_replaced(
    line: str,
    plain: bool,  # noqa: FBT001 - pytest passes the case's flag
) -> None:
    text = f"---\ntitle: T\n{line}status: open\n---\n"
    if plain:
        assert 'defer: "2026-12-01"\n' in with_fields(text, {"defer": "2026-12-01"}, "/tasks/t.md")
        return
    with pytest.raises(PluginError) as refused:
        with_fields(text, {"defer": "2026-12-01"}, "/tasks/t.md")
    assert (refused.value.code, refused.value.fields) == (
        "tasks::task::not_plain",
        {"path": "/tasks/t.md", "key": "defer"},
    )
    assert "one line" in refused.value.message


def test_a_key_indented_under_another_is_not_the_top_level_key() -> None:
    text = "---\ntitle: T\nmeta:\n  status: x\nstatus: open\n---\n"
    assert with_fields(text, {"status": "done"}, "/t.md") == text.replace("status: open", 'status: "done"')


@pytest.mark.parametrize("text", ["title: T\n", "---\ntitle: T\n"])
def test_a_file_without_a_closed_frontmatter_is_refused_and_one_with_it_is_edited(text: str) -> None:
    with pytest.raises(PluginError) as refused:
        with_fields(text, {"status": "done"}, "/t.md")
    assert refused.value.code == "tasks::task::not_plain"
    assert with_fields(f"---\n{text.removeprefix('---\n')}---\n", {"status": "done"}, "/t.md").endswith('"done"\n---\n')
