"""The plugin's manifest and the schemas it ships, generated from its models (STANDARDS.md P3)."""

from board_plugin.protocol import dumped
from pydantic import JsonValue

from board_plans.models import (
    ANSWER_TYPE,
    DECISION_TYPE,
    QUESTION_TYPE,
    AnswerRecord,
    DecisionRecord,
    OpenInput,
    OpenOutput,
    QuestionRecord,
)


def manifest() -> dict[str, JsonValue]:
    """`board/plugin-manifest/v1` for this plugin."""
    return {
        "$type": "board/plugin-manifest/v1",
        "name": "plans",
        "version": "0.1.0",
        "description": (
            "Questions an agent session asks the owner, the owner's answers, and the decisions sessions record. "
            "`board call plans.open` lists the questions still waiting for an answer, oldest first. "
            "README.md in the plugin's directory says where each record lives."
        ),
        "run": ["uv", "run", "--frozen", "--no-dev", "python", "-m", "board_plans"],
        "metadata": {"area": "planning"},
        "types": [QUESTION_TYPE, ANSWER_TYPE, DECISION_TYPE],
        "capabilities": [
            {
                "name": "open",
                "description": "The questions under a path whose status is open, oldest first.",
                "inputSchema": OpenInput.model_json_schema(),
                "outputSchema": OpenOutput.model_json_schema(),
                "annotations": {"readOnlyHint": True},
                "_meta": {},
            },
        ],
    }


def schemas() -> dict[str, dict[str, JsonValue]]:
    """Each type's schema, as the plugin ships it, by its path under `types/`."""
    return {
        "plans/question/v1.schema.json": QuestionRecord.model_json_schema(by_alias=True),
        "plans/answer/v1.schema.json": AnswerRecord.model_json_schema(by_alias=True),
        "plans/decision/v1.schema.json": DecisionRecord.model_json_schema(by_alias=True),
    }


if __name__ == "__main__":
    print(dumped(manifest()), end="")  # noqa: T201 - printing the manifest is this module's job
