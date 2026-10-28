//! Project-visible, turn-based discussions. Messages are context, never approval.
use crate::{database, dispatcher};
use anyhow::{ensure, Context};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde_json::{json, Value};
use std::path::Path;
pub const ROLES: &[&str] = &[
    "cto",
    "pm",
    "backend",
    "frontend",
    "devops",
    "qa",
    "reviewer",
    "docs",
    "supervisor",
];
pub fn mentions(body: &str) -> Vec<String> {
    let re = regex::Regex::new(r"(?i)@([a-z]+)\b").unwrap();
    let mut roles = vec![];
    for cap in re.captures_iter(body) {
        let role = cap[1].to_ascii_lowercase();
        if ROLES.contains(&role.as_str()) && !roles.contains(&role) {
            roles.push(role)
        }
    }
    roles
}
pub fn ensure_channels(tx: &Connection, project: &str) -> anyhow::Result<()> {
    for name in ["sprint", "general", "ai-activity"] {
        let id = format!("{project}:channel:{name}");
        tx.execute(
            "INSERT OR IGNORE INTO channels VALUES(?1,?2,?3)",
            params![id, project, name],
        )?;
        tx.execute(
            "INSERT OR IGNORE INTO threads(id,project_id,channel_id,title) VALUES(?1,?2,?3,?4)",
            params![format!("{id}:thread"), project, id, format!("#{name}")],
        )?;
    }
    tx.execute("INSERT OR IGNORE INTO threads(id,project_id,item_id,title) SELECT id||':thread',project_id,id,title FROM work_items WHERE project_id=?1 AND kind='eos' AND github_issue_number IS NOT NULL",[project])?;
    Ok(())
}
pub fn item_thread(tx: &Connection, item: &str) -> anyhow::Result<String> {
    let (project, _) = dispatcher::item(tx, item)?;
    ensure_channels(tx, &project)?;
    let id = format!("{item}:thread");
    tx.execute("INSERT OR IGNORE INTO threads(id,project_id,item_id,title) SELECT ?1,project_id,id,title FROM work_items WHERE id=?2",params![id,item])?;
    Ok(
        tx.query_row("SELECT id FROM threads WHERE item_id=?1", [item], |r| {
            r.get(0)
        })?,
    )
}
pub fn append(
    tx: &Connection,
    thread: &str,
    sender: &str,
    body: &str,
    key: &str,
) -> anyhow::Result<String> {
    ensure!(
        sender == "owner" || sender == "daemon" || ROLES.contains(&sender),
        "Invalid workspace sender"
    );
    ensure!(
        !body.trim().is_empty() && body.len() <= 64 * 1024,
        "Invalid message length"
    );
    ensure!(tx.query_row("SELECT EXISTS(SELECT 1 FROM threads WHERE id=?1 AND (item_id IS NOT NULL OR channel_id IS NOT NULL))",[thread],|r|r.get::<_,bool>(0))?,"No DMs; project-visible thread required");
    let id = plantool_core::sha256_hex(&format!("workspace:{thread}:{key}"));
    tx.execute("INSERT OR IGNORE INTO messages(id,thread_id,sender_role,body,mentions_json,dedupe_key,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![id,thread,sender,body,json!(mentions(body)).to_string(),key,plantool_core::now()])?;
    let previous: (String, String, String) = tx.query_row(
        "SELECT thread_id,sender_role,body FROM messages WHERE dedupe_key=?1",
        [key],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    ensure!(
        previous == (thread.into(), sender.into(), body.into()),
        "Message key reused for different content"
    );
    Ok(id)
}
pub fn begin(
    tx: &Transaction<'_>,
    item: &str,
    s: &mut Value,
    after: &str,
    body: &str,
) -> anyhow::Result<()> {
    let project = dispatcher::item(tx, item)?.0;
    ensure_channels(tx, &project)?;
    let thread = format!("{project}:channel:ai-activity:thread");
    let id = format!("{item}:topic:{}", s["sequence"].as_u64().unwrap_or(0));
    let found = mentions(body);
    let speaker = found.first().map(String::as_str).unwrap_or("pm");
    tx.execute(
        "INSERT INTO discussion_topics(id,thread_id,item_id,speaker) VALUES(?1,?2,?3,?4)",
        params![id, thread, item, speaker],
    )?;
    append(
        tx,
        &thread,
        if s["project"] == true {
            "daemon"
        } else {
            "cto"
        },
        body,
        &format!("{id}:opening"),
    )?;
    s["step"] = json!("discussion");
    s["discussion"] = json!({"id":id,"thread":thread,"round":0,"speaker":speaker,"participants":[],"after":after});
    Ok(())
}
pub fn validate(s: &Value, result: &plantool_contract::RoleResult) -> anyhow::Result<()> {
    ensure!(
        result.role == s["discussion"]["speaker"],
        "Wrong discussion speaker"
    );
    let Some(plantool_contract::RoleDetails::Discussion { body, decision }) = &result.details
    else {
        anyhow::bail!("Typed discussion result required")
    };
    ensure!(!body.trim().is_empty(), "Empty discussion turn");
    if s["discussion"]["round"].as_u64().unwrap_or(0) >= 3 {
        ensure!(
            decision.as_ref().is_some_and(|d| !d.trim().is_empty()),
            "Arbiter must close after three rounds"
        );
    } else {
        ensure!(
            decision.is_none(),
            "Discussion conclusions belong to the final arbiter"
        );
    }
    ensure!(
        result.tasks_done.is_empty() && result.artifacts.is_empty() && result.checks.is_empty(),
        "Discussion cannot implement tasks or claim execution"
    );
    Ok(())
}
pub fn record(
    tx: &Transaction<'_>,
    result: &plantool_contract::RoleResult,
    s: &mut Value,
) -> anyhow::Result<()> {
    let d = s["discussion"].clone();
    if result.outcome != plantool_contract::Outcome::Done {
        s["step"] = json!("escalated");
        s["frozen"] = json!(true);
        return Ok(());
    }
    let Some(plantool_contract::RoleDetails::Discussion { body, decision }) = &result.details
    else {
        anyhow::bail!("Discussion result missing")
    };
    append(
        tx,
        d["thread"].as_str().context("Missing thread")?,
        &result.role,
        body,
        &format!("turn:{}", result.attempt_id),
    )?;
    let round = d["round"].as_u64().unwrap_or(0);
    let normalized = body
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    if round < 3 && d["previous_body"].as_str() == Some(normalized.as_str()) {
        let summary = format!(
            "Discussion ended because no new information was added. Latest comment: {body}"
        );
        tx.execute(
            "UPDATE discussion_topics SET state='closed',summary=?2 WHERE id=?1",
            params![d["id"].as_str(), summary],
        )?;
        s["discussion_summary"] = json!(summary);
        s["step"] = d["after"].clone();
        append(
            tx,
            d["thread"].as_str().context("Missing thread")?,
            "daemon",
            "This round repeated the previous update. Moving on to prepare a proposal.",
            &format!("discussion-stalled:{}", result.attempt_id),
        )?;
        return Ok(());
    }
    let mut participants = d["participants"].as_array().cloned().unwrap_or_default();
    if !participants.contains(&json!(result.role)) {
        participants.push(json!(result.role))
    }
    if round >= 3 {
        let conclusion = decision.as_deref().context("Decision missing")?;
        tx.execute(
            "UPDATE discussion_topics SET state='closed',summary=?2,round=?3 WHERE id=?1",
            params![d["id"].as_str(), conclusion, round],
        )?;
        s["discussion_summary"] = json!(conclusion);
        s["step"] = d["after"].clone();
    } else {
        let next_round = round + 1;
        let found = mentions(body);
        let speaker = if next_round >= 3 {
            if participants.contains(&json!("pm")) {
                "supervisor"
            } else {
                "pm"
            }
        } else if found.len() == 1 {
            &found[0]
        } else {
            "pm"
        };
        s["discussion"]["speaker"] = json!(speaker);
        s["discussion"]["round"] = json!(next_round);
        s["discussion"]["participants"] = json!(participants);
        s["discussion"]["previous_body"] = json!(normalized);
        tx.execute("UPDATE discussion_topics SET speaker=?2,round=?3,participants_json=?4,sequence=sequence+1 WHERE id=?1",params![d["id"].as_str(),speaker,next_round,json!(participants).to_string()])?;
    }
    Ok(())
}
pub fn snapshot(home: &Path) -> anyhow::Result<Value> {
    let db = database::open(home)?;
    let mut q = db.prepare("SELECT id,repo_full_name FROM projects WHERE mode='eos'")?;
    let projects = q
        .query_map([], |r| {
            Ok(json!({"id":r.get::<_,String>(0)?,"name":r.get::<_,Option<String>>(1)?}))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut q=db.prepare("SELECT t.id,t.project_id,t.item_id,c.name,t.title FROM threads t LEFT JOIN channels c ON c.id=t.channel_id ORDER BY t.project_id,t.id")?;
    let mut threads=q.query_map([],|r|Ok(json!({"id":r.get::<_,String>(0)?,"project":r.get::<_,String>(1)?,"item":r.get::<_,Option<String>>(2)?,"channel":r.get::<_,Option<String>>(3)?,"title":r.get::<_,String>(4)?})))?.collect::<Result<Vec<_>,_>>()?;
    for t in &mut threads {
        if let Some(item) = t["item"].as_str().map(str::to_owned) {
            if let Some(state) = crate::workflow::state(&db, &item)? {
                let question = if state["owner_question"].is_object() {
                    Some(state["owner_question"].clone())
                } else {
                    blocker_question(&db, &item, &state)?
                };
                if let Some(question) = question {
                    t["question"] = question;
                    t["question"]["settling"] = json!(db.query_row("SELECT EXISTS(SELECT 1 FROM outbox WHERE aggregate_id=?1 AND delivered_at IS NULL AND kind LIKE 'worker.%')", [item], |r| r.get::<_,bool>(0))?);
                }
            }
        }
        let mut q=db.prepare("SELECT id,sender_role,body,mentions_json,created_at FROM messages WHERE thread_id=?1 ORDER BY created_at,rowid")?;
        t["messages"]=json!(q.query_map([t["id"].as_str().unwrap()],|r|Ok(json!({"id":r.get::<_,String>(0)?,"sender":r.get::<_,String>(1)?,"body":r.get::<_,String>(2)?,"mentions":serde_json::from_str::<Value>(&r.get::<_,String>(3)?).unwrap_or(json!([])),"at":r.get::<_,String>(4)?})))?.collect::<Result<Vec<_>,_>>()?);
    }
    Ok(json!({"projects":projects,"threads":threads}))
}
/// Frozen supervisor escalations with no approval gate, uncertain write, active
/// attempt or pending external effect get a deterministic workspace question.
/// The choices are guidance for a read-only supervisor review, never approvals.
pub(crate) fn blocker_question(
    db: &rusqlite::Connection,
    item: &str,
    state: &Value,
) -> anyhow::Result<Option<Value>> {
    if state["step"] != "escalated"
        || state["frozen"] != true
        || state["checkpoint_uncertain"] == true
    {
        return Ok(None);
    }
    let blocked: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM attempts WHERE item_id=?1 AND state IN('queued','starting','running','waiting','idle','recovery_needed')) OR EXISTS(SELECT 1 FROM gates WHERE item_id=?1 AND state IN('open','pending')) OR EXISTS(SELECT 1 FROM outbox WHERE aggregate_id=?1 AND delivered_at IS NULL AND kind NOT LIKE 'worker.%' AND kind<>'github.status')",
        [item],
        |r| r.get(0),
    )?;
    if blocked {
        return Ok(None);
    }
    let latest: Option<(String, Option<String>)> = db.query_row(
        "SELECT a.id,json_extract(a.outcome_json,'$.summary') FROM attempts a WHERE a.item_id=?1 ORDER BY a.generation DESC LIMIT 1",
        [item],
        |r| Ok((r.get(0)?, r.get(1)?)),
    ).optional()?;
    let Some((attempt, summary)) = latest else {
        return Ok(None);
    };
    let failure: Option<String> = db.query_row(
        "SELECT last_error FROM workflow_items WHERE item_id=?1",
        [item],
        |r| r.get(0),
    )?;
    let reason = summary
        .filter(|s| !s.trim().is_empty())
        .or(failure)
        .unwrap_or_else(|| {
            format!(
                "The {} step could not complete safely",
                state["failed_step"].as_str().unwrap_or("current")
            )
        });
    let reason = reason.trim().chars().take(500).collect::<String>();
    Ok(Some(json!({
        "id":format!("blocked:{attempt}"),
        "question":format!("Work is paused because: {reason}. What should the team try next? Your answer starts a read-only review; any revised plan or code changes still need the usual approval."),
        "options":[
            "Retry this step with the approved plan",
            "Try a different approach and show me a revised plan if scope changes",
            "I fixed the blocker; check the current evidence again",
            "I will explain another direction"
        ]
    })))
}
pub fn owner_reply(home: &Path, thread: &str, body: &str, key: &str) -> anyhow::Result<String> {
    reply_to_question(home, thread, body, key, None)
}
pub fn reply_to_question(
    home: &Path,
    thread: &str,
    body: &str,
    key: &str,
    question_id: Option<&str>,
) -> anyhow::Result<String> {
    let mut db = database::open(home)?;
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let (project,root):(String,String)=tx.query_row("SELECT p.id,p.local_checkout FROM threads t JOIN projects p ON p.id=t.project_id WHERE t.id=?1 AND p.mode='eos'",[thread],|r|Ok((r.get(0)?,r.get(1)?)))?;
    let root = Path::new(&root);
    crate::models::proxy::check_payload(
        root,
        &crate::sandbox::NeverSent::new(&crate::execution::Execution::load(home)?.rules(root))?,
        &json!({"body":body}),
    )?;
    let id = append(&tx, thread, "owner", body, key)?;
    if let Some(question_id) = question_id {
        let item: String = tx.query_row(
            "SELECT item_id FROM threads WHERE id=?1 AND item_id IS NOT NULL",
            [thread],
            |r| r.get(0),
        )?;
        let mut state = crate::workflow::state(&tx, &item)?.context("Workflow missing")?;
        // A replay of the same answer is harmless; a stale answer cannot wake a new question.
        if state["clarification_answer"]["message_id"] != id {
            let question = if state["owner_question"]["id"] == question_id
                && state["step"] == "owner_question"
            {
                Some(state["owner_question"].clone())
            } else {
                blocker_question(&tx, &item, &state)?.filter(|q| q["id"] == question_id)
            };
            ensure!(
                question.is_some(),
                "Question changed; refresh the thread before answering"
            );
            ensure!(
                state["step"] == "owner_question"
                    || (state["step"] == "escalated" && state["frozen"] == true),
                "This item no longer needs an answer; refresh the thread"
            );
            let blocked: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM attempts WHERE item_id=?1 AND state IN('queued','starting','running','waiting','idle','recovery_needed')) OR EXISTS(SELECT 1 FROM gates WHERE item_id=?1 AND state IN('open','pending')) OR EXISTS(SELECT 1 FROM outbox WHERE aggregate_id=?1 AND delivered_at IS NULL AND kind<>'github.status')", [&item], |r| r.get(0))?;
            ensure!(!blocked, "The question is still being recorded, or an active attempt, approval gate or uncertain external effect needs attention. Refresh and retry after pending workflow delivery completes");
            let question = question.unwrap();
            state["clarification_answer"] =
                json!({"question":question["question"],"answer":body,"message_id":id});
            state["owner_question"] = Value::Null;
            state["step"] = json!("supervisor");
            state["frozen"] = json!(false);
            state["supervisor_rounds"] = json!(0);
            state["sequence"] = json!(state["sequence"].as_u64().unwrap_or(0) + 1);
            crate::workflow::save(&tx, &item, &state)?;
            tx.execute("UPDATE outbox SET delivered_at=?2,result_json='{\"superseded_by_clarification\":true}' WHERE aggregate_id=?1 AND delivered_at IS NULL AND kind IN('worker.start','worker.resume')", params![item,plantool_core::now()])?;
            let start = plantool_contract::Start {
                thread_id: item.clone(),
                graph: plantool_contract::Graph::WorkItem,
                input: json!({"workflow":state}),
                idempotency_key: format!("clarification:{id}"),
                contract_version: plantool_contract::CONTRACT_VERSION,
                graph_version: plantool_contract::GRAPH_VERSION,
            };
            dispatcher::enqueue(&tx, &item, "worker.recover", &serde_json::to_value(start)?)?;
            append(&tx, thread, "daemon", "Thanks. The team will review your answer and continue planning when it can run. Any changes still need the usual approval.", &format!("answer:{id}"))?;
        }
    }
    database::audit(
        &tx,
        "human",
        "workspace.context",
        &id,
        &json!({"project":project,"thread":thread}),
    )?;
    tx.commit()?;
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use plantool_contract::{Outcome, RoleDetails, RoleResult};
    fn setup() -> (tempfile::TempDir, Connection, Value) {
        let temp = tempfile::tempdir().unwrap();
        let db = database::open(temp.path()).unwrap();
        db.execute(
            "INSERT INTO projects VALUES('p',NULL,NULL,?1,'eos','public','now')",
            [temp.path().to_string_lossy().as_ref()],
        )
        .unwrap();
        db.execute("INSERT INTO work_items(id,project_id,kind,title,state,created_at,repo_slug,slug,session_json) VALUES('p:1','p','eos','Item','research','now','p','item','{}')",[]).unwrap();
        (
            temp,
            db,
            json!({"step":"research","sequence":1,"generation":0}),
        )
    }
    #[test]
    fn routine_ai_events_stay_out_of_human_threads_and_material_notices_dedupe() {
        let (_, mut db, mut state) = setup();
        operational(
            &db,
            "p:1",
            "Model fallback",
            "fallback:a",
            "Trying another model",
        )
        .unwrap();
        operational(
            &db,
            "p:1",
            "Issue decision / status",
            "result:a",
            "A routine retry",
        )
        .unwrap();
        let human_count = || {
            db.query_row("SELECT count(*) FROM messages m JOIN threads t ON t.id=m.thread_id WHERE t.id<>'p:channel:ai-activity:thread'",[],|r|r.get::<_,u64>(0)).unwrap()
        };
        assert_eq!(human_count(), 0);
        operational(
            &db,
            "p:1",
            "Blocked",
            "attempt:a",
            "Failure needs reconciliation",
        )
        .unwrap();
        operational(
            &db,
            "p:1",
            "Blocked",
            "attempt:b",
            "Failure needs reconciliation",
        )
        .unwrap();
        assert_eq!(human_count(), 2); // One #general notice and one item notice, not two each.
        let tx = db.transaction().unwrap();
        begin(&tx, "p:1", &mut state, "plan", "@CTO inspect the evidence").unwrap();
        assert_eq!(
            state["discussion"]["thread"],
            "p:channel:ai-activity:thread"
        );
        tx.commit().unwrap();
        assert_eq!(
            db.query_row("SELECT count(*) FROM decisions", [], |r| r.get::<_, u64>(0))
                .unwrap(),
            0
        );
    }
    #[test]
    fn explicit_answer_wakes_only_matching_question_and_never_approves() {
        let (home, db, _) = setup();
        let question = json!({"item":"p:1","step":"owner_question","frozen":false,"sequence":2,"generation":0,"approved":false,"owner_question":{"id":"q1","question":"Document existing scripts only?"}});
        db.execute("INSERT INTO workflow_items(item_id,state_json,base_sha,head_sha) VALUES('p:1',?1,'base','base')", [question.to_string()]).unwrap();
        let thread = item_thread(&db, "p:1").unwrap();
        owner_reply(home.path(), &thread, "General context", "context1").unwrap();
        assert_eq!(
            crate::workflow::state(&db, "p:1").unwrap().unwrap()["step"],
            "owner_question"
        );
        assert!(reply_to_question(home.path(), &thread, "yes", "stale", Some("old")).is_err());
        let pending =
            dispatcher::enqueue(&db, "p:1", "worker.resume", &json!({"pending":true})).unwrap();
        assert!(reply_to_question(home.path(), &thread, "yes", "too-early", Some("q1")).is_err());
        database::finish_intent(home.path(), &pending, &json!({"recorded":true})).unwrap();
        let id = reply_to_question(
            home.path(),
            &thread,
            "Document existing scripts only",
            "answer1",
            Some("q1"),
        )
        .unwrap();
        assert_eq!(
            reply_to_question(
                home.path(),
                &thread,
                "Document existing scripts only",
                "answer1",
                Some("q1")
            )
            .unwrap(),
            id
        );
        let state = crate::workflow::state(&db, "p:1").unwrap().unwrap();
        assert_eq!(state["step"], "supervisor");
        assert_eq!(state["approved"], false);
        assert_eq!(
            state["clarification_answer"]["answer"],
            "Document existing scripts only"
        );
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM outbox WHERE kind='worker.recover'",
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
        assert!(
            reply_to_question(home.path(), &thread, "different", "answer2", Some("q1")).is_err()
        );
    }
    #[test]
    fn frozen_safe_escalation_offers_choices_and_answer_resumes_supervisor_without_approving() {
        let (home, db, _) = setup();
        let frozen = json!({"item":"p:1","project":"p","step":"escalated","failed_step":"qa","frozen":true,"sequence":7,"generation":2,"approved":true,"plan_sha":"approved-plan","checkpoint_uncertain":false});
        db.execute("INSERT INTO workflow_items(item_id,state_json,base_sha,head_sha) VALUES('p:1',?1,'base','head')", [frozen.to_string()]).unwrap();
        db.execute("INSERT INTO attempts(id,item_id,role,generation,model_route,state,worktree,started_at,idempotency_key,run_json,outcome_json) VALUES('attempt-7','p:1','supervisor',7,'route','completed','/checkout','now','attempt-7','{}',?1)", [json!({"summary":"QA could not verify the required check"}).to_string()]).unwrap();
        let thread = item_thread(&db, "p:1").unwrap();
        let question = snapshot(home.path()).unwrap()["threads"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["id"] == thread)
            .unwrap()["question"]
            .clone();
        assert_eq!(question["id"], "blocked:attempt-7");
        assert_eq!(question["options"].as_array().unwrap().len(), 4);
        let answer = reply_to_question(
            home.path(),
            &thread,
            question["options"][1].as_str().unwrap(),
            "blocker-answer-1",
            Some("blocked:attempt-7"),
        )
        .unwrap();
        let state = crate::workflow::state(&db, "p:1").unwrap().unwrap();
        assert_eq!(state["step"], "supervisor");
        assert_eq!(state["frozen"], false);
        assert_eq!(state["approved"], true);
        assert_eq!(state["plan_sha"], "approved-plan");
        assert_eq!(state["clarification_answer"]["message_id"], answer);
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM outbox WHERE aggregate_id='p:1' AND kind='worker.recover'",
                [],
                |r| r.get::<_, u64>(0)
            )
            .unwrap(),
            1
        );
    }
    fn result(role: &str, n: u64, body: &str, decision: Option<&str>) -> RoleResult {
        RoleResult {
            item: "p:1".into(),
            role: role.into(),
            attempt_id: format!("a{n}"),
            generation: n,
            outcome: Outcome::Done,
            summary: body.into(),
            tasks_done: vec![],
            artifacts: vec![],
            checks: vec![],
            findings: vec![],
            next: None,
            review_guide: None,
            details: Some(RoleDetails::Discussion {
                body: body.into(),
                decision: decision.map(str::to_owned),
            }),
        }
    }
    #[test]
    fn mentions_are_bounded_and_final_arbiter_excludes_pm_when_a_party() {
        for (first, expected) in [("@Backend", "pm"), ("@PM", "supervisor")] {
            let (_temp, mut db, mut s) = setup();
            let tx = db.transaction().unwrap();
            begin(&tx, "p:1", &mut s, "plan", first).unwrap();
            for (n, body) in [
                "@Frontend inspect UI",
                "@QA check failures",
                "@Backend one more turn",
            ]
            .iter()
            .enumerate()
            {
                let role = s["discussion"]["speaker"].as_str().unwrap().to_string();
                let r = result(&role, n as u64, body, None);
                validate(&s, &r).unwrap();
                record(&tx, &r, &mut s).unwrap();
            }
            assert_eq!(s["discussion"]["round"], 3);
            assert_eq!(s["discussion"]["speaker"], expected);
            let bad = result("backend", 9, "@Backend continue forever", None);
            assert!(validate(&s, &bad).is_err());
            let closing = result(
                expected,
                10,
                "Bounded conclusion",
                Some("Use existing contract; no approval granted"),
            );
            validate(&s, &closing).unwrap();
            record(&tx, &closing, &mut s).unwrap();
            assert_eq!(s["step"], "plan");
        }
    }
    #[test]
    fn owner_replies_are_only_context_and_no_dm_or_key_alias_is_possible() {
        let (temp, mut db, mut s) = setup();
        let tx = db.transaction().unwrap();
        begin(&tx, "p:1", &mut s, "plan", "@CTO inspect").unwrap();
        let thread = s["discussion"]["thread"].as_str().unwrap().to_string();
        tx.commit().unwrap();
        let before = s.clone();
        owner_reply(
            temp.path(),
            &thread,
            "/plantool approve-sprint",
            "reply-key",
        )
        .unwrap();
        owner_reply(
            temp.path(),
            &thread,
            "/plantool approve-sprint",
            "reply-key",
        )
        .unwrap();
        assert!(owner_reply(temp.path(), &thread, "different", "reply-key").is_err());
        assert_eq!(s, before);
        assert_eq!(
            db.query_row("SELECT count(*) FROM decisions", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
        db.execute(
            "INSERT INTO threads(id,project_id,title) VALUES('dm','p','DM')",
            [],
        )
        .unwrap();
        assert!(append(&db, "dm", "owner", "hi", "dm-key").is_err());
        assert_eq!(
            db.query_row("SELECT count(*) FROM channels", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            3
        );
    }
}

/// Operational context only. Never creates a decision, gate or graph event.
pub fn operational(
    db: &Connection,
    item: &str,
    event: &str,
    key: &str,
    detail: &str,
) -> anyhow::Result<()> {
    let (project, _) = dispatcher::item(db, item)?;
    ensure_channels(db, &project)?;
    let title: String = db.query_row("SELECT title FROM work_items WHERE id=?1", [item], |r| {
        r.get(0)
    })?;
    let url = if item == format!("{project}:project") {
        format!(
            "/engineering?project={}",
            percent_encoding::utf8_percent_encode(&project, percent_encoding::NON_ALPHANUMERIC)
        )
    } else {
        format!(
            "/engineering/items/{}",
            percent_encoding::utf8_percent_encode(item, percent_encoding::NON_ALPHANUMERIC)
        )
    };
    let label = match event {
        "Model fallback" => "Trying another model",
        "Recovery needed" => "Work needs attention",
        "Recovery queued" => "Retry queued",
        "Issue decision / status" => "Update",
        "Final issue decision" => "Decision",
        _ => event,
    };
    let body = format!("{label}: {title}\n{detail}\nOpen {url}");
    let activity_key = format!("ops:{item}:{key}");
    let exists: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM messages WHERE dedupe_key=?1)",
        [&activity_key],
        |r| r.get(0),
    )?;
    if !exists {
        append(
            db,
            &format!("{project}:channel:ai-activity:thread"),
            "daemon",
            &body,
            &activity_key,
        )?;
    }
    // Existing notices stay immutable. Do not republish old history as new alerts.
    let legacy: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM messages WHERE dedupe_key=?1)",
        [format!("item-ops:{item}:{key}")],
        |r| r.get(0),
    )?;
    if !legacy {
        if let Some(message) = human_notice(event, detail) {
            let human_body = format!("{title}\n{message}\nOpen {url}");
            // Repeated identical failures/recommendations add no new owner information.
            let identity = plantool_core::sha256_hex(&message);
            for thread in [
                format!("{project}:channel:general:thread"),
                item_thread(db, item)?,
            ] {
                let key = format!("notice:{thread}:{item}:{identity}");
                let seen: bool = db.query_row(
                    "SELECT EXISTS(SELECT 1 FROM messages WHERE dedupe_key=?1)",
                    [&key],
                    |r| r.get(0),
                )?;
                if !seen {
                    append(db, &thread, "daemon", &human_body, &key)?;
                }
            }
        }
    }
    let issue: Option<u64> = db.query_row(
        "SELECT github_issue_number FROM work_items WHERE id=?1",
        [item],
        |r| r.get(0),
    )?;
    if let Some(issue) = issue.filter(|_| {
        matches!(
            event,
            "Final issue decision" | "Recovery needed" | "Blocked"
        )
    }) {
        dispatcher::enqueue(
            db,
            item,
            "github.status",
            &json!({"key":format!("decision:{}",plantool_core::sha256_hex(detail)),"issue":issue,"body":format!("{label}\n\n{detail}"),"publish":true}),
        )?;
    }
    Ok(())
}

/// Routine AI events belong in #ai-activity; human channels carry decisions and
/// actions. Details remain available from the linked item and the activity ledger.
fn human_notice(event: &str, detail: &str) -> Option<String> {
    if let Some(kind) = event.strip_suffix(" gate open") {
        return Some(format!("Your {kind} proposal is ready to review. Open Approval gates for the current proposal and approval link."));
    }
    if let Some(kind) = event.strip_suffix(" gate approved") {
        return Some(format!(
            "Your {kind} approval was recorded. Follow this item for progress."
        ));
    }
    match event {
        "Recovery needed" | "Work remains frozen" | "Blocked" => Some("Work is paused because its last attempt could not be safely continued. Open the item to review the recovery action and preserved changes.".into()),
        "Final issue decision" => Some(detail.lines().filter(|line|!line.trim().is_empty()).take(2).collect::<Vec<_>>().join("\n").chars().take(600).collect()),
        _ => None,
    }
}

/// Short public explanation; raw errors remain in attempt diagnostics and the audit log.
pub fn plain_failure(error: &str) -> &'static str {
    if error.contains("429") {
        "The model provider is limiting requests. Another available model may be tried."
    } else if error.contains("No available, allowed route") {
        "No model is ready for this work. Check model availability and request limits in Settings."
    } else if error.contains("ReadTimeout") || error.to_ascii_lowercase().contains("timed out") {
        "The model took too long to reply. Check the attempt details for the next available model or retry."
    } else if error.contains("JSONDecodeError") || error.contains("invalid_role_result") {
        "The model's reply could not be used. Another available model may be tried."
    } else {
        "This attempt could not finish. Open the item to see the error and the next step."
    }
}
