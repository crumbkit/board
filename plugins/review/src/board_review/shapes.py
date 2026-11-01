"""The review records, and the plugin's inputs and outputs.

Board validates each call's input and output, and every record, so the plugin
only types them. manifest.py generates the schemas from these shapes, and is
the only module that imports pydantic (STANDARDS.md P3).
"""

from collections.abc import Callable
from typing import Annotated, Literal, NotRequired, TypedDict

from board_plugin.protocol import JsonValue

BRIEF_TYPE = "review/brief/v1"
REPORT_TYPE = "review/report/v1"
CHANGE_TYPE = "changes/change/v1"

# The variable that holds the agent `review.run` starts, as a JSON array of strings.
AGENT_VARIABLE = "BOARD_REVIEW_AGENT"
# How long `review.run` may take, in seconds: an agent reading a large change takes minutes.
RUN_TIMEOUT = 1800
# How long `review.post` may take, in seconds: one decide and one `gh pr comment`.
POST_TIMEOUT = 60

Verdict = Literal["approve", "changes", "comment"]
Severity = Literal["high", "medium", "low"]


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


def _ref(record_type: str) -> object:
    return Extra(format="board-ref", **{"x-board-type": record_type})


# A range of full commit ids, so a record names exactly which commits it means and cannot pass git an option.
Commits = Annotated[str, Extra(pattern=r"^[0-9a-f]{40,64}\.\.[0-9a-f]{40,64}$")]
PatchId = Annotated[str, Extra(pattern="^[0-9a-f]{40,64}$")]
Blob = Annotated[str, Extra(pattern="^[0-9a-f]{40,64}$")]


class Subject(TypedDict):
    """A record under review, pinned by its blob, so a review names exactly which version it read."""

    record: Annotated[str, Extra(format="board-ref")]
    blob: Blob


# A brief, and a run's output, name a range of commits or a subject, never both.
TARGET: dict[str, JsonValue] = {"oneOf": [{"required": ["range"]}, {"required": ["subject"]}]}
# A report covers a range with its patch ids, or a subject, never both.
REPORT_TARGET: dict[str, JsonValue] = {
    **TARGET,
    "dependentRequired": {"range": ["patch_ids"], "patch_ids": ["range"]},
}

BriefRecord = TypedDict(
    "BriefRecord",
    {
        "$type": Literal["review/brief/v1"],
        "range": NotRequired[Commits],
        "subject": NotRequired[Subject],
        "focus": Annotated[str, Extra(minLength=1)],
        "questions": NotRequired[list[str]],
        "follows": NotRequired[Annotated[str, _ref(REPORT_TYPE)]],
        "for": NotRequired[Annotated[str, _ref(CHANGE_TYPE)]],
        "step": NotRequired[Annotated[str, Extra(pattern="^[a-z0-9][a-z0-9-]{0,59}$")]],
        "repo": NotRequired[Annotated[str, Extra(format="board-ref")]],
    },
)
BriefRecord.__doc__ = """`review/brief/v1`, at /reviews/<id>/brief: what a change's author asks a review to look at.

`range` is the commits under review as `base..head`, both full commit ids,
and `subject` the record under review at its blob; a brief has one of them.
`focus` says what matters and what to check first, `questions` what the
author wants answered, and `follows` the report this brief continues from.
`for` is the change the review belongs to and `step` the change's step it
completes. `repo` is the repo record of the repository the range is in; with
it, `review.run` reads the range in the caller's clone of that repository."""


class Finding(TypedDict):
    """One thing a review found, at a file and line when it has one."""

    severity: Severity
    text: Annotated[str, Extra(minLength=1)]
    file: NotRequired[str]
    line: NotRequired[Annotated[int, Extra(minimum=1)]]


class Cost(TypedDict):
    """What a review took: its seconds, and its tokens when a model ran and said."""

    seconds: Annotated[float, Extra(minimum=0)]
    input_tokens: NotRequired[Annotated[int, Extra(minimum=0)]]
    output_tokens: NotRequired[Annotated[int, Extra(minimum=0)]]


ReportRecord = TypedDict(
    "ReportRecord",
    {
        "$type": Literal["review/report/v1"],
        "brief": NotRequired[Annotated[str, _ref(BRIEF_TYPE)]],
        "range": NotRequired[Commits],
        "patch_ids": NotRequired[list[PatchId]],
        "subject": NotRequired[Subject],
        "findings": list[Finding],
        "verdict": Verdict,
        "by": Annotated[str, Extra(minLength=1)],
        "cost": NotRequired[Cost],
        "source": NotRequired[Annotated[str, Extra(pattern="^https?://")]],
        "follows": NotRequired[Annotated[str, _ref(REPORT_TYPE)]],
    },
)
ReportRecord.__doc__ = """`review/report/v1`, at /reviews/<id>/report: what one review of a range or a record found.

`range` holds the full commit ids reviewed, and `patch_ids` the
`git patch-id --verbatim` of each commit in it that has a diff, oldest first,
so the review still covers the commits after a rebase. A review of a record
holds its `subject` instead, and covers no commit. `brief` is absent for
a review a person wrote. `by` names the models the agent reported, else the
program the agent command starts, or the person. `source` is a URL, such as
a GitHub review. `follows` is the previous report."""


class RunInput(TypedDict):
    """Review the range or record a brief names with the agent BOARD_REVIEW_AGENT names."""

    brief: Annotated[
        str,
        Extra(
            pattern="^/reviews/[^/]+/brief$",
            format="board-path",
            description="The brief to run, by its logical path, such as /reviews/fix-sync/brief.",
        ),
    ]


class RunOutput(TypedDict):
    """The report written, the commits or record it covers, the verdict and how many findings."""

    report: str
    range: NotRequired[str]
    subject: NotRequired[Subject]
    verdict: Verdict
    findings: int


class PostInput(TypedDict):
    """Post a report as a comment on a pull request, once the owner approves the comment."""

    report: Annotated[
        str,
        Extra(
            pattern="^/reviews/[^/]+/report$",
            format="board-path",
            description="The report to post, by its logical path, such as /reviews/fix-sync/report.",
        ),
    ]
    pr: Annotated[
        str,
        Extra(
            pattern=r"^https://github\.com/[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+/pull/[0-9]+$",
            description="The pull request to comment on, by its URL, such as https://github.com/o/r/pull/7.",
        ),
    ]


class PostOutput(TypedDict):
    """The comment's URL, from the report's `source` when it was already posted, and whether this call posted it."""

    comment: str
    posted: bool


class CoveredInput(TypedDict):
    """Whether a committed report covers the commits a pull request adds to its base."""

    base: Annotated[
        str,
        Extra(
            minLength=1,
            description="The commit the pull request merges into, in any form git resolves, such as its base commit.",
        ),
    ]
    head: NotRequired[
        Annotated[
            str,
            Extra(
                minLength=1,
                description="The commit to check, in any form git resolves, such as a pull request's head commit;"
                " HEAD when left out.",
            ),
        ]
    ]


class CoveredOutput(TypedDict):
    """The commit checked, its merge base with the base, and the reports that cover the commits between.

    `skipped` counts the commits that only add review records and need no
    report, `commits` the rest, and `reports` lists the reports that cover at
    least one of them.
    """

    head: str
    merge_base: str
    skipped: int
    commits: int
    reports: list[str]
