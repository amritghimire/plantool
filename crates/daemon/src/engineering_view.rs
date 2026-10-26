//! Browser-only read models. No graph transitions, approvals or model calls.
use crate::database;
use anyhow::Context;
use rusqlite::Connection;
use serde_json::{json, Value};
use std::path::Path;
fn activity_state(db: &Connection, id: &str, stored: &str) -> anyhow::Result<String> {
    if !["queued", "starting", "running", "waiting", "idle"].contains(&stored) {
        return Ok(stored.into());
    }
    let held: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM leases WHERE id=1 AND holder_attempt_id=?1)",
        [id],
        |r| r.get(0),
    )?;
    if held {
        return Ok("running".into());
    }
    let retry: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM attempt_waits WHERE attempt_id=?1)",
        [id],
        |r| r.get(0),
    )?;
    if retry {
        return Ok("retry_wait".into());
    }

    let queued: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM model_queue WHERE attempt_id=?1)",
        [id],
        |r| r.get(0),
    )?;
    Ok(if queued || stored == "queued" {
        "queued"
    } else {
        "processing"
    }
    .into())
}
fn annotate_activity(db: &Connection, rows: &mut [Value]) -> anyhow::Result<()> {
    for row in rows {
        let stored = row["state"].as_str().unwrap_or("").to_owned();
        row["stored_state"] = json!(stored);
        row["state"] = json!(activity_state(
            db,
            row["id"].as_str().unwrap_or(""),
            &stored
        )?);
    }
    Ok(())
}
fn item_rows(
    db: &Connection,
    project: Option<&str>,
    schedulers: bool,
) -> anyhow::Result<Vec<Value>> {
    let mut q=db.prepare("SELECT i.id,i.project_id,i.title,i.github_issue_number,i.state,i.priority,w.pr_number,w.last_error,json_extract(w.state_json,'$.frozen'),json_extract(w.state_json,'$.step'),(SELECT count(*) FROM gates g WHERE g.item_id=i.id AND g.state IN('open','pending')) FROM work_items i JOIN workflow_items w ON w.item_id=i.id WHERE i.kind='eos' AND i.deleted_at IS NULL AND (coalesce(json_extract(w.state_json,'$.project'),0)=0 OR ?2) AND (?1 IS NULL OR i.project_id=?1) ORDER BY i.created_at DESC,i.id")?;
    let rows=q.query_map(rusqlite::params![project,schedulers],|r|Ok(json!({"id":r.get::<_,String>(0)?,"project":r.get::<_,String>(1)?,"title":r.get::<_,String>(2)?,"issue":r.get::<_,Option<u64>>(3)?,"state":r.get::<_,String>(4)?,"priority":r.get::<_,Option<String>>(5)?,"pr":r.get::<_,Option<u64>>(6)?,"error":r.get::<_,Option<String>>(7)?,"frozen":r.get::<_,Option<bool>>(8)?.unwrap_or(false),"step":r.get::<_,Option<String>>(9)?.unwrap_or_default(),"waiting_gates":r.get::<_,u64>(10)?})))?.collect::<Result<Vec<_>,_>>()?;
    let mut rows = rows;
    for row in &mut rows {
        let state = crate::workflow::state(db, row["id"].as_str().unwrap())?.unwrap_or(Value::Null);
        row["board_column"] = database::get::<Option<String>>(
            db,
            &format!("board.column:{}", row["id"].as_str().unwrap()),
        )?
        .flatten()
        .map(Value::String)
        .unwrap_or(Value::Null);
        row["next_action"] =
            crate::automation::next_action(db, row["id"].as_str().unwrap(), &state)?;
    }
    Ok(rows)
}
fn items(db: &Connection, project: Option<&str>) -> anyhow::Result<Vec<Value>> {
    item_rows(db, project, false)
}
fn roles(home: &Path, db: &Connection, project: Option<&str>) -> anyhow::Result<Value> {
    let cfg = crate::models::router::config(home)?;
    let allowed = if let Some(project) = project {
        crate::models::router::allowed(db, project)?
    } else {
        cfg.routes.values().map(|r| r.provider.clone()).collect()
    };
    let mut q = db.prepare("SELECT route FROM route_health WHERE available=1")?;
    let healthy = q
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<Result<std::collections::HashSet<_>, _>>()?;
    Ok(json!([
        "cto",
        "pm",
        "backend",
        "frontend",
        "devops",
        "qa",
        "reviewer",
        "docs",
        "supervisor"
    ]
    .into_iter()
    .map(|role| {
        let ready = cfg.chains.get(role).is_some_and(|chain| {
            chain.iter().any(|name| {
                healthy.contains(name)
                    && cfg
                        .routes
                        .get(name)
                        .is_some_and(|r| allowed.contains(&r.provider))
            })
        });
        json!({"role":role,"ready":ready})
    })
    .collect::<Vec<_>>()))
}
pub fn overview(home: &Path) -> anyhow::Result<Value> {
    let db = database::open(home)?;
    let mut q=db.prepare("SELECT p.id,p.repo_full_name,p.local_checkout,p.visibility,g.app_id,g.installation_id,g.default_branch,g.last_sync_at,g.last_error,json_extract(w.state_json,'$.step') FROM projects p LEFT JOIN github_repositories g ON g.project_id=p.id LEFT JOIN workflow_items w ON w.item_id=p.id||':project' WHERE p.mode='eos' ORDER BY p.repo_full_name,p.id")?;
    let mut projects=q.query_map([],|r|Ok(json!({"id":r.get::<_,String>(0)?,"name":r.get::<_,Option<String>>(1)?,"checkout":r.get::<_,String>(2)?,"visibility":r.get::<_,Option<String>>(3)?,"app_id":r.get::<_,Option<u64>>(4)?,"installation_id":r.get::<_,Option<u64>>(5)?,"branch":r.get::<_,Option<String>>(6)?,"synced_at":r.get::<_,Option<String>>(7)?,"error":r.get::<_,Option<String>>(8)?,"scheduler":r.get::<_,Option<String>>(9)?})))?.collect::<Result<Vec<_>,_>>()?;
    for project in &mut projects {
        let id = project["id"].as_str().unwrap().to_owned();
        project["roles"] = roles(home, &db, Some(&id))?;
        project["preflight"] =
            database::get::<Value>(&db, &format!("github_preflight:{id}"))?.unwrap_or(Value::Null);
    }
    let mut q=db.prepare("SELECT a.id,a.item_id,i.project_id,i.title,a.role,a.state,a.model_route,a.started_at,s.provider,s.model FROM attempts a JOIN work_items i ON i.id=a.item_id LEFT JOIN runs_summary s ON s.attempt_id=a.id WHERE i.kind='eos' AND a.state IN('queued','starting','running','waiting','idle') ORDER BY a.started_at,a.id")?;
    let mut active=q.query_map([],|r|Ok(json!({"id":r.get::<_,String>(0)?,"item":r.get::<_,String>(1)?,"project":r.get::<_,String>(2)?,"title":r.get::<_,String>(3)?,"role":r.get::<_,String>(4)?,"state":r.get::<_,String>(5)?,"route":r.get::<_,String>(6)?,"started_at":r.get::<_,String>(7)?,"provider":r.get::<_,Option<String>>(8)?,"model":r.get::<_,Option<String>>(9)?})))?.collect::<Result<Vec<_>,_>>()?;
    annotate_activity(&db, &mut active)?;
    Ok(
        json!({"projects":projects,"items":items(&db,None)?,"active":active,"roles":roles(home,&db,None)?}),
    )
}
pub fn item(home: &Path, id: &str) -> anyhow::Result<Value> {
    let db = database::open(home)?;
    let (project,repo,base,head):(String,Option<String>,String,String)=db.query_row("SELECT i.project_id,p.repo_full_name,w.base_sha,w.head_sha FROM work_items i JOIN projects p ON p.id=i.project_id JOIN workflow_items w ON w.item_id=i.id WHERE i.id=?1 AND i.kind='eos' AND i.deleted_at IS NULL",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)))?;
    let item = item_rows(&db, Some(&project), true)?
        .into_iter()
        .find(|v| v["id"] == id)
        .context("Work item not found")?;
    let mut q=db.prepare("SELECT a.id,a.role,a.generation,a.state,a.model_route,a.started_at,a.ended_at,json_extract(a.outcome_json,'$.summary'),s.provider,s.model,s.tokens_in,s.tokens_out,s.duration_ms FROM attempts a LEFT JOIN runs_summary s ON s.attempt_id=a.id WHERE a.item_id=?1 ORDER BY a.generation DESC LIMIT 200")?;
    let mut attempts=q.query_map([id],|r|Ok(json!({"id":r.get::<_,String>(0)?,"role":r.get::<_,String>(1)?,"generation":r.get::<_,u64>(2)?,"state":r.get::<_,String>(3)?,"route":r.get::<_,String>(4)?,"started_at":r.get::<_,String>(5)?,"ended_at":r.get::<_,Option<String>>(6)?,"summary":r.get::<_,Option<String>>(7)?,"provider":r.get::<_,Option<String>>(8)?,"model":r.get::<_,Option<String>>(9)?,"tokens_in":r.get::<_,Option<u64>>(10)?,"tokens_out":r.get::<_,Option<u64>>(11)?,"duration_ms":r.get::<_,Option<u64>>(12)?})))?.collect::<Result<Vec<_>,_>>()?;
    annotate_activity(&db, &mut attempts)?;
    let mut q=db.prepare("SELECT id,kind,state,target_digest,target_sha,github_comment_id,created_at FROM gates WHERE item_id=?1 ORDER BY created_at DESC LIMIT 100")?;
    let gates=q.query_map([id],|r|Ok(json!({"id":r.get::<_,String>(0)?,"kind":r.get::<_,String>(1)?,"state":r.get::<_,String>(2)?,"hash":r.get::<_,String>(3)?,"sha":r.get::<_,Option<String>>(4)?,"comment":r.get::<_,Option<u64>>(5)?,"at":r.get::<_,String>(6)?})))?.collect::<Result<Vec<_>,_>>()?;
    let mut q=db.prepare("SELECT id,at,action,json_extract(metadata_json,'$.step') FROM audit WHERE subject=?1 AND action IN('workflow.projected','workflow.started','workflow.recovery_needed','loop.result') ORDER BY id DESC LIMIT 200")?;
    let events=q.query_map([id],|r|Ok(json!({"id":r.get::<_,u64>(0)?,"at":r.get::<_,String>(1)?,"action":r.get::<_,String>(2)?,"step":r.get::<_,Option<String>>(3)?})))?.collect::<Result<Vec<_>,_>>()?;
    let facts = crate::workflow::state(&db, id)?.unwrap_or(Value::Null);
    let scan_sources = crate::loop_engine::scan_sources(&facts["loop_context"]);
    let scan_snapshot = facts["loop_context"]["default_sha"].clone();
    let pending:u64=db.query_row("SELECT count(*) FROM outbox WHERE aggregate_id=?1 AND kind='github.status' AND delivered_at IS NULL",[id],|r|r.get(0))?;
    Ok(
        json!({"scan_snapshot":scan_snapshot,"scan_sources":scan_sources,"notification_delivery":{"pending":pending},"item":item,"repository":repo,"base":base,"head":head,"attempts":attempts,"gates":gates,"events":events,"history_limit":200}),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn manual_done_column_preserves_workflow_approvals_and_checkpoints() {
        let t = tempfile::tempdir().unwrap();
        let db = database::open(t.path()).unwrap();
        db.execute(
            "INSERT INTO projects VALUES('p','o/r',NULL,'/checkout','eos','public','now')",
            [],
        )
        .unwrap();
        db.execute("INSERT INTO work_items(id,project_id,kind,title,state,created_at,repo_slug,slug,session_json) VALUES('i','p','eos','Issue','plan_gate','now','p','i','{}')", []).unwrap();
        db.execute("INSERT INTO workflow_items(item_id,state_json,base_sha,head_sha) VALUES('i','{\"step\":\"plan_gate\",\"plan_sha\":\"hash\"}','base','checkpoint')", []).unwrap();
        db.execute("INSERT INTO gates(id,item_id,kind,target_digest,state,created_at) VALUES('gate','i','plan','hash','open','now')", []).unwrap();
        let before: (String, String) = db
            .query_row(
                "SELECT state_json,head_sha FROM workflow_items WHERE item_id='i'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        organize(t.path(), "i", Some("done")).unwrap();
        assert_eq!(item(t.path(), "i").unwrap()["item"]["board_column"], "done");
        assert_eq!(
            db.query_row("SELECT state FROM gates WHERE id='gate'", [], |r| r
                .get::<_, String>(0))
                .unwrap(),
            "open"
        );
        assert_eq!(
            db.query_row(
                "SELECT state_json,head_sha FROM workflow_items WHERE item_id='i'",
                [],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            )
            .unwrap(),
            before
        );
        assert_eq!(
            db.query_row("SELECT count(*) FROM outbox", [], |r| r.get::<_, u64>(0))
                .unwrap(),
            0
        );
        organize(t.path(), "i", None).unwrap();
        assert_eq!(
            item(t.path(), "i").unwrap()["item"]["board_column"],
            Value::Null
        );
        assert!(organize(t.path(), "i", Some("approved")).is_err());
        assert!(organize(t.path(), "missing", Some("done")).is_err());
    }
    #[test]
    fn projects_and_item_history_are_scoped_and_do_not_expose_credentials() {
        let tmp = tempfile::tempdir().unwrap();
        let db = database::open(tmp.path()).unwrap();
        for p in ["one", "two"] {
            db.execute(
                "INSERT INTO projects VALUES(?1,?1,NULL,'/checkout','eos','private','now')",
                [p],
            )
            .unwrap();
            db.execute("INSERT INTO work_items(id,project_id,kind,title,state,created_at,repo_slug,slug,session_json) VALUES(?1,?1,'eos',?1,'triage','now',?1,?1,'{}')",[p]).unwrap();
            db.execute("INSERT INTO workflow_items(item_id,state_json,base_sha,head_sha) VALUES(?1,'{\"step\":\"triage\"}','base','head')",[p]).unwrap();
            db.execute("INSERT INTO attempts VALUES(?1,?1,'pm',1,'route','running','/checkout','now',NULL,NULL,?1,'{\"token\":\"do-not-expose\"}',0)",[p]).unwrap();
        }
        let value = overview(tmp.path()).unwrap();
        assert_eq!(value["projects"].as_array().unwrap().len(), 2);
        assert_eq!(value["active"].as_array().unwrap().len(), 2);
        assert!(!value.to_string().contains("do-not-expose"));
        let value = item(tmp.path(), "one").unwrap();
        assert_eq!(value["attempts"].as_array().unwrap().len(), 1);
        assert_eq!(value["attempts"][0]["id"], "one");
        assert!(!value.to_string().contains("do-not-expose"));
        db.execute("INSERT INTO leases(id,holder_attempt_id,holder_kind,model_route,acquired_at,expires_at) VALUES(1,'one','attempt:one','{}','now',9999999999)",[]).unwrap();
        db.execute("INSERT INTO model_queue(id,attempt_id,route,provider,model,urgent,sprint_order,enqueued_at,execution_generation) VALUES('other','two','route','ollama','m',0,0,1,0)",[]).unwrap();
        let active = overview(tmp.path()).unwrap()["active"]
            .as_array()
            .unwrap()
            .clone();
        assert_eq!(active.iter().filter(|r| r["state"] == "running").count(), 1);
        assert_eq!(
            active.iter().find(|r| r["id"] == "two").unwrap()["state"],
            "queued"
        );
        assert_eq!(
            item(tmp.path(), "two").unwrap()["attempts"][0]["state"],
            "queued"
        );
        assert!(item(tmp.path(), "unknown").is_err());
        db.execute("UPDATE workflow_items SET state_json='{\"step\":\"project_wait\",\"project\":true}' WHERE item_id='one'", []).unwrap();
        assert_eq!(
            item(tmp.path(), "one").unwrap()["item"]["step"],
            "project_wait"
        );
        assert!(!items(&db, Some("one"))
            .unwrap()
            .iter()
            .any(|i| i["id"] == "one"));
    }
}

/// Presentation preference only. This never changes execution, gates or sprint membership.
pub fn organize(home: &Path, item: &str, column: Option<&str>) -> anyhow::Result<Value> {
    anyhow::ensure!(
        column.is_none_or(|c| [
            "backlog",
            "planned",
            "in_progress",
            "review",
            "waiting",
            "done"
        ]
        .contains(&c)),
        "Unknown board column"
    );
    let mut db = database::open(home)?;
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let exists: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM work_items WHERE id=?1 AND kind='eos' AND deleted_at IS NULL)",
        [item],
        |r| r.get(0),
    )?;
    anyhow::ensure!(exists, "Unknown work item");
    database::set(&tx, &format!("board.column:{item}"), &column)?;
    database::audit(
        &tx,
        "human",
        "board.organized",
        item,
        &json!({"column":column}),
    )?;
    tx.commit()?;
    Ok(json!({"column":column}))
}
