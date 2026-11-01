"""The destination is read from the origin URL in each form GitHub gives, and any other URL names no destination."""

from board_github.checkout import GITHUB


def test_each_github_url_form_names_owner_and_repository_and_its_non_github_twin_names_none() -> None:
    pairs = [
        ("https://github.com/Octo/Widgets.git", "https://gitlab.com/Octo/Widgets.git"),
        ("https://token@github.com/octo/widgets", "https://token@github.example/octo/widgets"),
        ("git@github.com:Octo/Widgets.git", "git@gitlab.com:Octo/Widgets.git"),
        ("ssh://git@github.com/octo/widgets.git", "ssh://git@example.com/octo/widgets.git"),
        ("git@github.com-work:octo/widgets.git", "git@github.org:octo/widgets.git"),
    ]
    for github, other in pairs:
        matched = GITHUB.fullmatch(github)
        assert matched is not None, github
        assert (matched[1].lower(), matched[2].lower()) == ("octo", "widgets"), github
        assert GITHUB.fullmatch(other) is None, other
    assert GITHUB.fullmatch("octo/widgets") is None
