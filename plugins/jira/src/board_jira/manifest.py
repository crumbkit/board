"""The plugin's manifest and the schemas it ships, generated from its models (STANDARDS.md P3)."""

from board_plugin.protocol import dumped
from pydantic import JsonValue

from board_jira.models import IssueRecord, RefInput, RefOutput
from board_jira.ref import ISSUE_TYPE


def manifest() -> dict[str, JsonValue]:
    """`board/plugin-manifest/v1` for this plugin."""
    return {
        "$type": "board/plugin-manifest/v1",
        "name": "jira",
        "version": "0.1.0",
        # SKILL.md's description, the callout an agent sees first.
        "description": (
            "Record a Jira issue as a typed jira/issue/v1 record that other records can reference by path. Use it "
            "when work names a Jira ticket. Start with `board -C <board> call jira.ref --key ACME-123 --site acme`, "
            "which writes /jira/acme/ACME-123 once with its title. SKILL.md and README.md in the plugin's "
            "directory say more."
        ),
        "run": ["uv", "run", "--frozen", "--no-dev", "python", "-m", "board_jira"],
        "metadata": {"area": "planning"},
        "types": [ISSUE_TYPE],
        "capabilities": [
            {
                "name": "ref",
                "description": (
                    "Write the issue at /jira/<site>/<key> with its title, read once through acli, unless a record "
                    "is already there."
                ),
                "inputSchema": RefInput.model_json_schema(),
                "outputSchema": RefOutput.model_json_schema(),
                "annotations": {"idempotentHint": True},
                "_meta": {},
            },
        ],
    }


def schemas() -> dict[str, dict[str, JsonValue]]:
    """Each type's schema, as the plugin ships it, by its path under `types/`."""
    return {
        f"{ISSUE_TYPE}.schema.json": IssueRecord.model_json_schema(by_alias=True),
    }


if __name__ == "__main__":
    print(dumped(manifest()), end="")  # noqa: T201 - printing the manifest is this module's job
