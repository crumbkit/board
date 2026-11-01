"""Run one request from Board."""

import sys

from board_plugin.protocol import CallRequest, PluginError, main

from board_github.hook import install_hook, pre_push
from board_github.ref import ref
from board_github.send import comment, issue_create, pr_create, pr_edit, push

CAPABILITIES = {
    "ref": ref,
    "push": push,
    "pr-create": pr_create,
    "pr-edit": pr_edit,
    "comment": comment,
    "issue-create": issue_create,
    "install-hook": install_hook,
    "pre-push": pre_push,
}


def call(request: CallRequest) -> dict[str, object]:
    """Dispatch a call to its capability."""
    capability = CAPABILITIES.get(request.capability)
    if capability is None:
        raise PluginError(
            "github::call::unknown_capability",
            f"github offers no capability {request.capability}; it offers {', '.join(CAPABILITIES)}",
        )
    return capability(request)


sys.exit(main("github", call))
