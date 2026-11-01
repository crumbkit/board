"""The plugin's manifest, generated from its models (STANDARDS.md P3). The plugin ships no schema."""

from board_plugin.protocol import dumped
from pydantic import JsonValue

from board_inflight.models import NowInput, NowOutput

# `now` runs `changes.next` once per change it lists, each a plugin of its own, so it gets longer than 30 s.
TIMEOUT = 120


def manifest() -> dict[str, JsonValue]:
    """`board/plugin-manifest/v1` for this plugin."""
    return {
        "$type": "board/plugin-manifest/v1",
        "name": "inflight",
        "version": "0.1.0",
        "description": (
            "What is open or running right now: each open change with its worktree, holder, next step and actor, "
            "subagents, pull request and artifacts, then unattached subagents, questions waiting on the owner and "
            "open evals. Use it to see what is in flight, and after a usage stop to rescue it with "
            "`board --personal call inflight.now --stopped-at <time>`. README.md in the plugin's directory says "
            "how to rescue each change."
        ),
        "run": ["uv", "run", "--frozen", "--no-dev", "python", "-m", "board_inflight"],
        "metadata": {"area": "planning"},
        "types": [],
        "capabilities": [
            {
                "name": "now",
                "description": (
                    "List the open changes with their next step and actor, subagents, pull request and artifacts,"
                    " the unattached subagents, the open questions and the open evals, each part bounded."
                ),
                "inputSchema": NowInput.model_json_schema(by_alias=True),
                "outputSchema": NowOutput.model_json_schema(),
                "annotations": {"readOnlyHint": True},
                "_meta": {},
                "timeout": TIMEOUT,
            },
        ],
    }


if __name__ == "__main__":
    print(dumped(manifest()), end="")  # noqa: T201 - printing the manifest is this module's job
