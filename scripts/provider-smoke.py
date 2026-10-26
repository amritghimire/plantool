#!/usr/bin/env python3
"""Live diagnostics must use the daemon's global lease and passkey authorization."""
import sys

route = sys.argv[1] if len(sys.argv) > 1 else "the configured OpenRouter route"
print(
    "Standalone provider requests cannot share PlanTool's global model slot.\n"
    "Open http://localhost:41200/settings/models and use Test <route> with passkey,\n"
    "or Test all configured routes serially with passkey.\n"
    f"Select the route for {route}; the result shows the resolved model, capabilities and sanitized errors.\n"
    "No provider request was sent by this script."
)
