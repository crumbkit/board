"""The question, answer and decision records, and the plugin's input and output.

The records' schemas, which the plugin ships, and the manifest's schemas are
generated from these models.
"""

from collections.abc import Callable
from typing import Annotated, Literal, cast

from board_plugin.model import Strict
from pydantic import AwareDatetime, BaseModel, ConfigDict, Field
from pydantic.config import JsonDict
from pydantic.json_schema import SkipJsonSchema

from board_plans.open import OPEN_LIMIT, QUESTION_TYPE

ANSWER_TYPE = "plans/answer/v1"
DECISION_TYPE = "plans/decision/v1"

# The most questions `open` returns.
OPEN_MAX = 10_000

# A decision a person or an agent writes says why. A stub a hook writes may
# leave `why` empty for the agent to fill in on its next turn.
WHY_UNLESS_WRITTEN_BY: JsonDict = {
    "if": {"not": {"required": ["written_by"]}},
    "then": {"properties": {"why": {"minLength": 1}}},
}

QuestionRef = Annotated[str, Field(json_schema_extra={"format": "board-ref", "x-board-type": QUESTION_TYPE})]
FileRef = Annotated[str, Field(json_schema_extra={"format": "board-ref"})]
WrittenBy = Annotated[
    str,
    Field(
        pattern=r"^[a-z0-9-]+/[a-z0-9-]+$",
        description=(
            "The plugin and hook that wrote this record, such as runs/pr-create; absent when an agent or a "
            "person wrote it."
        ),
    ),
]


def shipped(extra: JsonDict | None = None) -> Callable[[JsonDict], None]:
    """A schema hook: no other field, no `null` default for a field typed otherwise, and `extra` keywords."""

    def hook(schema: JsonDict) -> None:
        schema["additionalProperties"] = False
        for field in cast("dict[str, JsonDict]", schema.get("properties", {})).values():
            field.pop("default", None)
        schema.update(extra or {})

    return hook


class Record(BaseModel):
    """A record this plugin ships a schema for."""

    model_config = ConfigDict(frozen=True)


class Option(Record):
    """One answer the owner may give. An answer's `choice` and the question's `recommendation` name it by `id`."""

    model_config = ConfigDict(json_schema_extra=shipped())

    id: str = Field(min_length=1)
    text: str = Field(min_length=1)


class QuestionRecord(Record):
    """`plans/question/v1`: a question an agent session needs the owner to answer.

    It lives at `/projects/<project>/questions/<id>`, so its path names its
    project. `<id>` is a sortable UTC time and a short random suffix, such as
    `20261024T1830Z-k3`. Its answer is a `plans/answer/v1` at
    `/projects/<project>/answers/<id>`, with the same id. `session` is the
    asking session's `local_` id, so the answer can be sent back to it. `repo`
    names the repository, such as `crumbkit/viitta`, when there is one.
    `recommendation` is the `id` of one of `options`, and `why` gives the reason.
    `blocks` says what cannot proceed until the question is answered. `status`
    is `open` until an answer is recorded, then `answered`; `withdrawn` means
    the question no longer matters. `context_path` references a Markdown file
    in the board that holds long background. `note` records what the asker did
    not know, such as details nobody has written down yet. Never put a secret
    value in a question.
    """

    model_config = ConfigDict(json_schema_extra=shipped())

    tag: Literal["plans/question/v1"] = Field(alias="$type")
    question: str = Field(min_length=1)
    session: str = Field(pattern=r"^local_[0-9a-f-]+$")
    asked: AwareDatetime
    status: Literal["open", "answered", "withdrawn"]
    repo: str | SkipJsonSchema[None] = None
    options: list[Option] | SkipJsonSchema[None] = None
    recommendation: str | SkipJsonSchema[None] = None
    why: str | SkipJsonSchema[None] = None
    blocks: str | SkipJsonSchema[None] = None
    context_path: FileRef | SkipJsonSchema[None] = None
    note: str | SkipJsonSchema[None] = None


class Evidence(Record):
    """Where the owner gave the answer.

    `session` is the Claude Code session id of the transcript the owner typed
    in, `entry` is that turn's uuid, and `quote` is the owner's text exactly as
    it appears in the entry. Nothing checks it yet; it is written so that a
    later check can.
    """

    model_config = ConfigDict(json_schema_extra=shipped())

    session: str
    entry: str
    quote: str


class AnswerRecord(Record):
    """`plans/answer/v1`: the owner's answer to a `plans/question/v1`.

    It lives at `/projects/<project>/answers/<id>`, with its question's id, so a
    question has one answer. `question` references the question. `choice` is
    the `id` of the option the owner chose, or `none` when the owner's words
    choose no option. `words` holds the owner's words verbatim. `answered` is
    when the owner gave them. `by` names the session that wrote the record,
    which may differ from the session that asked. Never put a secret value in
    an answer.
    """

    model_config = ConfigDict(json_schema_extra=shipped())

    tag: Literal["plans/answer/v1"] = Field(alias="$type")
    question: QuestionRef
    choice: str = Field(min_length=1)
    words: str = Field(min_length=1)
    answered: AwareDatetime
    by: str | SkipJsonSchema[None] = None
    evidence: Evidence | SkipJsonSchema[None] = None


class DecisionRecord(Record):
    """`plans/decision/v1`: a decision a session made, and why.

    `decision` states it and `why` gives the reason; `alternatives` lists what
    was not chosen. `session` names the session that decided and `decided` is
    when. `refs` names what the decision concerns, such as a pull request's URL
    or a record's path. `review` is `wanted` when the owner should look at it.
    `written_by` names the plugin and hook that wrote a stub without an agent,
    such as `runs/pr-create`; only such a stub may leave `why` empty, for
    the agent to fill in.
    """

    model_config = ConfigDict(json_schema_extra=shipped(WHY_UNLESS_WRITTEN_BY))

    tag: Literal["plans/decision/v1"] = Field(alias="$type")
    decision: str
    why: str
    session: str
    decided: AwareDatetime
    alternatives: list[str] | SkipJsonSchema[None] = None
    refs: list[str] | SkipJsonSchema[None] = None
    review: Literal["wanted", "none"] | SkipJsonSchema[None] = None
    written_by: WrittenBy | SkipJsonSchema[None] = None


class OpenInput(Strict):
    """The open questions under `path`, oldest first."""

    path: str = Field(
        default="/",
        pattern="^/",
        json_schema_extra={"format": "board-path"},
        description=("Only questions below this logical path, such as /projects/board; every question when left out."),
    )
    limit: int = Field(
        default=OPEN_LIMIT,
        ge=1,
        le=OPEN_MAX,
        description=f"The most questions to return, 1 to {OPEN_MAX:,}; {OPEN_LIMIT} when left out.",
    )


class OpenQuestion(Strict):
    """An open question, by path, with what the owner needs to answer it."""

    path: str
    question: str
    asked: AwareDatetime
    session: str
    options: list[Option]
    recommendation: str | None
    blocks: str | None


class OpenOutput(Strict):
    """Open questions, oldest first.

    `total` counts every readable open question under the path and `truncated` says
    whether the limit left some out. `unreadable` is how many open questions
    were left out because Board marks them degraded or this plugin cannot read
    them.
    """

    questions: list[OpenQuestion]
    total: int
    truncated: bool
    unreadable: int
