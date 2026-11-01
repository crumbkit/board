"""Run one request from Board."""

import sys

from board_plugin.protocol import CallRequest, PluginError, main

from board_jira.ref import ref


def call(request: CallRequest) -> dict[str, object]:
    """Dispatch a call to its capability."""
    if request.capability == "ref":
        return ref(request)
    raise PluginError(
        "jira::call::unknown_capability", f"jira offers no capability {request.capability}; it offers ref"
    )


sys.exit(main("jira", call))
