"""Run one request from Board."""

import sys

from board_plugin.protocol import CallRequest, main

from board_boundary.crossing import check, record, verify
from board_boundary.shapes import CheckOutput, RecordOutput, VerifyOutput


def call(request: CallRequest) -> CheckOutput | VerifyOutput | RecordOutput:
    """Dispatch a call to its capability."""
    if request.capability == "check":
        return check(request)
    if request.capability == "verify":
        return verify(request)
    # Board refuses a capability the manifest does not offer before it starts the plugin.
    return record(request)


sys.exit(main("boundary", call))
