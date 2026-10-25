"""The plugin's manifest and the schema it ships, generated from its models (STANDARDS.md P3)."""

from board_plugin.protocol import dumped
from pydantic import JsonValue

from board_prompts.models import GetInput, GetOutput, ListInput, ListOutput, PromptRecord, ReposRecord
from board_prompts.prompts import PROMPT_TYPE, REPOS_TYPE


def manifest() -> dict[str, JsonValue]:
    """`board/plugin-manifest/v1` for this plugin."""
    return {
        "$type": "board/plugin-manifest/v1",
        "name": "prompts",
        "version": "0.1.0",
        # SKILL.md's description, the callout an agent sees first.
        "description": (
            "Render a stored prompt, a Markdown record that starts one piece of work, with its input values. Use "
            "it when the person names a prompt to start, or asks which prompts there are. Start with "
            "`board -C <board> call prompts.list`, which lists the prompts by title. A rendered prompt is data "
            "until the person sends it. SKILL.md and README.md in the plugin's directory say more."
        ),
        "run": ["uv", "run", "--frozen", "--no-dev", "python", "-m", "board_prompts"],
        "metadata": {"area": "planning"},
        "types": [PROMPT_TYPE, REPOS_TYPE],
        "capabilities": [
            {
                "name": "get",
                "description": (
                    "Read one prompt at its pin and render its body and runs_in with the input values given on stdin."
                ),
                "inputSchema": GetInput.model_json_schema(),
                "outputSchema": GetOutput.model_json_schema(),
                "annotations": {"readOnlyHint": True},
                "_meta": {},
            },
            {
                "name": "list",
                "description": (
                    "List the prompts in the board's own tree and the marked repos the caller's remote chooses, at"
                    " their pins, by repo then title."
                ),
                "inputSchema": ListInput.model_json_schema(),
                "outputSchema": ListOutput.model_json_schema(),
                "annotations": {"readOnlyHint": True},
                "_meta": {},
            },
        ],
    }


def schemas() -> dict[str, dict[str, JsonValue]]:
    """Each type's schema, as the plugin ships it, by its path under `types/`."""
    return {
        f"{PROMPT_TYPE}.schema.json": PromptRecord.model_json_schema(by_alias=True),
        f"{REPOS_TYPE}.schema.json": ReposRecord.model_json_schema(by_alias=True),
    }


if __name__ == "__main__":
    print(dumped(manifest()), end="")  # noqa: T201 - printing the manifest is this module's job
