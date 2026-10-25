//! Project sprint board and an explicit planning trigger. Neither approves an issue.
use crate::{database, scheduler, workflow};
use anyhow::{ensure, Context};
use chrono::Datelike;
use rusqlite::params;
use serde_json::{json, Value};
use std::path::Path;
pub fn week(
    db: &rusqlite::Connection,
    project: &str,
    now: chrono::DateTime<chrono::Utc>,
) -> anyhow::Result<String> {
    let zone:String=db.query_row("SELECT coalesce(timezone,'UTC') FROM project_settings WHERE project_id=?1 ORDER BY version DESC LIMIT 1",[project],|r|r.get(0))?;
    let zone: chrono_tz::Tz = zone
        .parse()
        .map_err(|_| anyhow::anyhow!("Unknown project timezone"))?;
    let local = now.with_timezone(&zone).iso_week();
    Ok(format!("{:04}-W{:02}", local.year(), local.week()))
}
pub fn view(home: &Path, project: &str) -> anyhow::Result<Value> {
    let db = database::open(home)?;
    let name: String = db.query_row(
        "SELECT repo_full_name FROM projects WHERE id=?1 AND mode='eos'",
        [project],
        |r| r.get(0),
    )?;
    let current = week(&db, project, chrono::Utc::now())?;
    let mut q=db.prepare("SELECT id,week,state,tracking_issue_number FROM sprints WHERE project_id=?1 ORDER BY week DESC LIMIT 26")?;
    let mut sprints=q.query_map([project],|r|Ok(json!({"id":r.get::<_,String>(0)?,"week":r.get::<_,String>(1)?,"state":r.get::<_,String>(2)?,"issue":r.get::<_,Option<u64>>(3)?})))?.collect::<Result<Vec<_>,_>>()?;
    for sprint in &mut sprints {
        let id = sprint["id"].as_str().unwrap();
        let mut q=db.prepare("SELECT i.id,i.github_issue_number,i.title,i.state,si.state,si.estimate,si.plan_sha,w.pr_number,coalesce(json_extract(w.state_json,'$.frozen'),0) FROM sprint_items si JOIN work_items i ON i.id=si.item_id LEFT JOIN workflow_items w ON w.item_id=i.id WHERE si.sprint_id=?1 AND si.state<>'backlog' ORDER BY i.queue_order,i.created_at,i.id")?;
        let items=q.query_map([id],|r|Ok(json!({"id":r.get::<_,String>(0)?,"issue":r.get::<_,Option<u64>>(1)?,"title":r.get::<_,String>(2)?,"step":r.get::<_,String>(3)?,"membership":r.get::<_,String>(4)?,"estimate":r.get::<_,String>(5)?,"plan_hash":r.get::<_,String>(6)?,"pr":r.get::<_,Option<u64>>(7)?,"frozen":r.get::<_,bool>(8)?})))?.collect::<Result<Vec<_>,_>>()?;
        sprint["items"] = json!(items);
    }
    let backlog:u64=db.query_row("SELECT count(*) FROM work_items WHERE project_id=?1 AND github_issue_number IS NOT NULL AND closed_at IS NULL AND (sprint_id IS NULL OR NOT EXISTS(SELECT 1 FROM sprint_items si WHERE si.item_id=work_items.id AND si.sprint_id=work_items.sprint_id AND si.state IN('pending','approved','started')))",[project],|r|r.get(0))?;
    let pending:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM project_events WHERE project_id=?1 AND kind='sprint_planning_due' AND delivered_at IS NULL)",[project],|r|r.get(0))?;
    Ok(
        json!({"project":project,"repository":name,"current_week":current,"sprints":sprints,"backlog_count":backlog,"planning_queued":pending || crate::workflow::state(&db,&format!("{project}:project"))?.is_some_and(|s|matches!(s["step"].as_str(),Some("discussion"|"sprint_plan"|"publish_plans")))}),
    )
}
pub fn trigger(home: &Path, project: &str, key: &str) -> anyhow::Result<Value> {
    ensure!(
        !key.is_empty() && key.len() <= 200,
        "Invalid planning request"
    );
    let item = scheduler::ensure_project(home, project)?;
    database::inbox_once(home, "sprint.manual", &format!("{project}:{key}"), |tx| {
        let state = workflow::state(tx, &item)?.context("Project scheduler missing")?;
        ensure!(
            state["frozen"] != true,
            "The project scheduler is paused. Use its recovery action first."
        );
        let week = week(tx, project, chrono::Utc::now())?;
        let (signature, _) = scheduler::schedule(tx, project, chrono::Utc::now())?;
        // Coalesce repeated clicks/requests while a planning event is already queued.
        let pending:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM project_events WHERE project_id=?1 AND kind='sprint_planning_due' AND delivered_at IS NULL AND json_extract(payload_json,'$.week')=?2)",params![project,week],|r|r.get(0))?;
        if !pending
            && !matches!(
                state["step"].as_str(),
                Some("discussion" | "sprint_plan" | "publish_plans")
            )
        {
            scheduler::event(
                tx,
                project,
                "sprint_planning_due",
                &json!({"week":week,"schedule":signature,"manual":true}),
                &format!("manual-sprint:{week}:{key}"),
            )?;
        }
        database::set(tx, &format!("manual_sprint_week:{project}"), &week)?;
        let mut q=tx.prepare("SELECT id FROM work_items WHERE project_id=?1 AND state='backlog' AND closed_at IS NULL AND github_issue_number IS NOT NULL")?;
        let backlog = q
            .query_map([project], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        for item in backlog {
            crate::loop_engine::wake(
                tx,
                &item,
                "research",
                &format!("manual-prepare:{week}:{key}"),
            )?;
        }
        database::audit(
            tx,
            "human",
            "sprint.planning_requested",
            project,
            &json!({"week":week}),
        )?;
        Ok(
            json!({"queued":true,"week":week,"message":"Planning is queued. Approved work will continue; proposed changes still need approval."}),
        )
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn trigger_is_idempotent_and_does_not_approve_or_change_running_membership() {
        let home = tempfile::tempdir().unwrap();
        let db = database::open(home.path()).unwrap();
        db.execute(
            "INSERT INTO projects VALUES('p','o/r',42,'/checkout','eos','public','now')",
            [],
        )
        .unwrap();
        db.execute("INSERT INTO project_settings(project_id,version,timezone,effective_at,set_by) VALUES('p',1,'Asia/Kathmandu','now','owner')",[]).unwrap();
        db.execute("INSERT INTO work_items(id,project_id,kind,title,state,created_at,repo_slug,slug,session_json) VALUES('p:project','p','eos','Project scheduler','project_wait','now','p','project','{}')",[]).unwrap();
        db.execute("INSERT INTO workflow_items(item_id,state_json,base_sha,head_sha) VALUES('p:project','{\"project\":true,\"step\":\"project_wait\",\"frozen\":false}','b','b')",[]).unwrap();
        let current = week(&db, "p", chrono::Utc::now()).unwrap();
        db.execute(
            "INSERT INTO sprints(id,project_id,week,state) VALUES('s','p',?1,'active')",
            [&current],
        )
        .unwrap();
        db.execute("INSERT INTO work_items(id,project_id,kind,title,state,created_at,repo_slug,slug,session_json,github_issue_number,sprint_id) VALUES('work','p','eos','In progress','backend','now','p','work','{}',1,'s')",[]).unwrap();
        db.execute("INSERT INTO sprint_items(sprint_id,item_id,plan_sha,estimate,state) VALUES('s','work','unchanged-hash','Small','started')",[]).unwrap();
        trigger(home.path(), "p", "click1").unwrap();
        trigger(home.path(), "p", "click1").unwrap();
        trigger(home.path(), "p", "click2").unwrap();
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM project_events WHERE kind='sprint_planning_due'",
                [],
                |r| r.get::<_, u64>(0)
            )
            .unwrap(),
            1
        );
        assert_eq!(
            db.query_row("SELECT count(*) FROM decisions", [], |r| r.get::<_, u64>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            db.query_row(
                "SELECT state FROM sprint_items WHERE item_id='work'",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
            "started"
        );
        scheduler::wait(
            home.path(),
            &plantool_contract::ProjectWait {
                item: "p:project".into(),
                thread_id: "p:project".into(),
                interrupt_id: "wait".into(),
                kind: "project".into(),
                idempotency_key: "wait".into(),
            },
        )
        .unwrap();
        let event: String = db
            .query_row(
                "SELECT id FROM project_events WHERE kind='sprint_planning_due'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        scheduler::ack(
            home.path(),
            &plantool_contract::EventAck {
                project: "p".into(),
                event_id: event,
                next_step: Some("sprint_plan".into()),
                idempotency_key: "ack".into(),
            },
        )
        .unwrap();
        assert_eq!(
            workflow::state(&db, "p:project").unwrap().unwrap()["step"],
            "sprint_plan"
        );
        assert_eq!(
            view(home.path(), "p").unwrap()["sprints"][0]["items"][0]["title"],
            "In progress"
        );
        assert!(scheduler::planning_open(&db, "p", chrono::Utc::now()).unwrap());
        let before = chrono::DateTime::parse_from_rfc3339("2026-10-25T18:20:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        assert_eq!(week(&db, "p", before).unwrap(), "2026-W44");
        assert_eq!(view(home.path(), "p").unwrap()["planning_queued"], true);
        assert!(view(home.path(), "another").is_err());
    }
}
