-- Phase 5 facts belong to the daemon; graph decisions are typed, never approvals.
CREATE TABLE loop_intake (
 item_id TEXT PRIMARY KEY, project_id TEXT NOT NULL REFERENCES projects(id),
 issue_number INTEGER NOT NULL, source TEXT NOT NULL, admitted_at TEXT NOT NULL,
 urgent INTEGER NOT NULL DEFAULT 0, epoch INTEGER NOT NULL DEFAULT 0,
 UNIQUE(project_id,issue_number)
);
CREATE TABLE loop_declines (
 item_id TEXT PRIMARY KEY REFERENCES work_items(id), reason TEXT NOT NULL,
 duplicate_of INTEGER, declined_at TEXT NOT NULL, reset_at TEXT NOT NULL,
 label_seen INTEGER NOT NULL DEFAULT 0, active INTEGER NOT NULL DEFAULT 1,
 epoch INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE loop_waiters (
 item_id TEXT PRIMARY KEY REFERENCES work_items(id), thread_id TEXT NOT NULL,
 interrupt_id TEXT NOT NULL, kind TEXT NOT NULL
);
CREATE TABLE loop_settings (
 project_id TEXT PRIMARY KEY REFERENCES projects(id), version INTEGER NOT NULL DEFAULT 1,
 scan_day INTEGER, scan_time TEXT NOT NULL DEFAULT '08:00',
 nix_enabled INTEGER NOT NULL DEFAULT 0 CHECK(nix_enabled IN(0,1))
);
CREATE TABLE loop_scans (
 id TEXT PRIMARY KEY, project_id TEXT NOT NULL REFERENCES projects(id),
 week TEXT NOT NULL, evidence_json TEXT NOT NULL, created_at TEXT NOT NULL
);
CREATE TABLE loop_routes (
 id TEXT PRIMARY KEY, item_id TEXT NOT NULL REFERENCES work_items(id),
 title TEXT NOT NULL, body TEXT NOT NULL, route TEXT NOT NULL CHECK(route IN('proposed','candidate','backlog')),
 reason TEXT NOT NULL, issue_number INTEGER, created_at TEXT NOT NULL,
 tracking_published INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE loop_notifications (
 id TEXT PRIMARY KEY, project_id TEXT NOT NULL REFERENCES projects(id),
 item_id TEXT, body TEXT NOT NULL, created_at TEXT NOT NULL
);
