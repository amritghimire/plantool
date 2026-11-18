#![cfg(unix)]

use plantool_core::{PermissionMode, Provider};
use plantool_daemon::providers::{self, ProviderEvent, RunInput, RunOptions};
use std::os::unix::fs::PermissionsExt;
use tokio::sync::mpsc;

const FAKE_T3: &str = r#"#!/usr/bin/env python3
import sys,json,os
os.chdir(os.path.dirname(os.path.abspath(sys.argv[0])))
state='runs'
runs=int(open(state).read()) if os.path.exists(state) else 0
cmd=sys.argv[2]
with open('calls','a') as f: f.write(' '.join(sys.argv[2:])+'\n')
if cmd in ('start','send'):
 msg=sys.stdin.read()
 open(state,'w').write(str(runs+1))
 open('last','w').write(msg)
 if cmd=='start': print('th1')
 if 'Stop plantool run' in sys.argv: print('stopped')
elif cmd=='show':
 last=open('last').read().strip() if os.path.exists('last') else ''
 busy=os.path.exists('busy')
 print(json.dumps({'thread':{'status':'running' if busy else 'completed','link':'http://t3/th1','activeRunId':'r' if busy else None,'runCount':runs,'latestRunId':'r%d'%runs,'pendingRequestCount':0},'messages':[{'role':'assistant','text':'old','runId':'r%d'%(runs-1)},{'role':'user','text':last,'runId':'r%d'%runs},{'role':'assistant','text':'reading','runId':'r%d'%runs},{'role':'assistant','text':'done: '+last,'runId':'r%d'%runs}]}))
"#;

async fn turn(rx: &mut mpsc::Receiver<ProviderEvent>) -> Vec<String> {
    tokio::time::timeout(std::time::Duration::from_secs(20), async {
        let mut said = Vec::new();
        while let Some(event) = rx.recv().await {
            match event {
                ProviderEvent::Message { role, content, .. } if role == "assistant" => {
                    said.push(content)
                }
                ProviderEvent::TurnCompleted { .. } => return said,
                _ => {}
            }
        }
        panic!("provider stopped before replying");
    })
    .await
    .expect("provider reply timed out")
}

fn options(dir: &std::path::Path, resume: Option<&str>) -> RunOptions {
    let script = dir.join("t3");
    std::fs::write(&script, FAKE_T3).unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    RunOptions {
        cwd: dir.to_path_buf(),
        prompt: "build it".into(),
        model: Some("codex:gpt-6.1-sol".into()),
        effort: None,
        resume: resume.map(str::to_string),
        executable: Some(script),
        writable_roots: Vec::new(),
        permission_mode: PermissionMode::Ask,
    }
}

#[tokio::test]
async fn starts_a_thread_then_sends_follow_ups_to_it() {
    let dir = tempfile::tempdir().unwrap();
    let (input, rx_input) = mpsc::channel(8);
    let (sink, mut events) = mpsc::channel(64);
    let task = tokio::spawn(providers::run_provider(
        Provider::T3,
        options(dir.path(), None),
        rx_input,
        sink,
    ));
    assert_eq!(turn(&mut events).await, ["reading", "done: build it"]);
    input
        .send(RunInput::Text("fix the comment".into()))
        .await
        .unwrap();
    assert_eq!(
        turn(&mut events).await,
        ["reading", "done: fix the comment"]
    );
    input.send(RunInput::Stop).await.unwrap();
    task.await.unwrap().unwrap();
    let calls = std::fs::read_to_string(dir.path().join("calls")).unwrap();
    assert!(calls.contains("start "), "{calls}");
    assert!(
        calls.contains("--provider codex --model gpt-6.1-sol"),
        "{calls}"
    );
    assert!(calls.contains("send th1 -"), "{calls}");
}

#[tokio::test]
async fn resumed_run_messages_the_same_thread() {
    let dir = tempfile::tempdir().unwrap();
    let (input, rx_input) = mpsc::channel(8);
    let (sink, mut events) = mpsc::channel(64);
    let task = tokio::spawn(providers::run_provider(
        Provider::T3,
        options(dir.path(), Some("th1")),
        rx_input,
        sink,
    ));
    assert_eq!(turn(&mut events).await, ["reading", "done: build it"]);
    input.send(RunInput::Stop).await.unwrap();
    task.await.unwrap().unwrap();
    let calls = std::fs::read_to_string(dir.path().join("calls")).unwrap();
    assert!(!calls.contains("start "), "{calls}");
    assert!(calls.contains("send th1 -"), "{calls}");
}

#[tokio::test]
async fn stop_during_a_turn_asks_a_helper_thread_to_interrupt_it() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("busy"), "").unwrap();
    let (input, rx_input) = mpsc::channel(8);
    let (sink, mut events) = mpsc::channel(64);
    let task = tokio::spawn(providers::run_provider(
        Provider::T3,
        options(dir.path(), Some("th1")),
        rx_input,
        sink,
    ));
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        while let Some(event) = events.recv().await {
            if matches!(event, ProviderEvent::Status { .. }) {
                return;
            }
        }
    })
    .await
    .unwrap();
    input.send(RunInput::Stop).await.unwrap();
    task.await.unwrap().unwrap();
    let calls = std::fs::read_to_string(dir.path().join("calls")).unwrap();
    assert!(
        calls.contains("--title Stop plantool run --wait"),
        "{calls}"
    );
    let last = std::fs::read_to_string(dir.path().join("last")).unwrap();
    assert!(last.contains(r#"threadId "th1" and stop true"#), "{last}");
}
