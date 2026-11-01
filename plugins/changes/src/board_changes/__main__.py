"""Run one request from Board."""

import sys

from board_plugin.protocol import CallRequest, PluginError, main

from board_changes.migrate import migrate
from board_changes.open import open_changes
from board_changes.plan import brief, plan
from board_changes.pr import pr
from board_changes.records import check, claim, drop, files, landed
from board_changes.start import start
from board_changes.steps import next_step
from board_changes.workflow import seed

CAPABILITIES = {
    "seed": seed,
    "start": start,
    "plan": plan,
    "brief": brief,
    "claim": claim,
    "files": files,
    "check": check,
    "next": next_step,
    "landed": landed,
    "drop": drop,
    "open": open_changes,
    "pr": pr,
    "migrate": migrate,
}


def call(request: CallRequest) -> dict[str, object]:
    """Dispatch a call to its capability."""
    capability = CAPABILITIES.get(request.capability)
    if capability is None:
        raise PluginError(
            "changes::call::unknown_capability",
            f"changes offers no capability {request.capability}; it offers {', '.join(CAPABILITIES)}",
        )
    return capability(request)


sys.exit(main("changes", call))
