-- One GitHub comment is single-use globally; one sprint decision may authorize many gates.
CREATE TABLE decision_targets(decision_id TEXT NOT NULL REFERENCES decisions(id),gate_id TEXT NOT NULL REFERENCES gates(id),item_id TEXT NOT NULL REFERENCES work_items(id),plan_sha TEXT NOT NULL,PRIMARY KEY(decision_id,gate_id));

UPDATE project_settings SET review_globs_json='["**/migrations/**","**/*auth*","**/api/**",".github/**","**/Cargo.toml","**/package.json","**/pyproject.toml","**/*auth*/**","**/routes/**","**/contract/**","**/Cargo.lock","**/package-lock.json","**/uv.lock","**/requirements*.txt"]' WHERE review_globs_json='["**/migrations/**","**/*auth*","**/api/**",".github/**","**/Cargo.toml","**/package.json","**/pyproject.toml"]';
