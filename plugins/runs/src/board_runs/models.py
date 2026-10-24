"""The suite and run records, and the plugin's inputs and outputs.

The records' schemas, which the plugin ships, and the manifest's schemas are
generated from these models.
"""

import datetime
from collections.abc import Callable
from typing import Annotated, Any, Literal, Self, cast

from board_plugin.model import Strict
from pydantic import AwareDatetime, BaseModel, ConfigDict, Field, model_validator
from pydantic.config import JsonDict
from pydantic.json_schema import SkipJsonSchema

SUITE_TYPE = "runs/suite/v1"
RUN_TYPE = "runs/run/v1"

# The most points `trend` returns, and its default `limit`. The cap keeps the output bounded (STANDARDS 23).
TREND_LIMIT = 1000

# Codes that depend on the tree rather than the record. A run holding one is still a run to read.
TREE_CODES = ("board::ref::dangling", "board::ref::wrong_type")

# The outcomes of a case that did not pass. A run records only these, each with a note.
NOT_PASSED = ("fail", "error", "skipped")
# The outcomes that make a run fail.
FAILED = ("fail", "error")

# References Board resolves and checks. Each is a plain property, never an
# `anyOf` with null, because Board does not search `anyOf` for references.
FileRef = Annotated[str, Field(json_schema_extra={"format": "board-ref"})]
SuiteRef = Annotated[str, Field(json_schema_extra={"format": "board-ref", "x-board-type": SUITE_TYPE})]

# A suite named by its logical path, which `board call` reads relative to the prefix.
SuitePath = Annotated[
    str,
    Field(
        pattern="^/",
        json_schema_extra={"format": "board-path"},
        description="The suite's logical path, such as /suites/board-bench, here or in a mounted board.",
    ),
]

# What a case or a run measures: a number, a label or a flag.
MeasureValue = int | float | str | bool

# A suite lists its cases inline or in a file, not both. pydantic cannot emit this itself.
NOT_BOTH_CASES: JsonDict = {"not": {"required": ["cases", "cases_file"]}}
# A result has an outcome, measures, or both, and a case that did not pass has a note.
RESULT_RULES: JsonDict = {
    "anyOf": [{"required": ["outcome"]}, {"required": ["measures"]}],
    "if": {"properties": {"outcome": {"enum": list(NOT_PASSED)}}, "required": ["outcome"]},
    "then": {"required": ["note"]},
}


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
    """One case of a suite. A run's result names it by `id`."""

    model_config = ConfigDict(json_schema_extra=shipped())

    id: str
    input: Any = None
    expected: Any = None


class Measure(Record):
    """A quantity a run takes, in `unit`. A result or a run gives its value under `name`."""

    model_config = ConfigDict(json_schema_extra=shipped())

    name: str
    unit: str
    description: str | SkipJsonSchema[None] = None


class SuiteRecord(Record):
    """`runs/suite/v1`: what gets run, how, and what a run of it measures.

    A suite's name is its path: the suite at `/suites/<name>.json` is
    `<name>`, and its runs sit in `/suites/<name>/runs/`. `subjects` are the
    names of what is tested. `cases` lists the cases inline, or `cases_file`
    references a file that does; a suite whose cases are found when it runs,
    such as a test suite, has neither.
    """

    model_config = ConfigDict(json_schema_extra=shipped(NOT_BOTH_CASES))

    tag: Literal["runs/suite/v1"] = Field(alias="$type")
    subjects: list[str] = Field(min_length=1)
    method: str
    command: str | SkipJsonSchema[None] = None
    cases: list[Case] | SkipJsonSchema[None] = None
    cases_file: FileRef | SkipJsonSchema[None] = None
    measures: list[Measure] | SkipJsonSchema[None] = None

    @model_validator(mode="after")
    def _not_both_cases(self) -> Self:
        if self.cases is not None and self.cases_file is not None:
            message = "a suite lists its cases in cases or cases_file, not both"
            raise ValueError(message)
        return self


class Host(Record):
    """The machine a run ran on."""

    model_config = ConfigDict(json_schema_extra=shipped())

    os: str
    arch: str
    logical_cpus: int = Field(ge=1)


class Result(Record):
    """What one case gave in a run. A case that did not pass has a `note`.

    `trial` numbers repeated trials of one case, from 1. `evidence` references files in the board that back the
    result, such as a log an eval kept.
    """

    model_config = ConfigDict(json_schema_extra=shipped(RESULT_RULES))

    case: str
    outcome: Literal["pass", "fail", "error", "skipped"] | SkipJsonSchema[None] = None
    measures: dict[str, MeasureValue] | SkipJsonSchema[None] = None
    note: str | SkipJsonSchema[None] = None
    trial: Annotated[int, Field(ge=1)] | SkipJsonSchema[None] = None
    evidence: list[FileRef] | SkipJsonSchema[None] = None

    @model_validator(mode="after")
    def _rules(self) -> Self:
        if self.outcome is None and self.measures is None:
            message = "a result needs an outcome or measures"
            raise ValueError(message)
        if self.outcome in NOT_PASSED and self.note is None:
            message = "a case that did not pass needs a note"
            raise ValueError(message)
        return self


class RunRecord(Record):
    """`runs/run/v1`: one run of a suite, by whom, when, on what, and what it gave.

    `scope` is `full` when the run ran every case and `targeted` when it ran
    some. `dirty` says whether the working tree under test had uncommitted
    changes; a run that does not say is never counted as clean. `versions`
    maps what was used, such as a commit, a model or a library, to its
    version. `setup` says how this run was prepared, and `summary` what it
    showed. `measures` holds the run's own measures, such as `tests` and
    `failures`. A test suite's run lists only the cases that did not pass and
    the cases that measured something; an eval's run may list every case.
    """

    model_config = ConfigDict(json_schema_extra=shipped())

    tag: Literal["runs/run/v1"] = Field(alias="$type")
    suite: SuiteRef
    by: str
    started: AwareDatetime
    scope: Literal["full", "targeted"]
    results: list[Result]
    dirty: bool | SkipJsonSchema[None] = None
    finished: AwareDatetime | SkipJsonSchema[None] = None
    versions: dict[str, str] | SkipJsonSchema[None] = None
    host: Host | SkipJsonSchema[None] = None
    measures: dict[str, MeasureValue] | SkipJsonSchema[None] = None
    setup: str | SkipJsonSchema[None] = None
    summary: str | SkipJsonSchema[None] = None


class LatestInput(Strict):
    """The latest full run of a clean revision of `suite`."""

    suite: SuitePath


class Latest(Strict):
    """A run, by path, and what it gave. `outcome` is `fail` when a result failed or erred, else `pass`.

    `age_s` is the whole seconds since the run finished, or since it started when it records no finish.
    """

    path: str
    started: datetime.datetime
    finished: datetime.datetime | None
    outcome: Literal["pass", "fail"]
    measures: dict[str, MeasureValue]
    failed: list[str]
    age_s: int


class LatestOutput(Strict):
    """The latest qualifying run, or `null` when none qualifies.

    `not_counted` is how many runs of the suite were left out because they were targeted, dirty or did not say.
    `unreadable` is how many were left out because Board marks them degraded for anything but a broken reference,
    or this plugin cannot read them.
    """

    run: Latest | None
    not_counted: int
    unreadable: int


class TrendInput(Strict):
    """One measure over the full runs of a clean revision of `suite`, newest first."""

    suite: SuitePath
    measure: str = Field(min_length=1, description="The measure's name, such as median_ms.")
    case: str | SkipJsonSchema[None] = Field(
        default=None,
        min_length=1,
        description="The case whose measure to read, such as get; the run's own measure when left out.",
    )
    limit: int = Field(
        default=TREND_LIMIT,
        ge=1,
        le=TREND_LIMIT,
        description=f"The most points to return, 1 to {TREND_LIMIT}; {TREND_LIMIT} when left out.",
    )


class Point(Strict):
    """One run's value of the measure."""

    path: str
    started: datetime.datetime
    value: float


class TrendOutput(Strict):
    """Points, newest first.

    `total` counts every point and `truncated` says whether the limit left some out. `without_measure` is how many
    qualifying runs give no number for the measure. `not_counted` is how many runs were targeted, dirty or did not
    say, and `unreadable` how many Board marks degraded or this plugin cannot read.
    """

    points: list[Point]
    total: int
    truncated: bool
    without_measure: int
    not_counted: int
    unreadable: int
