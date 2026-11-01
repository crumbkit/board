"""The control record, and the plugin's inputs and outputs.

Board validates each call's input and output, and every record, so the plugin
only types them. manifest.py generates the schemas from these shapes, and is
the only module that imports pydantic (STANDARDS.md P3).
"""

from collections.abc import Callable
from typing import Annotated, Literal, NotRequired, TypedDict

CONTROL_TYPE = "controls/control/v1"
REPO_TYPE = "board/repo/v1"
DECISION_TYPE = "decisions/decision/v1"
# The default and the most controls each of `check`'s lists returns, as in the other listings.
LIMIT_DEFAULT = 100
LIMIT_MAX = 10_000


class Extra:
    """JSON Schema keywords added to the schema generated for the type it annotates."""

    def __init__(self, **keywords: object) -> None:
        """Hold the keywords."""
        self.keywords = keywords

    def __get_pydantic_json_schema__(
        self, core_schema: object, handler: Callable[[object], dict[str, object]]
    ) -> dict[str, object]:
        """The schema pydantic generates, with the keywords added."""
        return handler(core_schema) | self.keywords


# A reference Board resolves and checks: an absolute logical path to any entry.
BoardRef = Annotated[str, Extra(format="board-ref")]


class Budget(TypedDict):
    """A limit on the words or bytes of the files the control applies to.

    With `per` `total`, the default, the files' usage is summed; with `file`,
    each file on its own must be within `max`. Words are separated by any
    whitespace.
    """

    budget: Literal["words", "bytes"]
    max: Annotated[int, Extra(minimum=0)]
    per: NotRequired[Annotated[Literal["total", "file"], Extra(default="total")]]


ControlRecord = TypedDict(
    "ControlRecord",
    {
        "$type": Literal["controls/control/v1"],
        "requirement": str,
        "purpose": str,
        "summary": NotRequired[Annotated[str, Extra(maxLength=120)]],
        "applies_to": NotRequired[list[BoardRef]],
        "repos": NotRequired[Annotated[list[BoardRef], Extra(minItems=1)]],
        "paths": NotRequired[list[str]],
        "rule": NotRequired[Annotated[Budget | None, Extra(default=None)]],
        "adds_step": NotRequired[Annotated[str, Extra(minLength=1)]],
        "basis": NotRequired[list[BoardRef]],
    },
)
ControlRecord.__doc__ = (
    "`controls/control/v1`: a requirement on files, why it exists, and optionally a budget a program checks."
    """

It applies to the entries `applies_to` references and to every entry of
the board whose path matches one of `paths`, SQLite GLOB patterns such as
`docs/*.md`. With `repos`, each pattern is matched against a file's path from
the root of each listed repository, and reaches nothing outside them.
`summary` is one line naming the code covered and what is asked. `adds_step`
names a workflow step the control adds to the work on its files; it is stored
and shown, and adds nothing yet. `basis` names the decisions the control
rests on."""
)


class ForPathInput(TypedDict):
    """The controls that apply to this logical path."""

    path: Annotated[
        str,
        Extra(
            pattern="^/",
            format="board-path",
            description="The file to list the controls of, by its logical path, such as /docs/README.md;"
            " ask about a new file by the path it will have.",
        ),
    ]


class Applying(TypedDict):
    """A control that applies to the path asked about."""

    control: str
    requirement: str
    purpose: str
    rule: Budget | None


class ForPathOutput(TypedDict):
    """The controls that apply to the path, by the control's path."""

    controls: list[Applying]


class CheckInput(TypedDict):
    """Check the controls that apply to a file at or under `path`, or to the `files` named beneath it.

    Without `path`, the caller's repository: `/` in the board's own repository.
    """

    path: NotRequired[
        Annotated[
            str,
            Extra(
                pattern="^/",
                format="board-path",
                description="The directory or file whose controls to check, by its logical path, such as"
                " /repos/app; the caller's repository when left out, / in the board's own.",
            ),
        ]
    ]
    files: NotRequired[
        Annotated[
            list[Annotated[str, Extra(minLength=1, pattern="^[^/]")]],
            Extra(
                description="The files to check, each by its path relative to `path` as git names it, such as"
                " src/main.rs, so a repository the board does not mount is checked too."
            ),
        ]
    ]
    measure: NotRequired[
        Annotated[
            bool,
            Extra(
                default=True,
                description="Whether to measure each budget, refusing the check when one is over; when false,"
                " budgets are listed among `stated` unmeasured. True when left out.",
            ),
        ]
    ]
    limit: NotRequired[
        Annotated[
            int,
            Extra(
                minimum=1,
                maximum=LIMIT_MAX,
                default=LIMIT_DEFAULT,
                description=f"The most controls each of `measured` and `stated` lists, 1 to {LIMIT_MAX:,};"
                f" {LIMIT_DEFAULT} when left out.",
            ),
        ]
    ]


class FileUsed(TypedDict):
    """What one file uses of a per-file budget."""

    file: str
    used: int


class Checked(TypedDict):
    """A budget control and what its files use: all of them summed, or its largest file when `per` is `file`.

    Only a per-file control carries `per`, `largest`, the file it measured
    as `used` (null when it has none), and `over`, every file over `max`.
    """

    control: str
    board: str
    requirement: str
    budget: Literal["words", "bytes"]
    max: int
    used: int
    files: list[str]
    adds_step: str | None
    basis: list[str]
    basis_superseded: NotRequired[list[str]]
    per: NotRequired[Literal["file"]]
    largest: NotRequired[str | None]
    over: NotRequired[list[FileUsed]]


class Stated(TypedDict):
    """A control without a rule, its patterns, and how many of its files are at or under the path.

    With `files` in the input, `files` counts the named files the control matches.
    """

    control: str
    board: str
    requirement: str
    purpose: str
    paths: list[str]
    files: int
    adds_step: str | None
    basis: list[str]
    basis_superseded: NotRequired[list[str]]


class Counts(TypedDict):
    """A number for each of `check`'s lists."""

    measured: int
    stated: int


class Cut(TypedDict):
    """Whether the limit cut each of `check`'s lists short."""

    measured: bool
    stated: bool


class CheckOutput(TypedDict):
    """The controls that apply under the path, by the control's path; each budget is met.

    `path` is what was checked, null when no path was given and the caller's
    repository is none the board knows; `next` then says what to do.
    `own_controls` is `read` when the path is in a mounted tree, whose own
    controls are listed with the board's; a tree that is not a board, or
    holds no controls, lists none. It is `not_read` when the path is a repo
    record without a checkout. Each control's `board` is the root of the board
    that holds it.
    """

    path: str | None
    measured: list[Checked]
    stated: list[Stated]
    total: Counts
    truncated: Cut
    own_controls: NotRequired[Literal["read", "not_read"]]
    next: NotRequired[str]
