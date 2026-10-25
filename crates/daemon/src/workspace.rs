//! Project-visible, turn-based discussions. Messages are context, never approval.
use crate::{database, dispatcher};
use anyhow::{ensure, Context};
use rusqlite::{params, Connection, Transaction};
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
    for name in ["sprint", "general"] {
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
    let thread = if s["project"] == true {
        let project = dispatcher::item(tx, item)?.0;
        ensure_channels(tx, &project)?;
        format!("{project}:channel:sprint:thread")
    } else {
        item_thread(tx, item)?
    };
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
        let mut q=db.prepare("SELECT id,sender_role,body,mentions_json,created_at FROM messages WHERE thread_id=?1 ORDER BY created_at,rowid")?;
        t["messages"]=json!(q.query_map([t["id"].as_str().unwrap()],|r|Ok(json!({"id":r.get::<_,String>(0)?,"sender":r.get::<_,String>(1)?,"body":r.get::<_,String>(2)?,"mentions":serde_json::from_str::<Value>(&r.get::<_,String>(3)?).unwrap_or(json!([])),"at":r.get::<_,String>(4)?})))?.collect::<Result<Vec<_>,_>>()?);
    }
    Ok(json!({"projects":projects,"threads":threads}))
}
pub fn owner_reply(home: &Path, thread: &str, body: &str, key: &str) -> anyhow::Result<String> {
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
            2
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
    let body=format!("{event}: {title} ({item}).\n{detail}\nOpen {url}\nThis message is status/context only, never an approval.");
    append(
        db,
        &format!("{project}:channel:general:thread"),
        "daemon",
        &body,
        &format!("ops:{item}:{key}"),
    )?;
    let thread = item_thread(db, item)?;
    append(
        db,
        &thread,
        "daemon",
        &body,
        &format!("item-ops:{item}:{key}"),
    )?;
    let issue: Option<u64> = db.query_row(
        "SELECT github_issue_number FROM work_items WHERE id=?1",
        [item],
        |r| r.get(0),
    )?;
    if let Some(issue) = issue {
        dispatcher::enqueue(
            db,
            item,
            "github.status",
            &json!({"key":key,"issue":issue,"body":body}),
        )?;
    }
    Ok(())
}
