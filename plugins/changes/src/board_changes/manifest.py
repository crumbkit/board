"""The plugin's manifest and the schema it ships, generated from its models (STANDARDS.md P3)."""

from typing import Any

from board_plugin.protocol import dumped
from pydantic import BaseModel, JsonValue

from board_changes.models import (
    RECORDS,
    BriefOutput,
    CheckInput,
    CheckOutput,
    ClaimInput,
    ClaimOutput,
    DropInput,
    FilesInput,
    LandedInput,
    MigrateInput,
    MigrateOutput,
    NextOutput,
    OpenInput,
    OpenOutput,
    PathInput,
    PlanInput,
    PlanOutput,
    PrOutput,
    SeedInput,
    SeedOutput,
    StartInput,
    StartOutput,
    Written,
)

# What each capability does, its input and output, and whether it only reads, in the order Board lists them.
CAPABILITIES: list[tuple[str, str, type[BaseModel], type[BaseModel], bool]] = [
    (
        "seed",
        "Write the workflows /workflows/personal and /workflows/work when they are missing.",
        SeedInput,
        SeedOutput,
        False,
    ),
    (
        "start",
        (
            "Start a change to the repository of the clone Board runs in: its repo record when it has none, the change"
            " with its branch, workflow, suite and route, and its first claim; return the repository's owners and"
            " contributing docs."
        ),
        StartInput,
        StartOutput,
        False,
    ),
    (
        "plan",
        (
            "Write a plan for the change and the records it uses, refusing a reference that reaches nothing, a"
            " plan over 3,000 words and used documents over 3,000 words in all; the newest plan is the change's"
            " plan."
        ),
        PlanInput,
        PlanOutput,
        False,
    ),
    (
        "brief",
        (
            "What an agent needs to start: the newest plan, one line per record it uses, the next step and the"
            " command that lists the repository's controls, in at most 600 words."
        ),
        PathInput,
        BriefOutput,
        True,
    ),
    (
        "claim",
        "Record that a session holds the change, and the root of the clone Board runs in as its worktree.",
        ClaimInput,
        ClaimOutput,
        False,
    ),
    (
        "files",
        "Record the paths the branch touches at a head, which decide the steps controls add.",
        FilesInput,
        Written,
        False,
    ),
    (
        "check",
        "Record a run of the check suite; the commit it names under the repo record's name is the change's head.",
        CheckInput,
        CheckOutput,
        False,
    ),
    (
        "next",
        "The change's steps, each done or not with the record that did it, and the next step and its actor.",
        PathInput,
        NextOutput,
        True,
    ),
    ("landed", "End the change as merged at a commit.", LandedInput, Written, False),
    ("drop", "End the change as stopped, saying why.", DropInput, Written, False),
    (
        "open",
        "The changes that have not ended, with their repository, branch, route, holder and worktree.",
        OpenInput,
        OpenOutput,
        True,
    ),
    (
        "pr",
        (
            "Write pr.md in the change's directory, a pull request body that says what changes, why, what the"
            " tests prove and do not prove, and the evidence, and return the gh command that would use it."
        ),
        PathInput,
        PrOutput,
        False,
    ),
    (
        "migrate",
        (
            "Rewrite each phase-based changes/change/v1 as a changes/change/v2 with its plan and opened records,"
            " in one write with apply; without it, list the lines and the changes left owed."
        ),
        MigrateInput,
        MigrateOutput,
        False,
    ),
]


def capability(
    name: str, description: str, given: type[BaseModel], output: type[BaseModel], *, read_only: bool
) -> dict[str, Any]:
    """One capability's Tool object."""
    return {
        "name": name,
        "description": description,
        "inputSchema": given.model_json_schema(),
        "outputSchema": output.model_json_schema(),
        "annotations": {"readOnlyHint": read_only},
        "_meta": {},
    }


def manifest() -> dict[str, JsonValue]:
    """`board/plugin-manifest/v1` for this plugin."""
    return {
        "$type": "board/plugin-manifest/v1",
        "name": "changes",
        "version": "0.1.0",
        # SKILL.md's description, the callout an agent sees first.
        "description": (
            "Record one change to a repository, from its plan to its merge, as records written once, and derive "
            "its next step from them. Use it when you start a change to a repository, and whenever you resume "
            "one. Start in the repository's clone, on the work branch, with `board --personal call changes.start "
            "--title '<title>' --session <session>`, then call `changes.next --path /changes/<name>` for the "
            "next step. A subagent whose prompt ends with `Board: /changes/<name>` first runs `board --personal "
            "call changes.brief --path /changes/<name>`, unless that brief is already in its context. SKILL.md and "
            "README.md in the plugin's directory say more."
        ),
        "run": ["uv", "run", "--frozen", "--no-dev", "python", "-m", "board_changes"],
        "metadata": {"area": "delivery"},
        "types": list(RECORDS),
        "capabilities": [
            capability(name, description, given, output, read_only=read_only)
            for name, description, given, output, read_only in CAPABILITIES
        ],
    }


def schemas() -> dict[str, dict[str, JsonValue]]:
    """Each type's schema, as the plugin ships it, by its path under `types/`."""
    return {f"{name}.schema.json": model.model_json_schema(by_alias=True) for name, model in RECORDS.items()}


if __name__ == "__main__":
    print(dumped(manifest()), end="")  # noqa: T201 - printing the manifest is this module's job
