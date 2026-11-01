"""The boundary and crossing records, and the plugin's inputs and outputs.

Board validates each call's input and output, and every record, so the plugin
only types them. manifest.py generates the schemas from these shapes, and is
the only module that imports pydantic (STANDARDS.md P3).
"""

from collections.abc import Callable
from typing import Annotated, Literal, NotRequired, TypedDict

BOUNDARY_TYPE = "boundary/boundary/v1"
CROSSING_TYPE = "boundary/crossing/v1"
# Where crossings live in the personal board, each named by its digest.
CROSSINGS = "/crossings/"
# Where boundaries belong in the personal board; every entry there must be a valid boundary.
BOUNDARIES = "boundaries/"
# How long `check` may take, in seconds: as long as `controls.evaluate`, whose judged rules wait on a model.
CHECK_TIMEOUT = 1800
# The variables `controls.evaluate` reads for a judged rule, passed through to it.
AGENT_VARIABLES = ["BOARD_CONTROLS_AGENT", "ANTHROPIC_API_KEY", "CLAUDE_CODE_OAUTH_TOKEN"]


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


def described(text: str, **keywords: object) -> Extra:
    """A property's `description`, with any other keywords."""
    return Extra(description=text, **keywords)


# A local pattern, exempt from the shared-shape rule: board_plugin::shape has no SHA-256 shape yet, and the
# one other copy is requests/reply/v1's. A shared shape replaces both when the shapes lint adds one.
Digest = Annotated[
    str, described("The SHA-256 of the crossing's content, in 64 lowercase hex digits.", pattern="^[0-9a-f]{64}$")
]
Action = Annotated[str, described("The capability that sends the parts, such as github.pr-create.")]
Destination = Annotated[str, described("Where the parts go, such as github:octo/board.")]
Parts = Annotated[
    dict[str, str],
    described("Each part of the content, by its name, such as pr.body.", minProperties=1),
]
FROM = "The checkout's path in the personal board, such as /repos/board; left out for a checkout no board mounts."
# A capability input marks a path as a board-path; the crossing keeps it as a reference.
FromInput = Annotated[str, described(FROM, pattern="^/", format="board-path")]
From = Annotated[str, described(FROM, format="board-ref")]
Boundaries = Annotated[
    list[Annotated[str, Extra(format="board-ref", **{"x-board-type": "boundary/boundary/v1"})]],
    described("The boundaries that matched, by their paths."),
]


class Check(TypedDict):
    """A control, and the names of the parts it applies to."""

    control: Annotated[
        str,
        described(
            "The control to check the parts against, in the personal board's own tree.",
            format="board-ref",
            **{"x-board-type": "controls/control/v1"},
        ),
    ]
    parts: Annotated[
        list[str],
        described("Globs of the part names it applies to, such as pr.* or *.", minItems=1),
    ]


BoundaryRecord = TypedDict(
    "BoundaryRecord",
    {
        "$type": Annotated[Literal["boundary/boundary/v1"], described("The record's type.")],
        "from": NotRequired[
            Annotated[
                list[Annotated[str, Extra(format="board-ref")]],
                described("The checkouts it applies to, by their paths, usually mounts; left out for any checkout."),
            ]
        ],
        "to": Annotated[str, described("A glob of the destinations it applies to, such as github:octo/*.")],
        "actions": Annotated[
            list[str],
            described("Globs of the actions it applies to, such as github.*.", minItems=1),
        ],
        "checks": Annotated[list[Check], described("The controls to check and the parts each applies to.", minItems=1)],
    },
)
BoundaryRecord.__doc__ = (
    "`boundary/boundary/v1`: the controls content must pass to leave a checkout for a destination by an action."
    """

Globs are shell-style, matched with case: `*` matches any characters,
`/` and `.` included."""
)


class Failure(TypedDict):
    """One finding of a failed check, as a refusal's `failures` field names it."""

    control: str
    part: str
    text: str
    line: NotRequired[int]
    rewrite: NotRequired[str]


class Finding(TypedDict):
    """What a control found wrong in a part: the text at fault, and its line when it has one."""

    text: Annotated[str, described("The text at fault: a match, a budget's count, or a quoted sentence.")]
    line: NotRequired[Annotated[int, described("The line of the part the text starts on.")]]
    pattern: NotRequired[Annotated[str, described("The expression that matched, when its control shows it.")]]
    rewrite: NotRequired[Annotated[str, described("How to rewrite the text, from a judged rule.")]]


type Status = Literal["passed", "failed", "skipped", "stated"]


class Verdict(TypedDict):
    """One control's verdict on one part, as `controls.evaluate` gave it."""

    control: Annotated[
        str, described("The control's path.", format="board-ref", **{"x-board-type": "controls/control/v1"})
    ]
    part: Annotated[str, described("The part's name.")]
    rule: Annotated[
        Literal["budget", "pattern", "judged"] | None,
        described("The kind of the control's rule, or null for a written requirement."),
    ]
    status: Annotated[Status, described("passed, failed, skipped after a free rule failed, or stated.")]
    findings: Annotated[list[Finding], described("What the control found wrong, at most 20.")]
    used: NotRequired[Annotated[int, described("What a budget counted.")]]
    max: NotRequired[Annotated[int, described("A budget's limit.")]]
    omitted: NotRequired[Annotated[int, described("How many findings were left out past the first 20.")]]


CrossingRecord = TypedDict(
    "CrossingRecord",
    {
        "$type": Annotated[Literal["boundary/crossing/v1"], described("The record's type.")],
        "digest": Digest,
        "action": Action,
        "to": Destination,
        "from": NotRequired[From],
        "passed": Annotated[bool, described("Whether every check passed.")],
        "boundaries": Boundaries,
        "verdicts": Annotated[list[Verdict], described("Each check's verdict on each part it applies to.")],
        "result": NotRequired[Annotated[str, described("What the destination returned, such as a URL.")]],
    },
)
CrossingRecord.__doc__ = (
    "`boundary/crossing/v1`: the verdicts on one content, at /crossings/<digest> in the personal board."
)


CheckInput = TypedDict(
    "CheckInput",
    {"action": Action, "to": Destination, "from": NotRequired[FromInput], "parts": Parts},
)
CheckInput.__doc__ = "Check the parts against every boundary that matches the action, destination and checkout."


class CheckOutput(TypedDict):
    """The crossing written, the boundaries that matched, and every verdict; each check passed."""

    digest: Digest
    crossing: Annotated[str, described("The crossing's path in the personal board.")]
    boundaries: Boundaries
    verdicts: Annotated[list[Verdict], described("Each check's verdict on each part it applies to.")]
    unchecked: Annotated[list[str], described("The parts no check of a matching boundary names, by name.")]
    note: Annotated[str | None, described("Why nothing was checked, or null.")]


CoversInput = TypedDict("CoversInput", {"action": Action, "to": Destination, "from": NotRequired[FromInput]})
CoversInput.__doc__ = (
    "Name the boundaries that cover the action to the destination from the checkout, checking nothing."
)


class CoversOutput(TypedDict):
    """The boundaries that cover it; none when nothing would be checked."""

    boundaries: Boundaries


class VerifyInput(TypedDict):
    """Find the crossing of a digest, or of the content that `action`, `to` and `parts` give."""

    digest: NotRequired[Digest]
    action: NotRequired[Action]
    to: NotRequired[Destination]
    parts: NotRequired[Parts]


class VerifyOutput(TypedDict):
    """Whether a passing crossing exists for the digest."""

    digest: Digest
    crossing: Annotated[str | None, described("The crossing's path, or null when there is none.")]
    passed: Annotated[
        bool,
        described("Whether the crossing exists, every check passed, and the same boundaries match it now."),
    ]
    result: Annotated[str | None, described("What the destination returned, or null.")]


class RecordInput(TypedDict):
    """Add what the destination returned to the passing crossing of `digest`."""

    digest: Digest
    result: Annotated[str, described("What the destination returned, such as the pull request's URL.", minLength=1)]


class RecordOutput(TypedDict):
    """The crossing and the result it now holds."""

    crossing: Annotated[str, described("The crossing's path in the personal board.")]
    result: Annotated[str, described("What the destination returned.")]
