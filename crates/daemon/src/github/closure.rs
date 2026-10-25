//! Explicit GitHub issue closure is terminal locally; absence from a feed is not closure.
use crate::{database, dispatcher, workflow};
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};
use std::path::Path;
pub async fn sync(
    home: &Path,
    api: &dyn super::api::Api,
    repo: &super::setup::Repository,
    issue: &Value,
) -> anyhow::Result<()> {
    if issue["state"] == "closed" && issue["pull_request"].is_null() {
        let db = database::open(home)?;
        let linked:Option<(String,u64)>=db.query_row("SELECT i.id,w.pr_number FROM work_items i JOIN workflow_items w ON w.item_id=i.id WHERE i.project_id=?1 AND i.github_issue_number=?2 AND i.closed_at IS NULL AND w.pr_number IS NOT NULL",params![repo.project,issue["number"].as_u64()],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
        if let Some((item, pr)) = linked {
            if let Ok(response) = api
                .request("GET", &format!("{}/pulls/{pr}", repo.path()?), None, None)
                .await
            {
                if response.status == 200 && response.body["merged"] == true {
                    return super::lifecycle::merged(home, &item, &response.body);
                }
            }
        }
    }
    observe(home, &repo.project, issue)
}
pub fn observe(home: &Path, project: &str, issue: &Value) -> anyhow::Result<()> {
    if issue["state"] != "closed" || !issue["pull_request"].is_null() {
        return Ok(());
    }
    let Some(number) = issue["number"].as_u64() else {
        return Ok(());
    };
    let mut db = database::open(home)?;
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let item: Option<String> = tx.query_row("SELECT id FROM work_items WHERE project_id=?1 AND github_issue_number=?2 AND closed_at IS NULL", params![project,number], |r|r.get(0)).optional()?;
    let Some(item) = item else {
        return Ok(());
    };
    let running:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM attempts WHERE item_id=?1 AND state IN('starting','running','recovery_needed')) OR EXISTS(SELECT 1 FROM workflow_actions WHERE item_id=?1 AND state='running')", [&item], |r|r.get(0))?;
    tx.execute("UPDATE attempts SET state='cancelled',ended_at=?2 WHERE item_id=?1 AND state IN('queued','starting','running','waiting','idle','recovery_needed')",params![item,plantool_core::now()])?;
    tx.execute(
        "UPDATE item_generations SET generation=generation+1 WHERE item_id=?1",
        [&item],
    )?;
    tx.execute(
        "DELETE FROM model_queue WHERE attempt_id IN(SELECT id FROM attempts WHERE item_id=?1)",
        [&item],
    )?;
    tx.execute(
        "UPDATE sprint_items SET state='closed' WHERE item_id=?1",
        [&item],
    )?;
    if workflow::state(&tx, &format!("{project}:project"))?.is_some() {
        crate::scheduler::event(
            &tx,
            project,
            "item_closed",
            &json!({"item":item}),
            &format!("closed:{item}"),
        )?;
    }
    tx.execute("UPDATE gates SET state='superseded',closed_at=?2 WHERE item_id=?1 AND state IN('open','pending','approved')",params![item,plantool_core::now()])?;
    tx.execute("UPDATE outbox SET delivered_at=?2,result_json='{\"issue_closed\":true}' WHERE delivered_at IS NULL AND (aggregate_id=?1 OR aggregate_id IN(SELECT id FROM attempts WHERE item_id=?1))",params![item,plantool_core::now()])?;
    if let Some(mut state) = workflow::state(&tx, &item)? {
        state["step"] = json!("closed");
        state["frozen"] = json!(false);
        state["owner_question"] = Value::Null;
        state["closed_on_github"] = json!(true);
        state["closure_review_needed"] = json!(running);
        workflow::save(&tx, &item, &state)?;
    }
    tx.execute(
        "UPDATE work_items SET state='closed',closed_at=?2,outcome='cancelled' WHERE id=?1",
        params![item, database::unix_now()],
    )?;
    tx.execute(
        "UPDATE workflow_items SET last_error=NULL WHERE item_id=?1",
        [&item],
    )?;
    dispatcher::enqueue(
        &tx,
        &item,
        "worker.cancel",
        &json!({"thread_id":item,"idempotency_key":format!("issue-closed:{item}")}),
    )?;
    let thread = crate::workspace::item_thread(&tx, &item)?;
    crate::workspace::append(
        &tx,
        &thread,
        "daemon",
        if running {
            "Issue closed on GitHub. This work item is closed and further work is cancelled. A run was active: inspect its worktree and external actions before discarding or reusing changes. No changes are rolled back automatically."
        } else {
            "Issue closed on GitHub. This work item is closed; queued work and approval gates were cancelled. History is preserved."
        },
        &format!("issue-closed:{item}"),
    )?;
    database::audit(
        &tx,
        "github",
        "issue.closed",
        &item,
        &json!({"issue":number,"active_run":running}),
    )?;
    tx.commit()?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_close_cancels_work_preserves_history_and_cannot_be_refrozen() {
        let home = tempfile::tempdir().unwrap();
        let db = database::open(home.path()).unwrap();
        db.execute(
            "INSERT INTO projects VALUES('p','o/r',42,'/checkout','eos','public','now')",
            [],
        )
        .unwrap();
        db.execute("INSERT INTO work_items(id,project_id,kind,title,state,created_at,repo_slug,slug,session_json,github_issue_number) VALUES('i','p','eos','Issue','research','now','r','i','{}',1)",[]).unwrap();
        db.execute("INSERT INTO workflow_items(item_id,state_json,base_sha,head_sha) VALUES('i','{\"step\":\"research\",\"frozen\":true}','b','h')",[]).unwrap();
        db.execute("INSERT INTO item_generations VALUES('i',1)", [])
            .unwrap();
        db.execute("INSERT INTO attempts VALUES('a','i','cto',1,'route','running','/checkout','now',NULL,NULL,'attempt','{}',0)",[]).unwrap();
        observe(home.path(), "p", &json!({"number":1,"state":"open"})).unwrap();
        assert_eq!(
            workflow::state(&db, "i").unwrap().unwrap()["step"],
            "research"
        );
        observe(home.path(), "p", &json!({"number":1,"state":"closed"})).unwrap();
        observe(home.path(), "p", &json!({"number":1,"state":"closed"})).unwrap();
        let state = workflow::state(&db, "i").unwrap().unwrap();
        assert_eq!(state["step"], "closed");
        assert_eq!(state["frozen"], false);
        assert_eq!(
            db.query_row("SELECT state FROM attempts WHERE id='a'", [], |r| r
                .get::<_, String>(0))
                .unwrap(),
            "cancelled"
        );
        assert_eq!(
            db.query_row("SELECT count(*) FROM attempts", [], |r| r.get::<_, u64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            db.query_row(
                "SELECT generation FROM item_generations WHERE item_id='i'",
                [],
                |r| r.get::<_, u64>(0)
            )
            .unwrap(),
            2
        );
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM outbox WHERE kind='worker.cancel'",
                [],
                |r| r.get::<_, u64>(0)
            )
            .unwrap(),
            1
        );
        workflow::recover(home.path(), "a", "late worker timeout").unwrap();
        assert_eq!(
            workflow::state(&db, "i").unwrap().unwrap()["step"],
            "closed"
        );
    }
}
