#![cfg(unix)]

use plantool_core::Provider;
use plantool_daemon::providers::{self, ProviderEvent, RunInput, RunOptions};
use serde_json::Value;
use std::os::unix::fs::PermissionsExt;
use tokio::sync::mpsc;

async fn next_event(
    rx: &mut mpsc::Receiver<ProviderEvent>,
    predicate: impl Fn(&ProviderEvent) -> bool,
) {
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        while let Some(event) = rx.recv().await {
            if predicate(&event) {
                return;
            }
        }
        panic!("provider stopped before expected event");
    })
    .await
    .expect("provider event timed out");
}

async fn native_turns(provider: Provider, reject_steer: bool) {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("attachments")).unwrap();
    let image = root.path().join("attachments/image.png");
    std::fs::write(&image, b"\x89PNG\r\n\x1a\nfixture").unwrap();
    let prompt = format!("Attached file: {}", image.display());
    let script = root.path().join("provider");
    let source = match provider {
        Provider::Codex => {
            r#"#!/usr/bin/env python3
import sys,json
turn=0
for line in sys.stdin:
 v=json.loads(line); method=v.get('method'); rid=v.get('id')
 if method in ['turn/start','turn/steer']:
  with open('payloads','a') as f: f.write(json.dumps(v)+'\n')
 if method=='initialize': result={}
 elif method=='thread/start': result={'thread':{'id':'thread'}}
 elif method=='turn/start':
  turn+=1; result={'turn':{'id':'turn'+str(turn)}}
 elif method=='turn/steer': result={}
 else: continue
 print(json.dumps({'id':rid,'result':result}),flush=True)
 if method=='turn/steer' or (method=='turn/start' and turn>1):
  print(json.dumps({'method':'turn/completed','params':{'threadId':'thread','turn':{'id':'turn'+str(turn),'status':'completed'}}}),flush=True)
"#
        }
        Provider::Claude => {
            r#"#!/usr/bin/env python3
import sys,json
count=0
for line in sys.stdin:
 v=json.loads(line)
 if v.get('type')!='user': continue
 count+=1
 with open('payloads','a') as f: f.write(json.dumps(v)+'\n')
 if count==1: print(json.dumps({'type':'system','subtype':'init','session_id':'thread'}),flush=True)
 if count>=2: print(json.dumps({'type':'result','subtype':'success'}),flush=True)
"#
        }
        _ => unreachable!(),
    };
    let source = if reject_steer {
        source.replace("elif method=='turn/steer': result={}", "elif method=='turn/steer':\n  print(json.dumps({'id':rid,'error':{'message':'Model does not support images'}}),flush=True)\n  continue")
    } else {
        source.to_string()
    };
    std::fs::write(&script, source).unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let opts = RunOptions {
        cwd: root.path().to_path_buf(),
        prompt: prompt.clone(),
        model: Some("fixture-model".into()),
        effort: None,
        resume: None,
        executable: Some(script),
        writable_roots: vec![root.path().to_path_buf()],
        permission_mode: Default::default(),
    };
    let (tx, input) = mpsc::channel(16);
    let (sink, mut events) = mpsc::channel(64);
    let handle = tokio::spawn(providers::run_provider(provider, opts, input, sink));
    next_event(&mut events, |e| {
        matches!(e, ProviderEvent::TurnStarted { .. })
    })
    .await;
    tx.send(RunInput::Text(format!("Active followup\n{prompt}")))
        .await
        .unwrap();
    if reject_steer {
        next_event(&mut events, |e| matches!(e, ProviderEvent::Status { label, detail } if label == "Follow-up rejected" && detail.as_deref() == Some("Model does not support images"))).await;
        tx.send(RunInput::Stop).await.unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), handle)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        return;
    }
    next_event(&mut events, |e| {
        matches!(e, ProviderEvent::TurnCompleted { .. })
    })
    .await;
    tx.send(RunInput::Text(format!("Idle followup\n{prompt}")))
        .await
        .unwrap();
    next_event(&mut events, |e| {
        matches!(e, ProviderEvent::TurnCompleted { .. })
    })
    .await;
    tx.send(RunInput::Stop).await.unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), handle)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let records: Vec<Value> = std::fs::read_to_string(root.path().join("payloads"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(records.len(), 3);
    if provider == Provider::Codex {
        assert_eq!(
            records
                .iter()
                .map(|v| v["method"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["turn/start", "turn/steer", "turn/start"]
        );
        for record in records {
            assert_eq!(record["params"]["input"][1]["type"], "localImage");
            assert_eq!(
                record["params"]["input"][1]["path"],
                image.canonicalize().unwrap().to_str().unwrap()
            );
        }
    } else {
        for record in records {
            assert_eq!(
                record["message"]["content"][1]["source"]["media_type"],
                "image/png"
            );
            assert!(
                record["message"]["content"][1]["source"]["data"]
                    .as_str()
                    .unwrap()
                    .len()
                    > 8
            );
        }
    }
}

#[tokio::test]
async fn codex_start_steer_and_idle_deliver_native_images() {
    native_turns(Provider::Codex, false).await;
}
#[tokio::test]
async fn claude_initial_active_and_idle_deliver_native_images() {
    native_turns(Provider::Claude, false).await;
}

async fn file_flag_turns(provider: Provider, flag: &str) {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("attachments")).unwrap();
    let image = root.path().join("attachments/image.png");
    std::fs::write(&image, "fixture").unwrap();
    let prompt = format!("Attached file: {}", image.display());
    let script = root.path().join("provider");
    std::fs::write(
        &script,
        r#"#!/usr/bin/env python3
import sys,json,time
with open('payloads','a') as f: f.write(json.dumps(sys.argv[1:])+'\n')
time.sleep(0.3)
print('{}',flush=True)
"#,
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let opts = RunOptions {
        cwd: root.path().to_path_buf(),
        prompt: prompt.clone(),
        model: Some("fixture-model".into()),
        effort: None,
        resume: None,
        executable: Some(script),
        writable_roots: vec![root.path().to_path_buf()],
        permission_mode: Default::default(),
    };
    let (tx, input) = mpsc::channel(16);
    let (sink, mut events) = mpsc::channel(64);
    let handle = tokio::spawn(providers::run_provider(provider, opts, input, sink));
    next_event(&mut events, |e| {
        matches!(e, ProviderEvent::TurnStarted { .. })
    })
    .await;
    tx.send(RunInput::Text(format!("Queued\n{prompt}")))
        .await
        .unwrap();
    next_event(&mut events, |e| {
        matches!(e, ProviderEvent::TurnCompleted { .. })
    })
    .await;
    next_event(&mut events, |e| {
        matches!(e, ProviderEvent::TurnCompleted { .. })
    })
    .await;
    tx.send(RunInput::Text(format!("Idle\n{prompt}")))
        .await
        .unwrap();
    next_event(&mut events, |e| {
        matches!(e, ProviderEvent::TurnCompleted { .. })
    })
    .await;
    tx.send(RunInput::Stop).await.unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), handle)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let records: Vec<Vec<String>> = std::fs::read_to_string(root.path().join("payloads"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(records.len(), 3);
    for args in records {
        let index = args
            .iter()
            .position(|arg| arg == flag)
            .expect("file flag missing");
        assert_eq!(
            args[index + 1],
            image.canonicalize().unwrap().to_str().unwrap()
        );
    }
}
#[tokio::test]
async fn copilot_initial_queued_and_idle_keep_attachment_flags() {
    file_flag_turns(Provider::Copilot, "--attachment").await;
}
#[tokio::test]
async fn opencode_initial_queued_and_idle_keep_file_flags() {
    file_flag_turns(Provider::Ollama, "--file").await;
}

#[tokio::test]
async fn codex_reports_native_image_followup_rejection() {
    native_turns(Provider::Codex, true).await;
}
