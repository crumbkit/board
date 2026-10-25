"""Run one request from Board."""

import sys

from board_plugin.protocol import CallRequest, PluginError, main

from board_tasks.available import available
from board_tasks.due import due
from board_tasks.extract import extract
from board_tasks.next import next_tasks


def call(request: CallRequest) -> dict[str, object]:
    """Dispatch a call to its capability."""
    if request.capability == "available":
        return available(request)
    if request.capability == "due":
        return due(request).model_dump(mode="json")
    if request.capability == "next":
        return next_tasks(request).model_dump(mode="json")
    raise PluginError("tasks::call::unknown_capability", f"tasks offers no capability {request.capability}")


sys.exit(main("tasks", call, extract))
