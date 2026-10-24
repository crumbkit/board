"""The eval record, and the plugin's inputs and outputs.

The record's schema, which the plugin ships, and the manifest's schemas are
generated from these models.
"""

from collections.abc import Callable
from typing import Annotated, Literal, Self, cast

from board_plugin.model import Strict
from pydantic import BaseModel, ConfigDict, Field, model_validator
from pydantic.config import JsonDict
from pydantic.json_schema import SkipJsonSchema

EVAL_TYPE = "evals/eval/v1"
# The types an eval refers to, which the runs plugin ships.
SUITE_TYPE = "runs/suite/v1"
RUN_TYPE = "runs/run/v1"

# The statuses of an eval that is still to be done or being done; `open` lists these.
OPEN = ("planned", "running")

# The most evals a capability lists, and its default `limit`. The cap keeps the output bounded (STANDARDS 23).
LIST_LIMIT = 100

# Codes that depend on the tree rather than the record. An eval holding one,
# such as a run that is not written yet, is still an eval to list.
TREE_CODES = ("board::ref::dangling", "board::ref::wrong_type")

# References Board resolves and checks. Each is a plain property, never an
# `anyOf` with null, because Board does not search `anyOf` for references.
FileRef = Annotated[str, Field(json_schema_extra={"format": "board-ref"})]
SuiteRef = Annotated[str, Field(json_schema_extra={"format": "board-ref", "x-board-type": SUITE_TYPE})]
RunRef = Annotated[str, Field(json_schema_extra={"format": "board-ref", "x-board-type": RUN_TYPE})]

# A concluded eval has a verdict. pydantic cannot emit this itself.
CONCLUDED_NEEDS_VERDICT: JsonDict = {
    "if": {"properties": {"status": {"const": "concluded"}}, "required": ["status"]},
    "then": {"required": ["verdict"]},
}
# `about` asks about a subject or a repository, never both. Board checks this before the plugin runs.
SUBJECT_OR_REPO: JsonDict = {"oneOf": [{"required": ["subject"]}, {"required": ["repo"]}]}


def shipped(extra: JsonDict | None = None) -> Callable[[JsonDict], None]:
    """A schema hook: no other field, no `null` default for a field typed otherwise, and `extra` keywords."""

    def hook(schema: JsonDict) -> None:
        schema["additionalProperties"] = False
        for field in cast("dict[str, JsonDict]", schema.get("properties", {})).values():
            field.pop("default", None)
        schema.update(extra or {})

    return hook


class Record(BaseModel):
    """A record this plugin reads and ships a schema for.

    The schema refuses any other field, but a board may keep a looser schema
    of its own (STANDARDS.md 3), so the plugin reads a record ignoring the rest.
    """

    model_config = ConfigDict(extra="ignore", frozen=True)


class EvalRecord(Record):
    """`evals/eval/v1`: a question about some subjects, how it is tested, what it rests on, and what was concluded.

    `subjects` are the names of what is tested, such as `crumbkit` or `clap`;
    versions go in a run's `versions`. `repos` references the mount of each
    repository the eval concerns, such as `/repos/crumbkit`. `suites`
    references the `runs/suite/v1` records that say how it is tested, and
    `runs` the `runs/run/v1` records its verdict rests on. `decision` is the
    decision this eval informs. A `concluded` eval has a `verdict`.
    """

    model_config = ConfigDict(json_schema_extra=shipped(CONCLUDED_NEEDS_VERDICT))

    tag: Literal["evals/eval/v1"] = Field(alias="$type")
    question: str
    subjects: list[str] = Field(
        min_length=1,
        description="The names of what is tested, such as `crumbkit` or `clap`, without versions. "
        "Name a repository in `repos`, by its mount.",
    )
    status: Literal["planned", "running", "concluded", "abandoned"]
    repos: list[FileRef] | SkipJsonSchema[None] = None
    suites: list[SuiteRef] | SkipJsonSchema[None] = None
    runs: list[RunRef] | SkipJsonSchema[None] = None
    decision: str | SkipJsonSchema[None] = None
    verdict: Literal["supported", "refuted", "inconclusive"] | SkipJsonSchema[None] = None
    conclusion: str | SkipJsonSchema[None] = None

    @model_validator(mode="after")
    def _concluded_has_a_verdict(self) -> Self:
        if self.status == "concluded" and self.verdict is None:
            message = "a concluded eval needs a verdict"
            raise ValueError(message)
        return self


class AboutInput(Strict):
    """The evals that name this `subject` or this `repo`, at most `limit` of them.

    A subject matches ignoring case for ASCII letters. A repo is a mount's path, such as `/repos/crumbkit`. It matches
    an eval whose `repos` include that exact path, or whose own path lies under it.
    """

    model_config = ConfigDict(json_schema_extra=SUBJECT_OR_REPO)

    subject: str | SkipJsonSchema[None] = Field(
        default=None,
        min_length=1,
        description="The name of what is tested, such as crumbkit or clap, matched ignoring ASCII case;"
        " give subject or repo, not both.",
    )
    repo: str | SkipJsonSchema[None] = Field(
        default=None,
        min_length=1,
        description="A repository's mount, such as /repos/crumbkit, to list the evals that name it or live in it;"
        " give subject or repo, not both.",
    )
    limit: int = Field(
        default=LIST_LIMIT,
        ge=1,
        le=LIST_LIMIT,
        description=f"The most evals to list, 1 to {LIST_LIMIT}; {LIST_LIMIT} when left out.",
    )


class OpenInput(Strict):
    """The evals still to do or being done, at most `limit` of them."""

    limit: int = Field(
        default=LIST_LIMIT,
        ge=1,
        le=LIST_LIMIT,
        description=f"The most evals to list, 1 to {LIST_LIMIT}; {LIST_LIMIT} when left out.",
    )


class Listed(Strict):
    """An eval, by its path in the board holding it, which may be a mounted one."""

    path: str
    question: str
    subjects: list[str]
    status: Literal["planned", "running", "concluded", "abandoned"]
    verdict: Literal["supported", "refuted", "inconclusive"] | None
    decision: str | None


class ListOutput(Strict):
    """Evals by path. `total` counts every match, and `truncated` says whether the limit left some out."""

    evals: list[Listed]
    total: int
    truncated: bool
