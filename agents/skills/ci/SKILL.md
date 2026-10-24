---
name: ci
description: Validate approved CI/build changes with least privilege, reproducibility and recovery guidance.
---

# CI review and implementation

1. Inspect triggers, matrix, dependencies, permissions and use of secrets for the affected workflow. Treat fork PR input, artifact names and downloaded scripts as untrusted.
2. Keep credentials out of logs/agents. Avoid broad write permissions, unsafe interpolation of issue/PR text and trusting unverified artifacts.
3. Make only approved changes. Run the repository YAML validator and relevant build/test/package commands; inspect platform-specific differences and failure propagation.
4. Record the exact tested head, commands and artifacts. Separate observed hosted CI jobs from local simulations and proposed future runs. Explain rollback and required owner configuration.

Do not publish releases, deploy infrastructure, change branch protection or retry privileged remote jobs as a role. Missing native tools/credentials are reported as limitations, not successful checks.
