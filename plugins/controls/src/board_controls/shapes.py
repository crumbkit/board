"""The control record, and the plugin's inputs and outputs.

Board validates each call's input and output, and every record, so the plugin
only types them. manifest.py generates the schemas from these shapes, and is
the only module that imports pydantic (STANDARDS.md P3).
"""

from collections.abc import Callable
from typing import Annotated, Literal, NotRequired, TypedDict

CONTROL_TYPE = "controls/control/v1"
# The variable that holds the agent `evaluate` starts for a judged rule, as a JSON array of strings.
AGENT_VARIABLE = "BOARD_CONTROLS_AGENT"
# How long `evaluate` may take, in seconds: each judged part waits on a model.
EVALUATE_TIMEOUT = 1800


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


def _kind(required: list[str], others: list[str]) -> dict[str, object]:
    """One kind of rule: the fields it requires, and none of the other kinds' fields."""
    return {"required": required, "not": {"anyOf": [{"required": [field]} for field in others]}}


# The kind of a rule, chosen by which fields it holds. Board searches no reference under `anyOf`, so the
# kinds share one object whose `pattern` and `judged` are references Board checks.
KINDS = [
    _kind(["budget", "max"], ["pattern", "show", "judged"]),
    _kind(["pattern", "show"], ["budget", "max", "per", "judged"]),
    _kind(["judged"], ["budget", "max", "per", "pattern", "show"]),
]


class Rule(TypedDict, total=False):
    """What a program checks: a budget, a pattern file or a judged rubric, told apart by their fields.

    A budget, `budget` and `max`, limits the words or bytes of the files or
    text; with `per` `total`, the default, the files' usage is summed, and
    with `file` each file on its own must be within `max`. Words are
    separated by any whitespace. A pattern, `pattern` and `show`, names a file
    of regular expressions, one per line, that text must not match; with
    `show` `match-only`, a finding quotes what matched and never the
    expression. A judged rule, `judged`, names a rubric that a clean-context
    model reads text against.
    """

    budget: Literal["words", "bytes"]
    max: Annotated[int, Extra(minimum=0)]
    per: Annotated[Literal["total", "file"], Extra(default="total")]
    pattern: BoardRef
    show: Literal["match-only", "pattern"]
    judged: BoardRef


ControlRecord = TypedDict(
    "ControlRecord",
    {
        "$type": Literal["controls/control/v1"],
        "requirement": str,
        "purpose": str,
        "applies_to": NotRequired[list[BoardRef]],
        "paths": NotRequired[list[str]],
        "rule": NotRequired[Annotated[Rule, Extra(oneOf=KINDS)]],
    },
)
ControlRecord.__doc__ = (
    "`controls/control/v1`: a requirement on files, why it exists, and optionally a rule a program checks."
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
    rule: NotRequired[Rule]


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
                format="board-path",
                description="The directory or file whose controls to check, by its logical path,"
                " such as /docs; / when left out.",
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
    requirement: str
    budget: Literal["words", "bytes"]
    max: int
    used: int
    files: list[str]
    per: NotRequired[Literal["file"]]
    largest: NotRequired[str | None]
    over: NotRequired[list[FileUsed]]


class Stated(TypedDict):
    """A control without a budget, its patterns, and how many of its files are at or under the path."""

    control: str
    requirement: str
    purpose: str
    paths: list[str]
    files: int


class CheckOutput(TypedDict):
    """The controls that apply under the path, by the control's path; each budget is met."""

    measured: list[Checked]
    stated: list[Stated]


class EvaluateInput(TypedDict):
    """Check each part of text against each control's rule."""

    controls: Annotated[
        list[Annotated[str, Extra(pattern="^/")]],
        Extra(
            minItems=1,
            description="The controls to check the parts against, by their logical paths in the board's own tree.",
        ),
    ]
    parts: Annotated[
        dict[str, str],
        Extra(
            minProperties=1,
            description="Each part of text to check, by its name, such as pr.body.",
        ),
    ]


class Finding(TypedDict):
    """What a rule found wrong in a part: the text at fault, and its line when it has one.

    A pattern finding quotes what matched, and names the expression as
    `pattern` only when the rule's `show` is `pattern`. A judged finding
    carries `rewrite`, the model's instruction for fixing the text.
    """

    text: str
    line: NotRequired[int]
    pattern: NotRequired[str]
    rewrite: NotRequired[str]


type Kind = Literal["budget", "pattern", "judged"]
type Status = Literal["passed", "failed", "skipped", "stated"]


class Evaluated(TypedDict):
    """One control's verdict on one part.

    `status` is `skipped` for a judged rule when the part failed a budget or
    pattern rule, and `stated` for a control without a rule. A budget result
    carries `used` and `max`. `omitted` counts the findings left out past the
    first 20.
    """

    control: str
    part: str
    rule: Kind | None
    status: Status
    findings: list[Finding]
    used: NotRequired[int]
    max: NotRequired[int]
    omitted: NotRequired[int]


class EvaluateOutput(TypedDict):
    """Each control's verdict on each part, by control as given and then by part; `passed` when none failed."""

    passed: bool
    results: list[Evaluated]
