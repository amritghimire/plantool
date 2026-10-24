//! The only wire contract between the authoritative daemon and the worker.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
pub const CONTRACT_VERSION: u32 = 2;
pub const GRAPH_VERSION: u32 = 1;
macro_rules! wire {
    ($name:ident {$($field:ident: $ty:ty),* $(,)?}) => {
        #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
        #[serde(deny_unknown_fields)]
        pub struct $name {$(pub $field: $ty),*}
    }
}
wire!(Health {
    contract_version: u32,
    graph_version: u32
});
impl Default for Health {
    fn default() -> Self {
        Self {
            contract_version: CONTRACT_VERSION,
            graph_version: GRAPH_VERSION,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Graph {
    Project,
    WorkItem,
}
wire!(Start {
    thread_id: String,
    graph: Graph,
    input: Value,
    idempotency_key: String,
    contract_version: u32,
    graph_version: u32
});
wire!(Resume {
    thread_id: String,
    interrupt_id: String,
    value: Value,
    idempotency_key: String,
    contract_version: u32,
    graph_version: u32
});
wire!(Cancel {
    thread_id: String,
    idempotency_key: String
});
wire!(Status {thread_id:String, state:String, interrupt_id:Option<String>, graph_version:u32, result:Option<Value>});
wire!(Dispatch {
    item: String,
    role: String,
    generation: u64,
    inputs: Value,
    plan_sha: String,
    policy_version: u64,
    capability: String,
    idempotency_key: String,
    thread_id: String,
    interrupt_id: String
});
wire!(DispatchReply {
    attempt_id: String,
    generation: u64
});
wire!(Gate {item:String, kind:String, target_digest:String, target_sha:Option<String>, idempotency_key:String, thread_id:Option<String>, interrupt_id:Option<String>, summary:Option<String>, discussion_thread_id:Option<String>});
wire!(ReadArtifact {
    item: String,
    sha256: String
});
wire!(ReadTask {
    item: String,
    key: String
});
wire!(ReadRepoFile {
    item: String,
    path: String
});
wire!(PostMessage {
    item: String,
    thread_id: String,
    body: String,
    idempotency_key: String
});
wire!(FileProposal {
    item: String,
    title: String,
    body: String,
    idempotency_key: String
});
wire!(EventAck {
    project: String,
    event_id: String,
    idempotency_key: String
});
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Artifact {
    pub kind: String,
    #[serde(rename = "ref")]
    pub reference: String,
}
macro_rules! wire_enum {($name:ident {$($variant:ident=>$text:literal),*})=>{
 #[derive(Debug,Clone,Serialize,Deserialize,JsonSchema,PartialEq,Eq)]
 pub enum $name {$(#[serde(rename=$text)] $variant),*}
 impl $name {pub fn as_str(&self)->&'static str{match self{$(Self::$variant=>$text),*}}}
};}
wire_enum!(CheckStatus {Pass=>"pass",Fail=>"fail",Skipped=>"skipped"});
wire_enum!(Severity {High=>"high",Medium=>"medium",Low=>"low"});
wire_enum!(FindingScope {In=>"in",Out=>"out"});
wire!(Check {
    name: String,
    status: CheckStatus,
    evidence: String
});
wire!(Finding {severity:Severity, scope:FindingScope, title:String, detail:String, file:Option<String>, line:Option<u32>});
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Done,
    NeedsFix,
    Blocked,
    Failed,
}
wire!(ReviewGuide {
    same_family_warning: bool
});
wire!(RoleResult {item:String, role:String, attempt_id:String, generation:u64, outcome:Outcome, summary:String, tasks_done:Vec<String>, artifacts:Vec<Artifact>, checks:Vec<Check>, findings:Vec<Finding>, next:Option<String>, review_guide:Option<ReviewGuide>});
wire!(LlmContext {item:String, role:String, generation:u64, attempt_id:Option<String>, idempotency_key:String});
/// Schema roots include every request and response, not just the graph entrypoints.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", content = "body", rename_all = "snake_case")]
pub enum Contract {
    Health(Health),
    Start(Start),
    Resume(Resume),
    Cancel(Cancel),
    Status(Status),
    RequestDispatch(Dispatch),
    DispatchReply(DispatchReply),
    RequestGate(Gate),
    ReadArtifact(ReadArtifact),
    ReadTask(ReadTask),
    ReadRepoFile(ReadRepoFile),
    PostMessage(PostMessage),
    ReportResult(RoleResult),
    FileProposal(FileProposal),
    ProjectEventAck(EventAck),
    Llm(LlmContext),
}
pub fn schema() -> String {
    format!(
        "{}\n",
        serde_json::to_string_pretty(&schemars::schema_for!(Contract)).unwrap()
    )
}
pub fn result_schema() -> String {
    format!(
        "{}\n",
        serde_json::to_string_pretty(&schemars::schema_for!(RoleResult)).unwrap()
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn checked_in_contract_matches_rust() {
        assert_eq!(schema(), include_str!("../../../contract/schema.json"));
    }
    #[test]
    fn unknown_result_fields_are_rejected() {
        assert!(
            serde_json::from_value::<RoleResult>(serde_json::json!({"approval":true})).is_err()
        );
    }
}
