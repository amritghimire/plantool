---
name: acceptance-check
description: Map each acceptance criterion to an executed check, environment and owned evidence artifact.
---

# Acceptance matrix

Use this matrix in the summary or a referenced evidence log:

| Criterion | T-key / owner | Command or procedure | Environment / head | Result | Artifact |
| --- | --- | --- | --- | --- | --- |
| AC1 | T1 / backend | actual focused test | actual environment and SHA | observed result | actual artifact reference |

Use existing Check fields: name includes AC/T-key, status is pass/fail/skipped, evidence is an actual artifact:// reference. Do not invent new JSON fields. Record exit status and important assertions; capture failures and unavailability honestly.

Mocks prove only simulated behavior. Linux compilation does not prove Seatbelt, Touch ID, Keychain, real provider calls or the playground exit. If a required criterion cannot be verified, return blocked/needs_fix rather than done with a skipped or invented pass. Recheck criteria after any head change; old evidence does not certify the new head.
