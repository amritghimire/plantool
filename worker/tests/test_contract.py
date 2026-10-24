import json
from pathlib import Path
import pytest
from pydantic import ValidationError
from plantool_worker.models import Health, RoleResult

def test_health_and_results_are_strict():
    versions = json.loads((Path(__file__).parents[1]/"plantool_worker/versions.json").read_text())
    assert Health(**versions).contract_version == 4
    with pytest.raises(ValidationError):
        RoleResult(approval=True)
