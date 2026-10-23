//! Strict, whole-comment command grammar. Natural language is never authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    ApprovePlan(String), RequestChanges(String), ApproveSprint, ApproveItem(String), Continue, Urgent,
}
pub fn parse(body: &str) -> Option<Command> {
    if body.contains(['\n','\r']) || body.trim()!=body { return None; }
    if body=="/plantool approve-sprint" {return Some(Command::ApproveSprint)}
    if body=="/plantool continue" {return Some(Command::Continue)}
    if body=="/plantool urgent" {return Some(Command::Urgent)}
    if let Some(hash)=body.strip_prefix("/plantool approve-plan sha256:") {
        if (12..=64).contains(&hash.len()) && hash.bytes().all(|b|b.is_ascii_hexdigit()) {return Some(Command::ApprovePlan(hash.to_ascii_lowercase()))}
        return None;
    }
    if let Some(reason)=body.strip_prefix("/plantool request-changes ") {
        if !reason.trim().is_empty() {return Some(Command::RequestChanges(reason.into()))}
    }
    if let Some(item)=body.strip_prefix("/plantool approve ") {
        if !item.is_empty() && !item.chars().any(char::is_whitespace) {return Some(Command::ApproveItem(item.into()))}
    }
    None
}
#[cfg(test)] mod tests {
 use super::*;
 #[test] fn grammar_is_exact(){
  assert_eq!(parse("/plantool approve-plan sha256:abcdef012345"),Some(Command::ApprovePlan("abcdef012345".into())));
  for s in ["LGTM","approved","👍","/Plantool urgent"," /plantool urgent","/plantool urgent\n","/plantool urgent\n/plantool continue","/plantool approve-plan sha256:abc","/plantool approve-plan sha256:zzzzzzzzzzzz"] {assert_eq!(parse(s),None,"{s}")}
 }
}
