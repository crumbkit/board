"""Run one request from Board."""

import sys

from board_plugin.protocol import CallRequest, PluginError, main

from board_boundary.crossing import check, record, verify
from board_boundary.shapes import CheckOutput, RecordOutput, VerifyOutput


def call(request: CallRequest) -> CheckOutput | VerifyOutput | RecordOutput:
    """Dispatch a call to its capability."""
    if request.capability == "check":
        return check(request)
    if request.capability == "verify":
        return verify(request)
    if request.capability == "record":
        return record(request)
    raise PluginError(
        "boundary::call::unknown_capability",
        f"boundary offers no capability {request.capability}; it offers check, verify and record",
    )


sys.exit(main("boundary", call))
