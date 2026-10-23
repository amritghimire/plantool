//! Opt-in integration with the disposable playground only. Never run in normal CI.
use plantool_daemon::{
    artifacts, database,
    github::{
        api::{self, Api},
        auth::{AppConfig, Keychain},
        gates, setup, sync,
    },
};
use rusqlite::params;
use serde_json::{json, Value};
use std::sync::Arc;
fn gh(args: &[&str]) -> anyhow::Result<Value> {
    let out = std::process::Command::new("gh").args(args).output()?;
    anyhow::ensure!(out.status.success(), "gh test-user API call failed");
    Ok(serde_json::from_slice(&out.stdout)?)
}
#[tokio::test]
async fn playground_plan_publication_and_verified_human_approval() -> anyhow::Result<()> {
    if std::env::var("PLANTOOL_E2E_GITHUB").as_deref() != Ok("1") {
        eprintln!("Skipped: set PLANTOOL_E2E_GITHUB=1 to opt into playground writes");
        return Ok(());
    }
    let cfg = AppConfig {
        app_id: std::env::var("PLANTOOL_E2E_APP_ID")?.parse()?,
        installation_id: std::env::var("PLANTOOL_E2E_INSTALLATION_ID")?.parse()?,
    };
    let client = api::Client::new(cfg.clone(), Arc::new(Keychain))?;
    let name = "amritghimire/plantool-playground";
    let path = api::repo_path(name)?;
    // Owner test credentials are used by this test runner only, never by the daemon/worker.
    let user = gh(&["api", "user"])?;
    anyhow::ensure!(
        user["login"] == "amritghimire" && user["type"] == "User",
        "gh must be logged in as amritghimire for this playground test"
    );
    let tmp = tempfile::tempdir()?;
    let home = tmp.path().join("home");
    let root = tmp.path().join("checkout");
    std::fs::create_dir_all(&root)?;
    for args in [
        vec!["init", "-q", "-b", "main"],
        vec![
            "remote",
            "add",
            "origin",
            "https://github.com/amritghimire/plantool-playground.git",
        ],
    ] {
        anyhow::ensure!(
            std::process::Command::new("git")
                .args(args)
                .current_dir(&root)
                .status()?
                .success(),
            "Test checkout setup failed"
        )
    }
    setup::configure(&home, &client, &cfg, name, &root).await?;
    let repo = setup::repositories(&home)?.remove(0);
    let created=client.request("POST",&format!("{path}/issues"),Some(json!({"title":"[PlanTool e2e] GitHub plan gate","body":"Disposable integration test; no code, PR or model calls."})),None).await?;
    anyhow::ensure!(created.status == 201, "Cannot create playground test issue");
    let number = created.body["number"]
        .as_u64()
        .ok_or_else(|| anyhow::anyhow!("Missing issue number"))?;
    let result: anyhow::Result<()>=async {
 let db=database::open(&home)?;db.execute("INSERT INTO work_items(id,project_id,kind,title,github_issue_number,created_at,repo_slug,slug,session_json) VALUES('e2e',?1,'eos','test',?2,?3,'playground','e2e','{}')",params![repo.project,number,plantool_core::now()])?;
 let plan=b"# E2E plan\n- [ ] T1 [backend] verify GitHub integration; no implementation\n";let sha=artifacts::put(&home,plan)?;artifacts::register(&db,&sha,plan.len())?;db.execute("INSERT INTO revisions VALUES('e2e-plan','e2e','plan',1,?1,'test',?2)",params![sha,plantool_core::now()])?;
 let mut db=database::open(&home)?;let tx=db.transaction()?;let gate=gates::request(&tx,"e2e",&sha,"e2e-thread","e2e-interrupt")?;tx.commit()?;gates::publish(&home,&client,&repo).await?;
 // The App's own forged approval must not count, even with the right hash.
 let command=format!("/plantool approve-plan sha256:{}",&sha[..12]);let forged=client.request("POST",&format!("{path}/issues/{number}/comments"),Some(json!({"body":command})),None).await?;anyhow::ensure!(forged.status==201,"Cannot post bot test comment");gates::comment(&home,&client,&repo,forged.body["id"].as_u64().unwrap()).await?;
 anyhow::ensure!(database::open(&home)?.query_row("SELECT count(*) FROM decisions",[],|r|r.get::<_,i64>(0))?==0,"Bot approval counted");
 tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
 let endpoint=format!("repos/{name}/issues/{number}/comments");let body=format!("body={command}");let approved=gh(&["api","--method","POST",&endpoint,"-f",&body])?;let id=approved["id"].as_u64().ok_or_else(||anyhow::anyhow!("Missing approval ID"))?;
 gates::comment(&home,&client,&repo,id).await?;gates::comment(&home,&client,&repo,id).await?;sync::poll(&home,&client,&repo).await?;
 let db=database::open(&home)?;let state:String=db.query_row("SELECT state FROM gates WHERE id=?1",[gate],|r|r.get(0))?;anyhow::ensure!(state=="approved","Owner approval did not close the plan gate");anyhow::ensure!(db.query_row("SELECT count(*) FROM decisions",[],|r|r.get::<_,i64>(0))?==1,"Approval replay created another decision");anyhow::ensure!(db.query_row("SELECT count(*) FROM outbox WHERE kind='worker.resume'",[],|r|r.get::<_,i64>(0))?==1,"Missing or duplicate durable graph resume");Ok(())
 }.await;
    // Keep the issue/comments as evidence, close it on both success and failure.
    let cleanup = client
        .request(
            "PATCH",
            &format!("{path}/issues/{number}"),
            Some(json!({"state":"closed"})),
            None,
        )
        .await?;
    anyhow::ensure!(
        cleanup.status == 200,
        "Close disposable playground issue #{number} manually"
    );
    result
}
