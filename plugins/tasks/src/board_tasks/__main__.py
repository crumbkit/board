"""Run one request from Board."""

import sys

from board_plugin.protocol import CallRequest, PluginError, main
from pydantic import JsonValue

from board_tasks.due import due
from board_tasks.extract import extract


def call(request: CallRequest) -> JsonValue:
    """Dispatch a call to its capability."""
    if request.capability == "due":
        return due(request).model_dump(mode="json")
    raise PluginError("tasks::call::unknown_capability", f"tasks offers no capability {request.capability}")


sys.exit(main("tasks", call, extract))
