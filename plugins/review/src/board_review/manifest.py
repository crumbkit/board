"""The plugin's manifest and the schemas it ships, generated from its shapes (STANDARDS.md P3)."""

from board_plugin.protocol import JsonValue, dumped
from pydantic import TypeAdapter
from pydantic.json_schema import GenerateJsonSchema, JsonSchemaValue
from pydantic_core import core_schema

from board_review.shapes import (
    AGENT_VARIABLE,
    BRIEF_TARGET,
    BRIEF_TYPE,
    POST_TIMEOUT,
    REPORT_TARGET,
    REPORT_TYPE,
    RUN_TIMEOUT,
    BriefRecord,
    CoveredInput,
    CoveredOutput,
    PostInput,
    PostOutput,
    ReportRecord,
    RunInput,
    RunOutput,
)

# What `run` reads: the agent to start, and what that agent may read, besides what Board passes every plugin.
RUN_ENV = [AGENT_VARIABLE, "ANTHROPIC_API_KEY", "CLAUDE_CODE_OAUTH_TOKEN"]
# What `post` passes gh: its token, or the directory that holds its login.
POST_ENV = ["GH_TOKEN", "GITHUB_TOKEN", "GH_CONFIG_DIR"]


class _Closed(GenerateJsonSchema):
    """Schemas that refuse fields they do not name."""

    def typed_dict_schema(self, schema: core_schema.TypedDictSchema) -> JsonSchemaValue:
        """The schema pydantic generates for a TypedDict, closed."""
        return super().typed_dict_schema(schema) | {"additionalProperties": False}


def _schema(shape: object) -> dict[str, JsonValue]:
    return TypeAdapter(shape).json_schema(schema_generator=_Closed)


def manifest() -> dict[str, JsonValue]:
    """`board/plugin-manifest/v1` for this plugin."""
    return {
        "$type": "board/plugin-manifest/v1",
        "name": "review",
        "version": "0.2.0",
        "description": (
            "Review briefs, the reports a clean-context agent or a person writes for them, and whether a commit is"
            " covered by reports. `board call review.run --brief /reviews/<id>/brief` reviews the brief's range or"
            " pinned record with the agent BOARD_REVIEW_AGENT names and records the report; `board call"
            " review.post --report /reviews/<id>/report --pr <url>` comments it on a pull request once the owner"
            " approves the comment; `board call review.covered --base <commit> --head <commit>` checks that"
            " committed reports cover a pull request's commits. README.md in the plugin's directory says more."
        ),
        "run": [".venv/bin/python", "-m", "board_review"],
        "metadata": {"area": "review"},
        "types": [BRIEF_TYPE, REPORT_TYPE],
        "capabilities": [
            {
                "name": "run",
                "description": (
                    "Review the range or pinned record a brief names with the agent the BOARD_REVIEW_AGENT variable"
                    " names, and write the report beside the brief."
                ),
                "inputSchema": _schema(RunInput),
                "outputSchema": _schema(RunOutput),
                "_meta": {},
                "timeout": RUN_TIMEOUT,
                "env": list[JsonValue](RUN_ENV),
            },
            {
                "name": "post",
                "description": (
                    "Comment a report on a GitHub pull request with gh, once the owner approves the whole comment"
                    " through `board decide`, and set the report's source to the comment's URL. A report whose"
                    " source is set is skipped."
                ),
                "inputSchema": _schema(PostInput),
                "outputSchema": _schema(PostOutput),
                "_meta": {},
                "timeout": POST_TIMEOUT,
                "env": list[JsonValue](POST_ENV),
            },
            {
                "name": "covered",
                "description": (
                    "The reports in the board's own tree that cover every commit from the merge base of a base"
                    " and a head to the head, each by a report's range or patch ids, after skipping commits that"
                    " only add review records. A report asking for changes never covers. Fails when a commit is"
                    " uncovered."
                ),
                "inputSchema": _schema(CoveredInput),
                "outputSchema": _schema(CoveredOutput),
                "annotations": {"readOnlyHint": True},
                "_meta": {},
            },
        ],
    }


def schemas() -> dict[str, dict[str, JsonValue]]:
    """The schema of each type the plugin ships, by its path in the plugin's directory."""
    return {
        f"types/{BRIEF_TYPE}.schema.json": _schema(BriefRecord) | BRIEF_TARGET,
        f"types/{REPORT_TYPE}.schema.json": _schema(ReportRecord) | REPORT_TARGET,
    }


if __name__ == "__main__":
    print(dumped(manifest()), end="")  # noqa: T201 - printing the manifest is this module's job
