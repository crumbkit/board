"""The control record, and the plugin's inputs and outputs.

Board validates each call's input and output, and every record, so the plugin
only types them. manifest.py generates the schemas from these shapes, and is
the only module that imports pydantic (STANDARDS.md P3).
"""

from collections.abc import Callable
from typing import Annotated, Literal, NotRequired, TypedDict

CONTROL_TYPE = "controls/control/v1"


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
    """A limit on the words or bytes of every file the control applies to, summed.

    Words are separated by any whitespace.
    """

    budget: Literal["words", "bytes"]
    max: Annotated[int, Extra(minimum=0)]


ControlRecord = TypedDict(
    "ControlRecord",
    {
        "$type": Literal["controls/control/v1"],
        "requirement": str,
        "purpose": str,
        "applies_to": NotRequired[list[BoardRef]],
        "paths": NotRequired[list[str]],
        "rule": NotRequired[Annotated[Budget | None, Extra(default=None)]],
    },
)
ControlRecord.__doc__ = (
    "`controls/control/v1`: a requirement on files, why it exists, and optionally a budget a program checks."
    """

It applies to the entries `applies_to` references and to every entry of
the board whose path matches one of `paths`, SQLite GLOB patterns such as
`docs/*.md`."""
)


class ForPathInput(TypedDict):
    """The controls that apply to this logical path."""

    path: Annotated[
        str,
        Extra(
            pattern="^/",
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
    """Check the controls that apply to a file at or under `path`, `/` when left out."""

    path: NotRequired[
        Annotated[
            str,
            Extra(
                pattern="^/",
                description="The directory or file whose controls to check, by its logical path,"
                " such as /docs; / when left out.",
            ),
        ]
    ]


class Checked(TypedDict):
    """A budget control and what all its files use."""

    control: str
    requirement: str
    budget: Literal["words", "bytes"]
    max: int
    used: int
    files: list[str]


class Stated(TypedDict):
    """A control without a rule, its patterns, and how many of its files are at or under the path."""

    control: str
    requirement: str
    purpose: str
    paths: list[str]
    files: int


class CheckOutput(TypedDict):
    """The controls that apply under the path, by the control's path; each budget is met."""

    measured: list[Checked]
    stated: list[Stated]
