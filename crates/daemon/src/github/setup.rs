use super::{
    api::{self, Api},
    auth::AppConfig,
};
use crate::database;
use anyhow::{ensure, Context};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::Path;
#[derive(Clone, Serialize, Deserialize)]
pub struct Repository {
    pub project: String,
    pub app_id: u64,
    pub installation_id: u64,
    pub repo_id: u64,
    pub full_name: String,
    pub default_branch: String,
    pub app_bot_id: u64,
}
impl Repository {
    pub fn path(&self) -> anyhow::Result<String> {
        api::repo_path(&self.full_name)
    }
}
pub fn repositories(home: &Path) -> anyhow::Result<Vec<Repository>> {
    let db = database::open(home)?;
    let mut q=db.prepare("SELECT project_id,app_id,installation_id,repo_id,full_name,default_branch,app_bot_id FROM github_repositories ORDER BY project_id")?;
    let rows = q.query_map([], |r| {
        Ok(Repository {
            project: r.get(0)?,
            app_id: r.get(1)?,
            installation_id: r.get(2)?,
            repo_id: r.get(3)?,
            full_name: r.get(4)?,
            default_branch: r.get(5)?,
            app_bot_id: r.get(6)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}
pub async fn resolve_user(api: &dyn Api, login: &str) -> anyhow::Result<Value> {
    ensure!(
        !login.is_empty()
            && login.len() <= 39
            && login
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-'),
        "Invalid GitHub username"
    );
    let user = api::get(api, &format!("/users/{login}")).await?;
    ensure!(
        user["type"] == "User" && user["id"].as_u64().is_some_and(|n| n > 0),
        "Allowlist requires a human GitHub user"
    );
    ensure!(
        user["login"]
            .as_str()
            .is_some_and(|name| name.eq_ignore_ascii_case(login)),
        "GitHub username resolution mismatch"
    );
    Ok(user)
}
pub fn allowlist(home: &Path) -> anyhow::Result<Vec<Value>> {
    let db = database::open(home)?;
    let mut q =
        db.prepare("SELECT github_user_id,login_at_add FROM allowlist ORDER BY github_user_id")?;
    let rows = q.query_map([], |r| {
        Ok(json!({"id":r.get::<_,u64>(0)?,"login":r.get::<_,String>(1)?}))
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}
pub async fn change_allowlist(
    home: &Path,
    api: &dyn Api,
    login: &str,
    remove: bool,
) -> anyhow::Result<()> {
    let user = resolve_user(api, login).await?;
    let id = user["id"].as_u64().unwrap();
    let mut db = database::open(home)?;
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    if remove {
        let count: i64 = tx.query_row("SELECT count(*) FROM allowlist", [], |r| r.get(0))?;
        ensure!(count > 1, "Cannot remove the last approver");
        tx.execute("DELETE FROM allowlist WHERE github_user_id=?1", [id])?;
    } else {
        tx.execute(
            "INSERT OR IGNORE INTO allowlist VALUES(?1,?2,?3,'browser')",
            params![
                id,
                user["login"].as_str().context("Missing login")?,
                plantool_core::now()
            ],
        )?;
    }
    database::audit(
        &tx,
        "human",
        "github.allowlist",
        "global",
        &json!({"id":id,"login":user["login"],"removed":remove}),
    )?;
    tx.commit()?;
    Ok(())
}
pub async fn configure(
    home: &Path,
    api: &dyn Api,
    cfg: &AppConfig,
    full_name: &str,
    checkout: &Path,
) -> anyhow::Result<Value> {
    let detected = crate::git::detect_checkout(checkout).map_err(anyhow::Error::msg)?;
    let root = crate::git::main_root(&detected.common_dir).canonicalize()?;
    let origin =
        crate::git::git(&root, &["remote", "get-url", "origin"]).map_err(anyhow::Error::msg)?;
    let remote = if let Some(path) = origin.strip_prefix("git@github.com:") {
        path.trim_end_matches('/')
            .trim_end_matches(".git")
            .to_string()
    } else {
        let u = reqwest::Url::parse(&origin)?;
        ensure!(
            u.host_str() == Some("github.com"),
            "Checkout origin must be on github.com"
        );
        u.path()
            .trim_start_matches('/')
            .trim_end_matches('/')
            .trim_end_matches(".git")
            .to_string()
    };
    ensure!(
        remote.eq_ignore_ascii_case(full_name),
        "Checkout origin does not match the selected repository"
    );
    let path = api::repo_path(full_name)?;
    let repo = api::get(api, &path).await?;
    let id = repo["id"].as_u64().context("Missing repository ID")?;
    ensure!(
        repo["full_name"]
            .as_str()
            .is_some_and(|n| n.eq_ignore_ascii_case(full_name)),
        "Repository identity mismatch"
    );
    let branch = repo["default_branch"]
        .as_str()
        .context("Missing default branch")?;
    let app = api::get(api, &format!("/app/installations/{}", cfg.installation_id)).await?;
    ensure!(
        app["id"].as_u64() == Some(cfg.installation_id)
            && app["app_id"].as_u64() == Some(cfg.app_id),
        "Installation/App mismatch"
    );
    for (permission, level) in [
        ("issues", "write"),
        ("pull_requests", "write"),
        ("contents", "write"),
        ("checks", "write"),
        ("statuses", "read"),
        ("actions", "read"),
        ("administration", "read"),
        ("metadata", "read"),
    ] {
        let actual = app["permissions"][permission].as_str().unwrap_or("");
        ensure!(
            actual == level || actual == "write",
            "Missing App permission: {permission} ({level})"
        );
    }
    let slug = app["app_slug"].as_str().context("Missing App slug")?;
    ensure!(
        slug.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'),
        "Invalid App slug"
    );
    let bot = api::get(api, &format!("/users/{slug}%5Bbot%5D")).await?;
    ensure!(bot["type"] == "Bot", "Missing App bot identity");
    let bot_id = bot["id"].as_u64().context("Missing bot ID")?;
    let initial = resolve_user(api, "amritghimire").await?;
    api::labels(
        home,
        api,
        &path,
        &format!("{}:{}:{id}", cfg.app_id, cfg.installation_id),
    )
    .await?;
    let preflight = api::preflight(api, &path, branch).await?;
    let project = format!("github:{id}");
    let visibility = api::visibility(&repo)?;
    let mut db = database::open(home)?;
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    tx.execute("INSERT OR IGNORE INTO projects(id,repo_full_name,repo_id,local_checkout,mode,visibility,created_at) VALUES(?1,?2,?3,?4,'eos',?5,?6)",params![project,full_name,id,root.to_string_lossy(),visibility,plantool_core::now()])?;
    let existing: String = tx.query_row(
        "SELECT local_checkout FROM projects WHERE id=?1",
        [&project],
        |r| r.get(0),
    )?;
    ensure!(
        Path::new(&existing) == root,
        "Repository already configured with another checkout"
    );
    let previous: Option<String> = tx.query_row(
        "SELECT visibility FROM projects WHERE id=?1",
        [&project],
        |r| r.get(0),
    )?;
    if previous.as_deref() != Some(visibility) {
        tx.execute(
            "UPDATE projects SET visibility=?2 WHERE id=?1",
            params![project, visibility],
        )?;
        database::audit(
            &tx,
            "github",
            "github.visibility",
            &project,
            &json!({"from":previous,"to":visibility,"provider_policy_changed":false}),
        )?;
    }
    database::set(&tx, &format!("github_preflight:{project}"), &preflight)?;
    tx.execute("INSERT OR IGNORE INTO project_settings(project_id,version,effective_at,set_by) VALUES(?1,1,?2,'browser')",params![project,plantool_core::now()])?;
    // Seed only once; removing the initial member later must remain effective.
    if database::get::<bool>(&tx, "github_allowlist_initialized")? != Some(true) {
        tx.execute(
            "INSERT OR IGNORE INTO allowlist VALUES(?1,?2,?3,'setup')",
            params![
                initial["id"].as_u64(),
                initial["login"].as_str(),
                plantool_core::now()
            ],
        )?;
        database::set(&tx, "github_allowlist_initialized", &true)?;
    }
    tx.execute("INSERT INTO github_repositories(project_id,app_id,installation_id,repo_id,full_name,default_branch,app_bot_id) VALUES(?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(project_id) DO UPDATE SET app_id=excluded.app_id,installation_id=excluded.installation_id,default_branch=excluded.default_branch,app_bot_id=excluded.app_bot_id,full_name=excluded.full_name",params![project,cfg.app_id,cfg.installation_id,id,full_name,branch,bot_id])?;
    database::audit(
        &tx,
        "human",
        "github.setup",
        &project,
        &json!({"app_id":cfg.app_id,"installation_id":cfg.installation_id,"repo_id":id,"visibility":visibility,"preflight":preflight}),
    )?;
    tx.commit()?;
    Ok(json!({"project":project,"preflight":preflight,"visibility":visibility}))
}

pub async fn refresh_preflight(
    home: &Path,
    api: &dyn Api,
    repo: &Repository,
) -> anyhow::Result<Value> {
    let path = repo.path()?;
    let metadata = api::get(api, &path).await?;
    ensure!(
        metadata["id"].as_u64() == Some(repo.repo_id),
        "Repository numeric identity changed"
    );
    let branch = metadata["default_branch"]
        .as_str()
        .context("Missing default branch")?;
    let mut result = api::preflight(api, &path, branch).await?;
    result["checked_at"] = json!(plantool_core::now());
    result["default_branch"] = json!(branch);
    let mut db = database::open(home)?;
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    database::set(&tx, &format!("github_preflight:{}", repo.project), &result)?;
    tx.execute(
        "UPDATE github_repositories SET default_branch=?2 WHERE project_id=?1",
        params![repo.project, branch],
    )?;
    database::audit(&tx, "human", "github.preflight", &repo.project, &result)?;
    tx.commit()?;
    Ok(result)
}
