"""The prompt record and the capabilities' inputs and outputs, from which the plugin's schemas are generated.

A call reads them as plain JSON, which Board has validated against these
schemas, so a call loads no pydantic (STANDARDS.md P3).
"""

from typing import Annotated, Literal

from board_plugin.model import Strict, unset
from pydantic import Field
from pydantic.config import JsonDict
from pydantic.json_schema import SkipJsonSchema

INPUT = r"[a-z][a-z0-9_]{0,31}"
PLACEHOLDER = r"\{" + INPUT + r"\}"


def named(schema: JsonDict) -> None:
    """As `unset`, and refuse an input whose name the pattern does not admit."""
    unset(schema)
    schema["additionalProperties"] = False


class InClone(Strict):
    """Work that starts in a clone of the GitHub repository whose remote is `remote`."""

    remote: str = Field(
        pattern=rf"^({PLACEHOLDER}|[^/\s]+/[^/\s]+)$", description="owner/name, or an input such as {repo}."
    )


class InDirectory(Strict):
    """Work that starts in a directory that is not one repository."""

    directory: str = Field(
        pattern=rf"^((/|~/).*|{PLACEHOLDER}(/.*)?)$",
        description="An absolute path, a path starting ~/, or an input alone or followed by /.",
    )


class PromptRecord(Strict):
    """`prompts/prompt/v1`: the frontmatter of a Markdown prompt under `prompts/`, whose body is the text to render.

    `runs_in` says where the work starts. `inputs` maps each input's name to a
    description of its value; every declared input is required. Any other
    field is refused, because a misspelt field in an instruction should fail.
    """

    tag: Literal["prompts/prompt/v1"] = Field(alias="$type")
    title: str = Field(min_length=1)
    purpose: str = Field(min_length=1)
    runs_in: InClone | InDirectory
    inputs: dict[Annotated[str, Field(pattern=f"^{INPUT}$")], str] | SkipJsonSchema[None] = Field(
        default=None, json_schema_extra=named
    )


class ReposRecord(Strict):
    """`prompts/repos/v1`: the mounts whose prompts the board admits, at `/prompts/repos.json` in its own tree.

    Only this file in the board's own working tree marks a repo, and only a mount whose pointer is in that same
    tree counts, so nothing a mounted repository commits can make its prompts admitted.
    """

    tag: Literal["prompts/repos/v1"] = Field(alias="$type")
    mounts: list[Annotated[str, Field(json_schema_extra={"format": "board-ref"})]] = Field(
        json_schema_extra={"uniqueItems": True},
        description="The mount of each repo whose prompts are admitted, such as /repos/notes-personal.",
    )


class GetInput(Strict):
    """The prompt to render, and its input values."""

    path: str = Field(
        pattern=r"^/.*\.md$", description="The prompt's path in the board, such as /prompts/review-pr.md."
    )
    inputs: dict[str, str] | SkipJsonSchema[None] = Field(
        default=None,
        json_schema_extra=unset,
        description=(
            "The value of each input by its name. Board reads stdin only when no flag is given, so give the whole "
            'input as JSON on stdin: {"path": "/prompts/review-pr.md", "inputs": {"pr": "2140"}}.'
        ),
    )
    runs_in: InClone | InDirectory | SkipJsonSchema[None] = Field(
        default=None,
        json_schema_extra=unset,
        description=(
            "Where the work starts, in place of the prompt's own runs_in; rendered with the inputs and checked the "
            "same way."
        ),
    )


class GetOutput(Strict):
    """The rendered prompt and where it came from.

    `repo` is the repo that holds it. `blob` is the git blob id of its file,
    and `source` the commit it was read at, or `worktree` for the board's own
    tree. `unused_inputs` lists the declared inputs its body never names.
    """

    path: str
    repo: str
    title: str
    purpose: str
    runs_in: dict[str, str]
    text: str
    blob: str
    source: str
    unused_inputs: list[str]


class ListInput(Strict):
    """The prompts to list, in the repos the caller's remote chooses."""

    repo: str | SkipJsonSchema[None] = Field(
        default=None,
        json_schema_extra=unset,
        pattern="^/",
        description=(
            "One repo's prompts: / for the board's own tree, or a marked repo's mount path such as "
            "/repos/notes-personal; the caller's git remote chooses the repos when left out."
        ),
    )
    remote: str | SkipJsonSchema[None] = Field(
        default=None,
        json_schema_extra=unset,
        min_length=1,
        description=(
            "A git remote, or owner/name, to choose the repos by in place of the origin of the directory board "
            "was started in."
        ),
    )


class ListedPrompt(Strict):
    """A prompt as written: where it is, what it is for, where it runs and the inputs it takes."""

    repo: str
    path: str
    title: str
    purpose: str
    runs_in: dict[str, str]
    inputs: dict[str, str]


class ListOutput(Strict):
    """At most 100 prompts, by repo then title, and how many there are.

    `degraded` counts the Markdown files under a `prompts/` directory in scope
    that `board check` marks degraded for any code but a broken link
    (`board::ref::dangling` or `board::ref::wrong_type`): an invalid prompt, or
    frontmatter that does not parse or names an unknown type. Plain Markdown,
    such as an eval's evidence, is not counted. `repos` are the repos read. `origin_unread`
    says why the caller's git repository gave no origin, when it did not, so
    only `/` and the personal repo were read.
    """

    prompts: list[ListedPrompt]
    total: int
    degraded: int
    repos: list[str]
    origin_unread: str | None
