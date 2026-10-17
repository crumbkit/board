"""The plugin's manifest and the schemas it ships, generated from its models (STANDARDS.md P3)."""

from board_plugin.protocol import dumped
from pydantic import JsonValue

from board_evals.models import EVAL_TYPE, RUN_TYPE, AboutInput, EvalRecord, ListOutput, OpenInput, RunRecord


def manifest() -> dict[str, JsonValue]:
    """`board/plugin-manifest/v1` for this plugin."""
    return {
        "$type": "board/plugin-manifest/v1",
        "name": "evals",
        "version": "0.1.0",
        # SKILL.md's description, the callout an agent sees first.
        "description": (
            "Record every test of a library, tool, model or agent as an eval and its runs in the board, and "
            "check what was already tested before testing. Use it before you start a test and when you "
            'conclude one. Start with `board -C <board> call evals.about` with `{"subject": "<name>"}` on '
            "stdin, which lists the evals of what you will test. `board -C <board> call evals.open` with "
            "`{}` on stdin lists those still planned or running. SKILL.md and README.md in the plugin's "
            "directory say more."
        ),
        "run": ["uv", "run", "--frozen", "--no-dev", "python", "-m", "board_evals"],
        "metadata": {"area": "testing"},
        "types": [EVAL_TYPE, RUN_TYPE],
        "capabilities": [
            {
                "name": "about",
                "description": (
                    "The evals that name a subject, or that name a repository's mount or live in it,"
                    " whatever their status, here and in mounted boards."
                ),
                "inputSchema": AboutInput.model_json_schema(),
                "outputSchema": ListOutput.model_json_schema(),
                "annotations": {"readOnlyHint": True},
                "_meta": {},
            },
            {
                "name": "open",
                "description": "The evals still planned or running, here and in mounted boards.",
                "inputSchema": OpenInput.model_json_schema(),
                "outputSchema": ListOutput.model_json_schema(),
                "annotations": {"readOnlyHint": True},
                "_meta": {},
            },
        ],
    }


def eval_schema() -> dict[str, JsonValue]:
    """The schema of `evals/eval/v1`, as the plugin ships it."""
    return EvalRecord.model_json_schema(by_alias=True)


def run_schema() -> dict[str, JsonValue]:
    """The schema of `evals/run/v1`, as the plugin ships it."""
    return RunRecord.model_json_schema(by_alias=True)


if __name__ == "__main__":
    print(dumped(manifest()), end="")  # noqa: T201 - printing the manifest is this module's job
