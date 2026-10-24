//! Browser-only read models. No graph transitions, approvals or model calls.
use crate::database;
use anyhow::Context;
use rusqlite::Connection;
use serde_json::{json, Value};
use std::path::Path;
fn items(db: &Connection, project: Option<&str>) -> anyhow::Result<Vec<Value>> {
    let mut q=db.prepare("SELECT i.id,i.project_id,i.title,i.github_issue_number,i.state,i.priority,w.pr_number,w.last_error,json_extract(w.state_json,'$.frozen'),json_extract(w.state_json,'$.step'),(SELECT count(*) FROM gates g WHERE g.item_id=i.id AND g.state IN('open','pending')) FROM work_items i JOIN workflow_items w ON w.item_id=i.id WHERE i.kind='eos' AND i.deleted_at IS NULL AND coalesce(json_extract(w.state_json,'$.project'),0)=0 AND (?1 IS NULL OR i.project_id=?1) ORDER BY i.created_at DESC,i.id")?;
    let rows=q.query_map([project],|r|Ok(json!({"id":r.get::<_,String>(0)?,"project":r.get::<_,String>(1)?,"title":r.get::<_,String>(2)?,"issue":r.get::<_,Option<u64>>(3)?,"state":r.get::<_,String>(4)?,"priority":r.get::<_,Option<String>>(5)?,"pr":r.get::<_,Option<u64>>(6)?,"error":r.get::<_,Option<String>>(7)?,"frozen":r.get::<_,Option<bool>>(8)?.unwrap_or(false),"step":r.get::<_,Option<String>>(9)?.unwrap_or_default(),"waiting_gates":r.get::<_,u64>(10)?})))?.collect::<Result<Vec<_>,_>>()?;
    Ok(rows)
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
    let active=q.query_map([],|r|Ok(json!({"id":r.get::<_,String>(0)?,"item":r.get::<_,String>(1)?,"project":r.get::<_,String>(2)?,"title":r.get::<_,String>(3)?,"role":r.get::<_,String>(4)?,"state":r.get::<_,String>(5)?,"route":r.get::<_,String>(6)?,"started_at":r.get::<_,String>(7)?,"provider":r.get::<_,Option<String>>(8)?,"model":r.get::<_,Option<String>>(9)?})))?.collect::<Result<Vec<_>,_>>()?;
    Ok(
        json!({"projects":projects,"items":items(&db,None)?,"active":active,"roles":roles(home,&db,None)?}),
    )
}
pub fn item(home: &Path, id: &str) -> anyhow::Result<Value> {
    let db = database::open(home)?;
    let (project,repo,base,head):(String,Option<String>,String,String)=db.query_row("SELECT i.project_id,p.repo_full_name,w.base_sha,w.head_sha FROM work_items i JOIN projects p ON p.id=i.project_id JOIN workflow_items w ON w.item_id=i.id WHERE i.id=?1 AND i.kind='eos' AND i.deleted_at IS NULL",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)))?;
    let item = items(&db, Some(&project))?
        .into_iter()
        .find(|v| v["id"] == id)
        .context("Work item is a project scheduler, not an item")?;
    let mut q=db.prepare("SELECT a.id,a.role,a.generation,a.state,a.model_route,a.started_at,a.ended_at,json_extract(a.outcome_json,'$.summary'),s.provider,s.model,s.tokens_in,s.tokens_out,s.duration_ms FROM attempts a LEFT JOIN runs_summary s ON s.attempt_id=a.id WHERE a.item_id=?1 ORDER BY a.generation DESC LIMIT 200")?;
    let attempts=q.query_map([id],|r|Ok(json!({"id":r.get::<_,String>(0)?,"role":r.get::<_,String>(1)?,"generation":r.get::<_,u64>(2)?,"state":r.get::<_,String>(3)?,"route":r.get::<_,String>(4)?,"started_at":r.get::<_,String>(5)?,"ended_at":r.get::<_,Option<String>>(6)?,"summary":r.get::<_,Option<String>>(7)?,"provider":r.get::<_,Option<String>>(8)?,"model":r.get::<_,Option<String>>(9)?,"tokens_in":r.get::<_,Option<u64>>(10)?,"tokens_out":r.get::<_,Option<u64>>(11)?,"duration_ms":r.get::<_,Option<u64>>(12)?})))?.collect::<Result<Vec<_>,_>>()?;
    let mut q=db.prepare("SELECT id,kind,state,target_digest,target_sha,github_comment_id,created_at FROM gates WHERE item_id=?1 ORDER BY created_at DESC LIMIT 100")?;
    let gates=q.query_map([id],|r|Ok(json!({"id":r.get::<_,String>(0)?,"kind":r.get::<_,String>(1)?,"state":r.get::<_,String>(2)?,"hash":r.get::<_,String>(3)?,"sha":r.get::<_,Option<String>>(4)?,"comment":r.get::<_,Option<u64>>(5)?,"at":r.get::<_,String>(6)?})))?.collect::<Result<Vec<_>,_>>()?;
    let mut q=db.prepare("SELECT id,at,action,json_extract(metadata_json,'$.step') FROM audit WHERE subject=?1 AND action IN('workflow.projected','workflow.started','workflow.recovery_needed','loop.result') ORDER BY id DESC LIMIT 200")?;
    let events=q.query_map([id],|r|Ok(json!({"id":r.get::<_,u64>(0)?,"at":r.get::<_,String>(1)?,"action":r.get::<_,String>(2)?,"step":r.get::<_,Option<String>>(3)?})))?.collect::<Result<Vec<_>,_>>()?;
    Ok(
        json!({"item":item,"repository":repo,"base":base,"head":head,"attempts":attempts,"gates":gates,"events":events,"history_limit":200}),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
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
        assert!(item(tmp.path(), "unknown").is_err());
    }
}
