"""Reflect's slug for a title and its reading of `private`, against cases from Reflect's own tests."""

import pytest

from board_notes.notes import private, slug


@pytest.mark.parametrize(
    ("title", "expected"),
    [
        # slugForTitle's documented examples (packages/core/src/markdown/slug.ts).
        ("Meeting Notes", "meeting-notes"),
        ("Don't Panic!", "dont-panic"),
        ("日本語ノート", "日本語ノート"),
        ("🎉🎉🎉", "untitled"),
        ("CON", "con-note"),
        ("  a_b -- c  ", "a-b-c"),
        ("x" * 59 + " y", "x" * 59),
        # Cases checked against slugForTitle itself, which drops combining marks NFC cannot compose,
        # counts characters outside the Basic Multilingual Plane as one each, and uses JavaScript's `\s`.
        ("Cafe\u0301", "café"),
        ("x\u20dd y", "x-y"),
        ("\U0001d49c\U0001d4b7 math", "\U0001d49c\U0001d4b7-math"),
        ("\U0001d49c" * 100, "\U0001d49c" * 60),
        ("a\ufeffb", "a-b"),
        ("a\x1cb\x85c", "abc"),
    ],
)
def test_a_title_takes_the_slug_reflect_gives_it(title: str, expected: str) -> None:
    assert slug(title) == expected


@pytest.mark.parametrize(
    ("frontmatter", "hidden"),
    [
        ("private: true", True),
        ("private: on", True),
        ('private: "yes"', True),
        ("private: 1", True),
        ("private: maybe", True),
        ("private: !!bool true", True),
        ('"private": true', True),
        ("'private' : true", True),
        ("private : true", True),
        ("private: false\ntitle: x\nprivate: true", True),
        ("private: false", False),
        ("private: 'no'", False),
        ("private: off # shown", False),
        ("private: 0", False),
        ("privately: true", False),
    ],
)
def test_a_note_is_hidden_unless_every_private_key_in_its_frontmatter_is_plainly_false(
    frontmatter: str, *, hidden: bool
) -> None:
    assert private(f"---\nid: x\n{frontmatter}\n---\n# Note\n") is hidden


def test_a_byte_order_mark_before_the_frontmatter_does_not_show_a_private_note() -> None:
    assert private("\ufeff---\nprivate: true\n---\n") is True
    assert private("\ufeff---\nprivate: false\n---\n") is False


def test_a_private_line_in_the_body_after_the_frontmatter_does_not_hide_the_note() -> None:
    assert private("---\nid: x\n---\nprivate: true\n") is False
    assert private("---\nprivate: true\n---\n") is True
