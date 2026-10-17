"""The plugin's manifest and the schema it ships, generated from its shapes (STANDARDS.md P3)."""

from board_plugin.protocol import JsonValue, dumped
from pydantic import TypeAdapter
from pydantic.json_schema import GenerateJsonSchema, JsonSchemaValue
from pydantic_core import core_schema

from board_controls.shapes import (
    CONTROL_TYPE,
    CheckInput,
    CheckOutput,
    ControlRecord,
    ForPathInput,
    ForPathOutput,
)


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
        "name": "controls",
        "version": "0.1.0",
        # SKILL.md's description, the callout an agent sees first.
        "description": (
            "The controls plugin keeps requirements on files as records in the board that holds the files. "
            "Use it when you start work in a board or a repository it mounts, and before you edit a file a "
            'control may govern. Start with `board -C <board> call controls.check` with `{"path": '
            '"/<directory>"}` on stdin, which lists the budgets it measures and the written requirements '
            'under that path. `board -C <board> call controls.for-path` with `{"path": "/<file>"}` on stdin '
            "lists the controls on one file. SKILL.md and README.md in the plugin's directory say more."
        ),
        "run": [".venv/bin/python", "-m", "board_controls"],
        "metadata": {"area": "governance"},
        "types": [CONTROL_TYPE],
        "capabilities": [
            {
                "name": "for-path",
                "description": "The controls that apply to a path.",
                "inputSchema": _schema(ForPathInput),
                "outputSchema": _schema(ForPathOutput),
                "annotations": {"readOnlyHint": True},
                "_meta": {},
            },
            {
                "name": "check",
                "description": "Check the controls under a path, `/` when left out."
                " Fails naming each control over budget or degraded.",
                "inputSchema": _schema(CheckInput),
                "outputSchema": _schema(CheckOutput),
                "annotations": {"readOnlyHint": True},
                "_meta": {},
            },
        ],
    }


def schema() -> dict[str, JsonValue]:
    """The schema of `controls/control/v1`, as the plugin ships it."""
    return _schema(ControlRecord)


if __name__ == "__main__":
    print(dumped(manifest()), end="")  # noqa: T201 - printing the manifest is this module's job
