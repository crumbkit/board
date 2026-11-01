"""The plugin's manifest and the schemas it ships, generated from its shapes (STANDARDS.md P3)."""

from board_plugin.protocol import JsonValue, dumped
from pydantic import TypeAdapter
from pydantic.json_schema import GenerateJsonSchema, JsonSchemaValue
from pydantic_core import core_schema

from board_boundary.shapes import (
    AGENT_VARIABLES,
    BOUNDARY_TYPE,
    CHECK_TIMEOUT,
    CROSSING_TYPE,
    BoundaryRecord,
    CheckInput,
    CheckOutput,
    CoversInput,
    CoversOutput,
    CrossingRecord,
    RecordInput,
    RecordOutput,
    VerifyInput,
    VerifyOutput,
)


class _Closed(GenerateJsonSchema):
    """Schemas that refuse fields they do not name, and whose properties carry a description and no title."""

    def typed_dict_schema(self, schema: core_schema.TypedDictSchema) -> JsonSchemaValue:
        """The schema pydantic generates for a TypedDict, closed."""
        return super().typed_dict_schema(schema) | {"additionalProperties": False}

    def field_title_should_be_set(self, schema: object) -> bool:  # noqa: ARG002 - no property has a title
        """Never: the record shape rules give a property a description and no title."""
        return False


def _schema(shape: object) -> dict[str, JsonValue]:
    return TypeAdapter(shape).json_schema(schema_generator=_Closed)


def manifest() -> dict[str, JsonValue]:
    """`board/plugin-manifest/v1` for this plugin."""
    env: list[JsonValue] = [*AGENT_VARIABLES]
    return {
        "$type": "board/plugin-manifest/v1",
        "name": "boundary",
        "version": "0.1.0",
        # SKILL.md's description, the callout an agent sees first.
        "description": (
            "The boundary plugin checks content, such as a pull request body, before it leaves a checkout, against "
            "the controls that boundaries in the personal board name for its destination. Use it before you send "
            "anything a boundary may cover. Run `board --personal call boundary.check` with the action, destination, "
            "checkout and parts on stdin; on `boundary::check::failed`, change what it names and check again. "
            "SKILL.md and README.md in the plugin's directory say more."
        ),
        "run": [".venv/bin/python", "-m", "board_boundary"],
        "metadata": {"area": "governance"},
        "types": [BOUNDARY_TYPE, CROSSING_TYPE],
        "capabilities": [
            {
                "name": "check",
                "description": "Check content against every boundary that matches where it goes, and record the"
                " crossing in the personal board. Fails naming each failing control, part and line.",
                "inputSchema": _schema(CheckInput),
                "outputSchema": _schema(CheckOutput),
                "annotations": {"readOnlyHint": False},
                "_meta": {},
                "timeout": CHECK_TIMEOUT,
                "env": env,
            },
            {
                "name": "covers",
                "description": "Name the boundaries that cover an action to a destination from a checkout, checking"
                " nothing and writing nothing.",
                "inputSchema": _schema(CoversInput),
                "outputSchema": _schema(CoversOutput),
                "annotations": {"readOnlyHint": True},
                "_meta": {},
            },
            {
                "name": "verify",
                "description": "Whether a passing crossing exists for a digest, or for given content.",
                "inputSchema": _schema(VerifyInput),
                "outputSchema": _schema(VerifyOutput),
                "annotations": {"readOnlyHint": True},
                "_meta": {},
            },
            {
                "name": "record",
                "description": "Add what the destination returned, such as a URL, to a passing crossing.",
                "inputSchema": _schema(RecordInput),
                "outputSchema": _schema(RecordOutput),
                "annotations": {"readOnlyHint": False},
                "_meta": {},
            },
        ],
    }


def schemas() -> dict[str, dict[str, JsonValue]]:
    """The schema of each record type, as the plugin ships it, by type."""
    return {BOUNDARY_TYPE: _schema(BoundaryRecord), CROSSING_TYPE: _schema(CrossingRecord)}


if __name__ == "__main__":
    print(dumped(manifest()), end="")  # noqa: T201 - printing the manifest is this module's job
