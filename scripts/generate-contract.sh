#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
cargo run --quiet -p plantool-contract --bin export-schema > contract/schema.json
uv run --project worker --frozen datamodel-codegen \
  --input contract/schema.json --input-file-type jsonschema \
  --output worker/plantool_worker/models.py --output-model-type pydantic_v2.BaseModel \
  --target-python-version 3.12 --disable-timestamp --use-standard-collections \
  --use-union-operator --use-subclass-enum
cargo run --quiet -p plantool-contract --bin export-schema -- --versions > worker/plantool_worker/versions.json
