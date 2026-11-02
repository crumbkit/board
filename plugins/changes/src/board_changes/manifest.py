"""The plugin's manifest and the schema it ships, generated from its models (STANDARDS.md P3)."""

from typing import Any

from board_plugin.protocol import dumped
from pydantic import BaseModel, JsonValue

from board_changes.change import CHANGE_TYPE, PLAN_TYPE, UNDERSTANDING_TYPE
from board_changes.models import (
    CHANGE_SCHEMA_PATH,
    PLAN_SCHEMA_PATH,
    UNDERSTANDING_SCHEMA_PATH,
    AdvanceOutput,
    AttachInput,
    AttachOutput,
    BriefOutput,
    ChangeRecord,
    NextOutput,
    OpenInput,
    OpenOutput,
    PathInput,
    PlanInput,
    PlanOutput,
    PlanRecord,
    PrototypeInput,
    PrototypeOutput,
    PrOutput,
    StartInput,
    StartOutput,
    UnderstandingRecord,
    VerdictInput,
    VerdictOutput,
)


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
            "Walk one change to a repository from its plan to its pull request, and gather every "
            "record it produces. Use it when you start a change to a repository mounted in the board, and whenever "
            "you resume one. Start with `board -C <board> call changes.start --mount /repos/<name> --title "
            "'<title>'`, then call `changes.next --path /changes/<name>` for the next step. SKILL.md and "
            "README.md in the plugin's directory say more."
        ),
        "run": ["uv", "run", "--frozen", "--no-dev", "python", "-m", "board_changes"],
        "metadata": {"area": "delivery"},
        "types": [CHANGE_TYPE, PLAN_TYPE, UNDERSTANDING_TYPE],
        "capabilities": [
            capability(
                "start",
                "Create a change, its plan to fill in and, unless one is named, its task, set its route from "
                "CODEOWNERS, and return the repository's owners, contributing docs and check suite.",
                StartInput,
                StartOutput,
                read_only=False,
            ),
            capability(
                "next",
                "The change's phase, each check of its gate as pass or missing, and what to do now.",
                PathInput,
                NextOutput,
                read_only=True,
            ),
            capability(
                "attach",
                "Add a reference to a record the change produced, or set its task, route, head commit or pull request.",
                AttachInput,
                AttachOutput,
                read_only=False,
            ),
            capability(
                "advance",
                "Move the change to its next phase when its gate passes and, into build, while fewer than three"
                " changes are in build; otherwise refuse and say what is missing.",
                PathInput,
                AdvanceOutput,
                read_only=False,
            ),
            capability(
                "open",
                "The changes that are not done or dropped, with their phase, route, task and kind; by kind, every"
                " prototype with its question, state, verdict and branch.",
                OpenInput,
                OpenOutput,
                read_only=True,
            ),
            capability(
                "pr",
                "Write pr.md in the change's directory, a pull request body that says what changes, why, what the"
                " tests prove and do not prove, and the evidence, and return the gh command that would use it.",
                PathInput,
                PrOutput,
                read_only=False,
            ),
            capability(
                "plan",
                "Write a plan for the change and the records it uses, refusing a reference that reaches nothing, a"
                " plan over 3,000 words and used documents over 3,000 words in all; the newest plan is the change's"
                " plan.",
                PlanInput,
                PlanOutput,
                read_only=False,
            ),
            capability(
                "brief",
                "What an agent needs to start: the newest plan, one line per record it uses, the next step and the"
                " command that lists the repository's controls, in at most 600 words.",
                PathInput,
                BriefOutput,
                read_only=True,
            ),
            capability(
                "prototype",
                "Start a prototype that answers one question: add its task under a parent, write its change in"
                " build on branch proto/<name>, and return its brief. It never lands.",
                PrototypeInput,
                PrototypeOutput,
                read_only=False,
            ),
            capability(
                "verdict",
                "Record what a prototype found in one sentence, and the artifact version it means, which ends it:"
                " done when settled, dropped when abandoned.",
                VerdictInput,
                VerdictOutput,
                read_only=False,
            ),
            capability(
                "pause",
                "Move a change in build to paused, which frees its place under the cap of three changes in build.",
                PathInput,
                AdvanceOutput,
                read_only=False,
            ),
            capability(
                "resume",
                "Move a paused change back to build, refused while three changes of its kind hold places.",
                PathInput,
                AdvanceOutput,
                read_only=False,
            ),
        ],
    }


def schemas() -> dict[str, dict[str, JsonValue]]:
    """Each type's schema, as the plugin ships it, by its path under `types/`."""
    return {
        CHANGE_SCHEMA_PATH: ChangeRecord.model_json_schema(by_alias=True),
        PLAN_SCHEMA_PATH: PlanRecord.model_json_schema(by_alias=True),
        UNDERSTANDING_SCHEMA_PATH: UnderstandingRecord.model_json_schema(by_alias=True),
    }


if __name__ == "__main__":
    print(dumped(manifest()), end="")  # noqa: T201 - printing the manifest is this module's job
