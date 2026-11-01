"""The plugin's manifest and the schemas it ships, generated from its models (STANDARDS.md P3)."""

from board_plugin.model import Untitled
from board_plugin.protocol import dumped
from pydantic import JsonValue

from board_runs.models import (
    AGENT_TYPE,
    RUN_TYPE,
    SUITE_TYPE,
    AgentRecord,
    LatestInput,
    LatestOutput,
    RunRecord,
    SuiteRecord,
    TrendInput,
    TrendOutput,
)


def manifest() -> dict[str, JsonValue]:
    """`board/plugin-manifest/v1` for this plugin."""
    return {
        "$type": "board/plugin-manifest/v1",
        "name": "runs",
        "version": "0.1.0",
        "description": (
            "Suites of tests, benchmarks and evals, and the runs that record what each gave. "
            "`board call runs.latest --suite /suites/<name>` returns the latest full run of a clean revision; "
            "`board call runs.trend --suite /suites/<name> --measure <name>` returns a measure over such runs. "
            "README.md in the plugin's directory says how to record a run."
        ),
        "run": ["uv", "run", "--frozen", "--no-dev", "python", "-m", "board_runs"],
        "metadata": {"area": "testing"},
        "types": [SUITE_TYPE, RUN_TYPE, AGENT_TYPE],
        "capabilities": [
            {
                "name": "latest",
                "description": (
                    "The latest full run of a clean revision of a suite, with its outcome, measures, failed cases"
                    " and age, or null when no run qualifies."
                ),
                "inputSchema": LatestInput.model_json_schema(),
                "outputSchema": LatestOutput.model_json_schema(),
                "annotations": {"readOnlyHint": True},
                "_meta": {},
            },
            {
                "name": "trend",
                "description": (
                    "One measure of a suite, or of one of its cases, over its full runs of clean revisions,"
                    " newest first."
                ),
                "inputSchema": TrendInput.model_json_schema(),
                "outputSchema": TrendOutput.model_json_schema(),
                "annotations": {"readOnlyHint": True},
                "_meta": {},
            },
        ],
    }


def suite_schema() -> dict[str, JsonValue]:
    """The schema of `runs/suite/v1`, as the plugin ships it."""
    return SuiteRecord.model_json_schema(by_alias=True)


def run_schema() -> dict[str, JsonValue]:
    """The schema of `runs/run/v1`, as the plugin ships it."""
    return RunRecord.model_json_schema(by_alias=True)


def agent_schema() -> dict[str, JsonValue]:
    """The schema of `runs/agent/v1`, as the plugin ships it, keeping Board's shape rules."""
    return AgentRecord.model_json_schema(by_alias=True, schema_generator=Untitled)


if __name__ == "__main__":
    print(dumped(manifest()), end="")  # noqa: T201 - printing the manifest is this module's job
