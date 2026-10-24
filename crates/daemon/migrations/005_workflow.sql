CREATE TABLE workflow_items(item_id TEXT PRIMARY KEY REFERENCES work_items(id),state_json TEXT NOT NULL,base_sha TEXT NOT NULL,head_sha TEXT NOT NULL,pr_number INTEGER,pr_head TEXT,merge_state TEXT NOT NULL DEFAULT 'none',last_error TEXT,docs_body TEXT,reviewed_head TEXT,qa_head TEXT);
CREATE TABLE workflow_proposals(id TEXT PRIMARY KEY,item_id TEXT NOT NULL REFERENCES work_items(id),title TEXT NOT NULL,body TEXT NOT NULL,issue_number INTEGER,UNIQUE(item_id,title,body));
CREATE TABLE workflow_actions(id TEXT PRIMARY KEY,item_id TEXT NOT NULL REFERENCES work_items(id),kind TEXT NOT NULL,payload_json TEXT NOT NULL,state TEXT NOT NULL DEFAULT 'pending',result_json TEXT);

ALTER TABLE dispatches ADD COLUMN git_config_sha TEXT;
ALTER TABLE dispatches ADD COLUMN base_sha TEXT;
