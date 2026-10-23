-- Keep previously pushed draft checkpoints upgradeable too.
ALTER TABLE github_gate_context ADD COLUMN summary TEXT NOT NULL DEFAULT 'No discussion summary supplied.';
ALTER TABLE github_gate_context ADD COLUMN discussion_thread_id TEXT REFERENCES threads(id);
