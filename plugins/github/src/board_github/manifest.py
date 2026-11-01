"""The plugin's manifest and the schemas it ships, generated from its models (STANDARDS.md P3)."""

from typing import Any

from board_plugin.protocol import dumped
from pydantic import BaseModel, JsonValue

from board_github.models import (
    ISSUE_SCHEMA_PATH,
    PULL_SCHEMA_PATH,
    CommentInput,
    CoversInput,
    CoversOutput,
    InstallHookInput,
    InstallHookOutput,
    IssueCreateInput,
    IssueCreateOutput,
    IssueRecord,
    PrCreateInput,
    PrCreateOutput,
    PrEditInput,
    PrePushInput,
    PrePushOutput,
    PullRecord,
    PushInput,
    PushOutput,
    RefInput,
    RefOutput,
    SentOutput,
)
from board_github.objects import ISSUE_TYPE, PULL_TYPE

# The variables gh reads for its account and configuration, which Board passes a capability that runs gh.
GH_ENV = ["GH_TOKEN", "GITHUB_TOKEN", "GH_CONFIG_DIR"]
# What a sending action passes on: gh's variables, those the boundary's judged rules read, and the ssh agent git
# pushes through.
SEND_ENV = [*GH_ENV, "BOARD_CONTROLS_AGENT", "ANTHROPIC_API_KEY", "CLAUDE_CODE_OAUTH_TOKEN", "SSH_AUTH_SOCK"]
# The variables that make git run another ssh command, which the URL rule reads to fail closed on an ssh alias.
SSH_ENV = ["GIT_SSH_COMMAND", "GIT_SSH"]
# How long a sending action may run, in seconds: the boundary's 1,800 for its check, then the push and gh.
SEND_TIMEOUT = 2400
# The sending actions: name, description, input and output.
SENDING: list[tuple[str, str, type[BaseModel], type[BaseModel]]] = [
    (
        "push",
        "Push the branch checked out to origin, once the boundary passes its name and each commit it would send.",
        PushInput,
        PushOutput,
    ),
    (
        "pr-create",
        (
            "Push the branch checked out and open a pull request from it, once the boundary passes the commits, "
            "title and body in one check, and write the pull request's record."
        ),
        PrCreateInput,
        PrCreateOutput,
    ),
    (
        "pr-edit",
        "Change a pull request's title or body, once the boundary passes the new text.",
        PrEditInput,
        SentOutput,
    ),
    ("comment", "Comment on a pull request or issue, once the boundary passes the comment.", CommentInput, SentOutput),
    (
        "issue-create",
        "Open an issue, once the boundary passes its title and body, and write the issue's record.",
        IssueCreateInput,
        IssueCreateOutput,
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
        "name": "github",
        "version": "0.1.0",
        # SKILL.md's description, the callout an agent sees first.
        "description": (
            "Record a GitHub pull request or issue as a typed record, and push, open or edit pull requests, comment "
            "and open issues only after the boundary passes what they send. Use it to name a pull request or issue "
            "in a record, and in place of git push and gh for sending. Run `board call github.ref --ref "
            "<owner>/<repo>#<number>`, or `board call github.push`, `github.pr-create`, `github.pr-edit`, "
            "`github.comment` or `github.issue-create` from the checkout. SKILL.md and README.md in the plugin's "
            "directory say more."
        ),
        "run": ["uv", "run", "--frozen", "--no-dev", "python", "-m", "board_github"],
        "metadata": {"area": "delivery"},
        "types": [PULL_TYPE, ISSUE_TYPE],
        "capabilities": [
            capability(
                "ref",
                "Write the record of a pull request or issue, asking gh which it is and its title, or report the "
                "record already there without asking.",
                RefInput,
                RefOutput,
                read_only=False,
            )
            | {"env": list[JsonValue](GH_ENV)},
            *(
                capability(name, description, given, output, read_only=False)
                | {"env": list[JsonValue](SEND_ENV), "timeout": SEND_TIMEOUT}
                for name, description, given, output in SENDING
            ),
            capability(
                "install-hook",
                "Install a git pre-push hook in the checkout that has the boundary check what each push sends, "
                "refusing to replace a different pre-push hook.",
                InstallHookInput,
                InstallHookOutput,
                read_only=False,
            ),
            capability(
                "pre-push",
                "Run by the pre-push hook: check the refs git pushes as github.push checks a push, and refuse "
                "when the boundary refuses or the check cannot run.",
                PrePushInput,
                PrePushOutput,
                read_only=False,
            )
            | {"env": list[JsonValue]([*SEND_ENV, "BOARD_GITHUB_PUSH_URL", *SSH_ENV]), "timeout": SEND_TIMEOUT},
            capability(
                "covers",
                "Name the boundaries that cover a git push or gh write from the checkout, and where it goes, "
                "checking nothing and writing nothing.",
                CoversInput,
                CoversOutput,
                read_only=True,
            )
            | {"env": list[JsonValue](SSH_ENV)},
        ],
    }


def schemas() -> dict[str, dict[str, JsonValue]]:
    """Each type's schema, as the plugin ships it, by its path under `types/`."""
    return {
        PULL_SCHEMA_PATH: PullRecord.model_json_schema(by_alias=True),
        ISSUE_SCHEMA_PATH: IssueRecord.model_json_schema(by_alias=True),
    }


if __name__ == "__main__":
    print(dumped(manifest()), end="")  # noqa: T201 - printing the manifest is this module's job
