"""The eval and run records, and the plugin's inputs and outputs.

The records' schemas, which the plugin ships, and the manifest's schemas are
generated from these models.
"""

import datetime
from collections.abc import Callable
from typing import Annotated, Any, Literal, Self, cast

from board_plugin.model import Strict
from pydantic import BaseModel, ConfigDict, Field, model_validator
from pydantic.config import JsonDict
from pydantic.json_schema import SkipJsonSchema

EVAL_TYPE = "evals/eval/v1"
RUN_TYPE = "evals/run/v1"

# The statuses of an eval that is still to be done or being done; `open` lists these.
OPEN = ("planned", "running")

# The most evals a capability lists, and its default `limit`. The cap keeps the output bounded (STANDARDS 23).
LIST_LIMIT = 100

# Codes that depend on the tree rather than the record. An eval holding one,
# such as a `cases_file` that reaches nothing yet, is still an eval to list.
TREE_CODES = ("board::ref::dangling", "board::ref::wrong_type")

# References Board resolves and checks. Each is a plain property, never an
# `anyOf` with null, because Board does not search `anyOf` for references.
FileRef = Annotated[str, Field(json_schema_extra={"format": "board-ref"})]
EvalRef = Annotated[str, Field(json_schema_extra={"format": "board-ref", "x-board-type": EVAL_TYPE})]

# What a case measures: a number, a label or a flag.
MeasureValue = float | str | bool

# A concluded eval has a verdict. pydantic cannot emit this itself.
CONCLUDED_NEEDS_VERDICT: JsonDict = {
    "if": {"properties": {"status": {"const": "concluded"}}, "required": ["status"]},
    "then": {"required": ["verdict"]},
}
# `about` asks about a subject or a repository, never both. Board checks this before the plugin runs.
SUBJECT_OR_REPO: JsonDict = {"oneOf": [{"required": ["subject"]}, {"required": ["repo"]}]}
# A result has an outcome, measures, or both.
OUTCOME_OR_MEASURES: JsonDict = {"anyOf": [{"required": ["outcome"]}, {"required": ["measures"]}]}


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


class Case(Record):
    """One case of an eval. A run's result names it by `id`."""

    model_config = ConfigDict(json_schema_extra=shipped())

    id: str
    input: Any = None
    expected: Any = None


class Measure(Record):
    """A quantity the eval takes. Each result and each run gives its value under `name`."""

    model_config = ConfigDict(json_schema_extra=shipped())

    name: str
    unit: str | SkipJsonSchema[None] = None
    description: str | SkipJsonSchema[None] = None


class EvalRecord(Record):
    """`evals/eval/v1`: a question about some subjects, how it is tested, and what was concluded.

    `subjects` are the names of what is tested, such as `crumbkit` or `clap`;
    versions go in a run's `versions`. `repos` references the mount of each
    repository the eval concerns, such as `/repos/crumbkit`. `cases` lists the
    cases inline, or `cases_file` references a file that does. `decision` is
    the decision this eval informs. A `concluded` eval has a `verdict`.
    """

    model_config = ConfigDict(json_schema_extra=shipped(CONCLUDED_NEEDS_VERDICT))

    tag: Literal["evals/eval/v1"] = Field(alias="$type")
    question: str
    subjects: list[str] = Field(
        min_length=1,
        description="The names of what is tested, such as `crumbkit` or `clap`, without versions. "
        "Name a repository in `repos`, by its mount.",
    )
    method: str
    status: Literal["planned", "running", "concluded", "abandoned"]
    repos: list[FileRef] | SkipJsonSchema[None] = None
    cases: list[Case] | SkipJsonSchema[None] = None
    cases_file: FileRef | SkipJsonSchema[None] = None
    measures: list[Measure] | SkipJsonSchema[None] = None
    decision: str | SkipJsonSchema[None] = None
    verdict: Literal["supported", "refuted", "inconclusive"] | SkipJsonSchema[None] = None
    conclusion: str | SkipJsonSchema[None] = None

    @model_validator(mode="after")
    def _concluded_has_a_verdict(self) -> Self:
        if self.status == "concluded" and self.verdict is None:
            message = "a concluded eval needs a verdict"
            raise ValueError(message)
        return self


class Result(Record):
    """What one case gave in a run. A scored case may give `measures` and no `outcome`.

    `trial` numbers repeated trials of one case, from 1.
    """

    model_config = ConfigDict(json_schema_extra=shipped(OUTCOME_OR_MEASURES))

    case: str
    outcome: Literal["pass", "fail", "error", "skipped"] | SkipJsonSchema[None] = None
    measures: dict[str, MeasureValue] | SkipJsonSchema[None] = None
    trial: Annotated[int, Field(ge=1)] | SkipJsonSchema[None] = None
    evidence: list[FileRef] | SkipJsonSchema[None] = None
    note: str | SkipJsonSchema[None] = None

    @model_validator(mode="after")
    def _has_an_outcome_or_measures(self) -> Self:
        if self.outcome is None and self.measures is None:
            message = "a result needs an outcome or measures"
            raise ValueError(message)
        return self


class RunRecord(Record):
    """`evals/run/v1`: one run of an eval, by whom, when, with what versions, and what each case gave.

    `versions` maps what was used, such as a model, a prompt or a library, to
    its version. `measures` holds aggregates over the whole run.
    """

    model_config = ConfigDict(json_schema_extra=shipped())

    tag: Literal["evals/run/v1"] = Field(alias="$type")
    eval: EvalRef
    by: str
    started: datetime.datetime
    results: list[Result]
    finished: datetime.datetime | SkipJsonSchema[None] = None
    setup: str | SkipJsonSchema[None] = None
    versions: dict[str, str] | SkipJsonSchema[None] = None
    measures: dict[str, MeasureValue] | SkipJsonSchema[None] = None
    summary: str | SkipJsonSchema[None] = None


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
