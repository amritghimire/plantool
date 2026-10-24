-- GitHub comment IDs are single-use across all gates, not just within a gate.
CREATE UNIQUE INDEX github_decision_once ON decisions(source_event_id) WHERE source='github';
CREATE TABLE github_repositories(project_id TEXT PRIMARY KEY REFERENCES projects(id), app_id INTEGER NOT NULL, installation_id INTEGER NOT NULL, repo_id INTEGER NOT NULL UNIQUE, full_name TEXT NOT NULL, default_branch TEXT NOT NULL, app_bot_id INTEGER NOT NULL, last_error TEXT, last_sync_at TEXT);
CREATE TABLE github_objects(resource TEXT NOT NULL, object_id TEXT NOT NULL, body_json TEXT NOT NULL, PRIMARY KEY(resource,object_id));
CREATE TABLE github_gate_context(gate_id TEXT PRIMARY KEY REFERENCES gates(id), revision_id TEXT NOT NULL REFERENCES revisions(id), thread_id TEXT NOT NULL, interrupt_id TEXT NOT NULL, publication_key TEXT NOT NULL UNIQUE);
CREATE TABLE github_label_actors(project_id TEXT NOT NULL REFERENCES projects(id), issue_number INTEGER NOT NULL, label TEXT NOT NULL, actor_id INTEGER, authorized INTEGER NOT NULL, event_id INTEGER NOT NULL, PRIMARY KEY(project_id,issue_number,label));
