import json
from pathlib import Path
import pytest
from pydantic import ValidationError
from plantool_worker.models import Health, RoleResult

def test_health_and_results_are_strict():
    versions = json.loads((Path(__file__).parents[1]/"plantool_worker/versions.json").read_text())
    assert Health(**versions).contract_version == 5
    with pytest.raises(ValidationError):
        RoleResult(approval=True)

def test_check_evidence_must_reference_a_stored_artifact():
    result = {
        "item": "i", "role": "cto", "attempt_id": "a", "generation": 1,
        "outcome": "done", "summary": "Reviewed plan", "tasks_done": [],
        "artifacts": [], "findings": [], "next": None,
        "checks": [{"name": "plan review", "status": "pass", "evidence": "local plan"}],
    }
    with pytest.raises(ValidationError):
        RoleResult.model_validate(result)
    result["checks"][0]["evidence"] = "artifact://" + "a" * 64
    assert RoleResult.model_validate(result).checks[0].evidence == result["checks"][0]["evidence"]
