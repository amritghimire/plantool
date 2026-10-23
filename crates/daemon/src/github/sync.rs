//! Webhook-less sync. Cursor advancement follows durable processing of every page.
use super::{
    api::{self, Api},
    gates,
    setup::Repository,
};
use crate::database;
use anyhow::{ensure, Context};
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};
use std::path::Path;
async fn collection(
    home: &Path,
    api: &dyn Api,
    repo: &Repository,
    path: &str,
    incremental: bool,
    kind: &str,
) -> anyhow::Result<Vec<Value>> {
    let key = format!("{}:{path}", repo.repo_id);
    let since: Option<String> = if incremental {
        database::open(home)?
            .query_row(
                "SELECT last_seen_at FROM github_cursors WHERE resource=?1",
                [&key],
                |r| r.get(0),
            )
            .optional()?
            .flatten()
    } else {
        None
    };
    let since_query = since
        .as_ref()
        .map(|s| {
            format!(
                "&since={}",
                percent_encoding::utf8_percent_encode(s, percent_encoding::NON_ALPHANUMERIC)
            )
        })
        .unwrap_or_default();
    let mut all = vec![];
    let mut cursors = vec![];
    for page in 1..=1000 {
        let uri = format!(
            "{path}{}per_page=100&page={page}{since_query}",
            if path.contains('?') { "&" } else { "?" }
        );
        let page_key = format!("{key}:page:{page}:{since_query}");
        let etag = database::open(home)?
            .query_row(
                "SELECT etag FROM github_cursors WHERE resource=?1",
                [&page_key],
                |r| r.get::<_, Option<String>>(0),
            )
            .optional()?
            .flatten();
        let r = api.request("GET", &uri, None, etag).await?;
        let values: Vec<Value> = if r.status == 304 {
            let saved =
                database::get::<Value>(&database::open(home)?, &format!("github_page:{page_key}"))?
                    .context("Missing cached page for 304")?;
            let values = saved["values"]
                .as_array()
                .context("Invalid cached page")?
                .clone();
            let next = saved["next"].as_bool().unwrap_or(false);
            all.extend(values);
            if !next {
                break;
            } else {
                continue;
            }
        } else {
            ensure!(r.status == 200, "GitHub poll failed ({})", r.status);
            if kind == "checks" {
                r.body["check_runs"].as_array()
            } else {
                r.body.as_array()
            }
            .context("Expected GitHub collection")?
            .clone()
        };
        let next = r.next;
        all.extend(values.clone());
        cursors.push((page_key, r.etag, values, next));
        if !next {
            break;
        }
        ensure!(page < 1000, "Too many pages to sync safely");
    }
    // Process commands before marking their page complete; refetch even on 304.
    if kind == "comments" {
        for c in &all {
            if c["body"]
                .as_str()
                .is_some_and(|b| b.starts_with("/plantool"))
            {
                if let Some(id) = c["id"].as_u64() {
                    gates::comment(home, api, repo, id).await?;
                }
            }
        }
    }
    let mut db = database::open(home)?;
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    for object in &all {
        let id = object["id"]
            .as_u64()
            .map(|x| x.to_string())
            .unwrap_or_else(|| plantool_core::sha256_hex(&object.to_string()));
        let event = format!(
            "{}:{id}:{}",
            repo.repo_id,
            plantool_core::sha256_hex(&object.to_string())
        );
        let fresh=tx.execute("INSERT OR IGNORE INTO inbox(source,external_event_id,processed_at,result) VALUES(?1,?2,?3,'{}')",params![format!("github:{kind}"),event,plantool_core::now()])?;
        if fresh > 0 {
            tx.execute("INSERT INTO github_objects VALUES(?1,?2,?3) ON CONFLICT(resource,object_id) DO UPDATE SET body_json=excluded.body_json",params![key,id,object.to_string()])?;
        }
    }
    for (page, etag, values, next) in cursors {
        tx.execute("INSERT INTO github_cursors VALUES(?1,?2,NULL) ON CONFLICT(resource) DO UPDATE SET etag=excluded.etag",params![page,etag])?;
        database::set(
            &tx,
            &format!("github_page:{page}"),
            &json!({"values":values,"next":next}),
        )?;
    }
    if incremental {
        let latest = all
            .iter()
            .filter_map(|x| x["updated_at"].as_str())
            .filter_map(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .max()
            .map(|t| {
                (t - chrono::Duration::seconds(1))
                    .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
            })
            .or(since);
        tx.execute("INSERT INTO github_cursors(resource,last_seen_at) VALUES(?1,?2) ON CONFLICT(resource) DO UPDATE SET last_seen_at=excluded.last_seen_at",params![key,latest])?;
    }
    tx.commit()?;
    Ok(all)
}
pub fn label_event(
    home: &Path,
    repo: &Repository,
    issue: i64,
    event: &Value,
) -> anyhow::Result<()> {
    let kind = event["event"].as_str().unwrap_or("");
    if !matches!(kind, "labeled" | "unlabeled") {
        return Ok(());
    }
    let Some(label) = event["label"]["name"].as_str() else {
        return Ok(());
    };
    if !api::LABELS.contains(&label) {
        return Ok(());
    }
    let Some(id) = event["id"].as_u64() else {
        return Ok(());
    };
    let actor = event["actor"]["id"].as_u64();
    let db = database::open(home)?;
    let authorized = kind == "labeled"
        && event["actor"]["type"] == "User"
        && actor.is_some_and(|id| {
            db.query_row(
                "SELECT EXISTS(SELECT 1 FROM allowlist WHERE github_user_id=?1)",
                [id],
                |r| r.get::<_, bool>(0),
            )
            .unwrap_or(false)
        });
    db.execute("INSERT INTO github_label_actors VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(project_id,issue_number,label) DO UPDATE SET actor_id=excluded.actor_id,authorized=excluded.authorized,event_id=excluded.event_id WHERE excluded.event_id>github_label_actors.event_id",params![repo.project,issue,label,actor,authorized,id])?;
    Ok(())
}
pub fn authorized_label(
    home: &Path,
    project: &str,
    issue: i64,
    label: &str,
) -> anyhow::Result<bool> {
    let db = database::open(home)?;
    Ok(db.query_row("SELECT EXISTS(SELECT 1 FROM github_label_actors l JOIN allowlist a ON a.github_user_id=l.actor_id WHERE l.project_id=?1 AND l.issue_number=?2 AND l.label=?3 AND l.authorized=1)",params![project,issue,label],|r|r.get(0))?)
}
async fn object(
    home: &Path,
    api: &dyn Api,
    repo: &Repository,
    path: &str,
) -> anyhow::Result<Value> {
    let key = format!("{}:{path}", repo.repo_id);
    let db = database::open(home)?;
    let tag = db
        .query_row(
            "SELECT etag FROM github_cursors WHERE resource=?1",
            [&key],
            |r| r.get::<_, Option<String>>(0),
        )
        .optional()?
        .flatten();
    let response = api.request("GET", path, None, tag).await?;
    if response.status == 304 {
        return database::get(&db, &format!("github_object:{key}"))?
            .context("Missing cached object");
    }
    ensure!(
        response.status == 200,
        "GitHub object poll failed ({})",
        response.status
    );
    let mut db = database::open(home)?;
    let tx = db.transaction()?;
    database::set(&tx, &format!("github_object:{key}"), &response.body)?;
    tx.execute("INSERT INTO github_cursors(resource,etag) VALUES(?1,?2) ON CONFLICT(resource) DO UPDATE SET etag=excluded.etag",params![key,response.etag])?;
    tx.commit()?;
    Ok(response.body)
}
pub async fn poll(home: &Path, api: &dyn Api, repo: &Repository) -> anyhow::Result<()> {
    let base = repo.path()?;
    // Visibility is deliberately unconditional on every sync.
    let metadata = api::get(api, &base).await?;
    ensure!(
        metadata["id"].as_u64() == Some(repo.repo_id),
        "Repository numeric identity changed"
    );
    let visibility = metadata["visibility"]
        .as_str()
        .unwrap_or(if metadata["private"] == true {
            "private"
        } else {
            "public"
        });
    {
        let mut db = database::open(home)?;
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let old: Option<String> = tx.query_row(
            "SELECT visibility FROM projects WHERE id=?1",
            [&repo.project],
            |r| r.get(0),
        )?;
        if old.as_deref() != Some(visibility) {
            tx.execute(
                "UPDATE projects SET visibility=?2 WHERE id=?1",
                params![repo.project, visibility],
            )?;
            database::audit(
                &tx,
                "github",
                "github.visibility",
                &repo.project,
                &json!({"from":old,"to":visibility,"provider_policy_changed":false}),
            )?;
        }
        tx.commit()?;
    }
    let issues = collection(
        home,
        api,
        repo,
        &format!("{base}/issues?state=all&sort=updated&direction=asc"),
        true,
        "issues",
    )
    .await?;
    let _ = collection(
        home,
        api,
        repo,
        &format!("{base}/issues/comments?sort=updated&direction=asc"),
        true,
        "comments",
    )
    .await?;
    let mut numbers: Vec<i64> = issues.iter().filter_map(|x| x["number"].as_i64()).collect();
    {
        let db = database::open(home)?;
        let mut q=db.prepare("SELECT github_issue_number FROM work_items WHERE project_id=?1 AND github_issue_number IS NOT NULL AND closed_at IS NULL")?;
        for n in q.query_map([&repo.project], |r| r.get::<_, i64>(0))? {
            numbers.push(n?);
        }
    }
    numbers.sort_unstable();
    numbers.dedup();
    for number in numbers {
        let events = collection(
            home,
            api,
            repo,
            &format!("{base}/issues/{number}/events"),
            false,
            "events",
        )
        .await?;
        for e in events {
            label_event(home, repo, number, &e)?;
        }
    }
    let pulls = collection(
        home,
        api,
        repo,
        &format!("{base}/pulls?state=open"),
        false,
        "pulls",
    )
    .await?;
    let mut pull_numbers: Vec<u64> = pulls.iter().filter_map(|p| p["number"].as_u64()).collect();
    {
        let db = database::open(home)?;
        let mut q=db.prepare("SELECT object_id FROM github_objects WHERE resource=?1 AND json_extract(body_json,'$.state')='open'")?;
        for id in q.query_map([format!("{}:pr_state", repo.repo_id)], |r| {
            r.get::<_, String>(0)
        })? {
            if let Ok(n) = id?.parse() {
                pull_numbers.push(n)
            }
        }
    }
    pull_numbers.sort_unstable();
    pull_numbers.dedup();
    for n in pull_numbers {
        let state = object(home, api, repo, &format!("{base}/pulls/{n}")).await?;
        let _ = collection(
            home,
            api,
            repo,
            &format!("{base}/pulls/{n}/reviews"),
            false,
            "reviews",
        )
        .await?;
        if let Some(sha) = state["head"]["sha"].as_str() {
            ensure!(
                sha.len() == 40 && sha.bytes().all(|b| b.is_ascii_hexdigit()),
                "Invalid PR head"
            );
            let _ = collection(
                home,
                api,
                repo,
                &format!("{base}/commits/{sha}/check-runs"),
                false,
                "checks",
            )
            .await?;
        }
        database::open(home)?.execute("INSERT INTO github_objects VALUES(?1,?2,?3) ON CONFLICT(resource,object_id) DO UPDATE SET body_json=excluded.body_json",params![format!("{}:pr_state",repo.repo_id),n.to_string(),state.to_string()])?;
    }
    gates::publish(home, api, repo).await?;
    database::open(home)?.execute(
        "UPDATE github_repositories SET last_sync_at=?2,last_error=NULL WHERE project_id=?1",
        params![repo.project, plantool_core::now()],
    )?;
    Ok(())
}
pub async fn tick(home: &Path) -> anyhow::Result<()> {
    for r in super::setup::repositories(home)? {
        let client = super::api::Client::new(
            super::auth::AppConfig {
                app_id: r.app_id,
                installation_id: r.installation_id,
            },
            std::sync::Arc::new(super::auth::Keychain),
        )?;
        if let Err(e) = poll(home, &client, &r).await {
            database::open(home)?.execute(
                "UPDATE github_repositories SET last_error=?2 WHERE project_id=?1",
                params![r.project, e.to_string()],
            )?;
            tracing::warn!(project=%r.project,"GitHub sync pending: {e}");
        }
    }
    Ok(())
}
