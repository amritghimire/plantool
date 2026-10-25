//! Informational issue comments use durable intent/observation, never gate authority.
use super::{
    api::Api,
    lifecycle::{pages, write},
    setup::Repository,
};
use crate::database;
use serde_json::{json, Value};
use std::path::Path;
pub async fn deliver(home: &Path, api: &dyn Api, repo: &Repository) -> anyhow::Result<()> {
    if crate::execution::Execution::load(home)?.status().stopped {
        return Ok(());
    }
    let db = database::open(home)?;
    let rows = {
        let mut q=db.prepare("SELECT o.id,o.aggregate_id,o.payload_json FROM outbox o JOIN work_items i ON i.id=o.aggregate_id WHERE o.kind='github.status' AND o.delivered_at IS NULL AND o.next_attempt_at<=?2 AND i.project_id=?1 ORDER BY o.created_at,o.id LIMIT 32")?;
        let rows = q
            .query_map(rusqlite::params![repo.project, database::unix_now()], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };
    for (id, item, raw) in rows {
        let payload: Value = serde_json::from_str(&raw)?;
        let issue = payload["issue"]
            .as_u64()
            .ok_or_else(|| anyhow::anyhow!("Status issue missing"))?;
        let path = format!("{}/issues/{issue}/comments", repo.path()?);
        let marker = format!("<!-- plantool-status:{id} -->");
        let result = async {
            let comments = pages(api, &path, None).await?;
            let observed = comments.into_iter().find(|c| {
                c["user"]["id"] == repo.app_bot_id
                    && c["body"].as_str().is_some_and(|b| b.contains(&marker))
            });
            let body = format!(
                "PlanTool status update (not an approval)\n\n{}\n\n{marker}",
                payload["body"].as_str().unwrap_or("")
            );
            write(
                home,
                api,
                &item,
                &format!("status:{id}"),
                "POST",
                &path,
                json!({"body":body}),
                observed,
                false,
            )
            .await
        }
        .await;
        match result {
            Ok(value) => database::finish_intent(home, &id, &value)?,
            Err(error) => {
                db.execute("UPDATE outbox SET attempts=attempts+1,next_attempt_at=?2+min(3600,(1 << min(attempts,10))) WHERE id=?1",rusqlite::params![id,database::unix_now()])?;
                tracing::warn!("Issue status delivery pending for {item}: {error}");
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::api::Reply;
    use super::*;
    use std::sync::{Arc, Mutex};
    struct Fake(Arc<Mutex<Vec<Value>>>);
    impl Api for Fake {
        fn request<'a>(
            &'a self,
            method: &'a str,
            path: &'a str,
            body: Option<Value>,
            _: Option<String>,
        ) -> futures::future::BoxFuture<'a, anyhow::Result<Reply>> {
            Box::pin(async move {
                assert!(path.starts_with("/repos/o/r/issues/1/comments"));
                if method == "GET" {
                    Ok(Reply {
                        status: 200,
                        etag: None,
                        next: false,
                        body: json!(*self.0.lock().unwrap()),
                    })
                } else {
                    assert_eq!(method, "POST");
                    let comment = json!({"id":11,"user":{"id":99},"body":body.unwrap()["body"]});
                    self.0.lock().unwrap().push(comment.clone());
                    Ok(Reply {
                        status: 201,
                        etag: None,
                        next: false,
                        body: comment,
                    })
                }
            })
        }
    }
    #[tokio::test]
    async fn frozen_status_is_mirrored_once_and_observed_after_ack_loss() {
        let t = tempfile::tempdir().unwrap();
        let db = database::open(t.path()).unwrap();
        db.execute(
            "INSERT INTO projects VALUES('p','o/r',42,'/checkout','eos','private','now')",
            [],
        )
        .unwrap();
        db.execute("INSERT INTO github_repositories(project_id,app_id,installation_id,repo_id,full_name,default_branch,app_bot_id) VALUES('p',1,2,42,'o/r','main',99)",[]).unwrap();
        db.execute("INSERT INTO work_items(id,project_id,kind,title,state,created_at,repo_slug,slug,session_json,github_issue_number) VALUES('i','p','eos','Issue','escalated','now','r','i','{}',1)",[]).unwrap();
        db.execute("INSERT INTO workflow_items(item_id,state_json,base_sha,head_sha) VALUES('i','{\"step\":\"escalated\",\"frozen\":true}','b','b')",[]).unwrap();
        for _ in 0..2 {
            crate::workspace::operational(
                &db,
                "i",
                "Blocked",
                "same",
                "Owner: inspect the failed read-only step",
            )
            .unwrap();
        }
        assert_eq!(
            db.query_row("SELECT count(*) FROM messages", [], |r| r.get::<_, u64>(0))
                .unwrap(),
            2
        );
        assert_eq!(
            db.query_row(
                "SELECT count(*) FROM outbox WHERE kind='github.status'",
                [],
                |r| r.get::<_, u64>(0)
            )
            .unwrap(),
            1
        );
        let api = Fake(Default::default());
        let repo = Repository {
            project: "p".into(),
            app_id: 1,
            installation_id: 2,
            repo_id: 42,
            full_name: "o/r".into(),
            default_branch: "main".into(),
            app_bot_id: 99,
        };
        deliver(t.path(), &api, &repo).await.unwrap();
        deliver(t.path(), &api, &repo).await.unwrap();
        assert_eq!(api.0.lock().unwrap().len(), 1);
        db.execute(
            "UPDATE outbox SET delivered_at=NULL WHERE kind='github.status'",
            [],
        )
        .unwrap();
        db.execute("UPDATE workflow_actions SET state='running',result_json=NULL WHERE kind LIKE 'status:%'",[]).unwrap();
        deliver(t.path(), &api, &repo).await.unwrap();
        assert_eq!(api.0.lock().unwrap().len(), 1);
        assert_eq!(
            db.query_row("SELECT count(*) FROM decisions", [], |r| r.get::<_, u64>(0))
                .unwrap(),
            0
        );
        assert!(crate::workflow::state(&db, "i").unwrap().unwrap()["frozen"] == true);
    }
}
