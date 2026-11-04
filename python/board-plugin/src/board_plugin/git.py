"""What every plugin's git command runs with."""

import os

# The variables that keep the user's and the system's git config out of a git command, and stop it prompting.
ISOLATED = {"GIT_CONFIG_GLOBAL": os.devnull, "GIT_CONFIG_NOSYSTEM": "1", "GIT_TERMINAL_PROMPT": "0"}
