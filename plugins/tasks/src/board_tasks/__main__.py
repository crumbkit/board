"""Run one request from Board."""

import sys

from board_plugin.protocol import CallRequest, PluginError, main

from board_tasks.available import available
from board_tasks.extract import extract
from board_tasks.write import add, claim, defer, done, drop, reviewed

WRITES = {"add": add, "claim": claim, "done": done, "defer": defer, "drop": drop, "reviewed": reviewed}


def call(request: CallRequest) -> dict[str, object]:
    """Dispatch a call to its capability."""
    if request.capability == "available":
        return available(request)
    if request.capability in WRITES:
        return WRITES[request.capability](request)
    raise PluginError("tasks::call::unknown_capability", f"tasks offers no capability {request.capability}")


sys.exit(main("tasks", call, extract))
