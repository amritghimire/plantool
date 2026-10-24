//! Additive presets and phase-scoped progress. Owner gates are daemon facts.
use anyhow::{ensure, Context};
use plantool_core::presets::Preset;
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde_json::{json, Value};
pub fn preset(db: &Connection, project: &str) -> anyhow::Result<Preset> {
    let value:Option<String>=db.query_row("SELECT oversight FROM project_settings WHERE project_id=?1 ORDER BY version DESC LIMIT 1",[project],|r|r.get(0)).optional()?;
    Ok(serde_json::from_value(json!(
        value.unwrap_or_else(|| "minimal".into())
    ))?)
}
pub fn rules(db: &Connection, project: &str) -> anyhow::Result<Vec<String>> {
    let raw:String=db.query_row("SELECT review_globs_json FROM project_settings WHERE project_id=?1 ORDER BY version DESC LIMIT 1",[project],|r|r.get(0)).optional()?.unwrap_or_else(||"[]".into());
    Ok(serde_json::from_str(&raw)?)
}
pub fn suggested(paths: &[String], rules: &[String]) -> anyhow::Result<bool> {
    for rule in rules {
        let glob = globset::GlobBuilder::new(rule)
            .case_insensitive(true)
            .literal_separator(true)
            .build()?
            .compile_matcher();
        if paths.iter().any(|p| glob.is_match(p)) {
            return Ok(true);
        }
    }
    Ok(false)
}
pub fn mark_plan(
    markdown: &str,
    preset: Preset,
    paths: &[String],
    rules: &[String],
) -> anyhow::Result<String> {
    if preset != Preset::Standard || !suggested(paths, rules)? {
        return Ok(markdown.into());
    }
    let mut content = String::new();
    let phases = markdown
        .lines()
        .any(|l| l.starts_with("## ") || l.starts_with("### "));
    for line in markdown.lines() {
        let tag = if phases {
            (line.starts_with("## ") || line.starts_with("### ")) && !line.contains("[review]")
        } else {
            line.contains("- [ ] T") && !line.contains("[review]")
        };
        content.push_str(line);
        if tag {
            content.push_str(" [review]")
        }
        content.push('\n');
    }
    Ok(content)
}
pub fn normalize_phases(markdown: &str) -> String {
    markdown
        .lines()
        .map(|line| {
            if line.starts_with("## ") {
                format!("#{line}\n")
            } else {
                format!("{line}\n")
            }
        })
        .collect()
}
pub fn phases(markdown: &str) -> anyhow::Result<Value> {
    let normalized = normalize_phases(markdown);
    let tasks = plantool_core::tasks::parse(&normalized).map_err(anyhow::Error::msg)?;
    let mut phases: Vec<Value> = vec![];
    for t in tasks {
        let phase = t.phase.unwrap_or_else(|| "Tasks".into());
        let index = phases
            .iter()
            .position(|p| p["name"] == phase)
            .unwrap_or_else(|| {
                phases.push(
                    json!({"name":phase,"keys":[],"areas":[],"tagged":phase.contains("[review]")}),
                );
                phases.len() - 1
            });
        phases[index]["keys"]
            .as_array_mut()
            .unwrap()
            .push(json!(t.key));
        if let Some(area) = t.area {
            if !phases[index]["areas"]
                .as_array()
                .unwrap()
                .contains(&json!(area))
            {
                phases[index]["areas"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!(area));
            }
        }
        if t.review_tag {
            phases[index]["tagged"] = json!(true)
        }
    }
    Ok(json!(phases))
}
pub fn first_step(areas: &Value) -> &'static str {
    if areas
        .as_array()
        .is_some_and(|v| v.contains(&json!("backend")))
    {
        "backend"
    } else if areas
        .as_array()
        .is_some_and(|v| v.contains(&json!("frontend")))
    {
        "frontend"
    } else if areas
        .as_array()
        .is_some_and(|v| v.contains(&json!("devops")))
    {
        "devops"
    } else {
        "qa"
    }
}
pub fn initialize(s: &mut Value) {
    if s["phases"].as_array().is_some_and(|p| !p.is_empty()) {
        s["phase_index"] = json!(0);
        s["areas"] = s["phases"][0]["areas"].clone();
    }
}
pub fn active_key(s: &Value, key: &str) -> bool {
    let Some(phases) = s["phases"].as_array().filter(|p| !p.is_empty()) else {
        return true;
    };
    phases
        .get(s["phase_index"].as_u64().unwrap_or(0) as usize)
        .is_some_and(|p| {
            p["keys"]
                .as_array()
                .is_some_and(|keys| keys.contains(&json!(key)))
        })
}
pub fn complete(db: &Connection, item: &str, s: &Value) -> anyhow::Result<bool> {
    let Some(phases) = s["phases"].as_array().filter(|p| !p.is_empty()) else {
        return Ok(true);
    };
    let phase = &phases[s["phase_index"].as_u64().unwrap_or(0) as usize];
    for key in phase["keys"].as_array().context("Phase keys missing")? {
        let done:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM task_progress WHERE item_id=?1 AND task_key=?2 AND status='done')",params![item,key.as_str()],|r|r.get(0))?;
        if !done {
            return Ok(false);
        }
    }
    Ok(true)
}
pub fn stop_required(s: &Value) -> anyhow::Result<bool> {
    let preset: Preset =
        serde_json::from_value(s.get("preset").cloned().unwrap_or(json!("minimal")))?;
    let index = s["phase_index"].as_u64().unwrap_or(0) as usize;
    Ok(s["phases"]
        .as_array()
        .and_then(|p| p.get(index))
        .is_some_and(|p| preset.milestone(p["tagged"] == true)))
}
pub fn advance(s: &mut Value) {
    let next = s["phase_index"].as_u64().unwrap_or(0) as usize + 1;
    if s["phases"].as_array().is_some_and(|p| next < p.len()) {
        s["phase_index"] = json!(next);
        s["areas"] = s["phases"][next]["areas"].clone();
        s["step"] = json!(first_step(&s["areas"]));
    } else {
        s["step"] = json!("pr")
    }
}
pub fn boundary(tx: &Transaction<'_>, item: &str, s: &mut Value) -> anyhow::Result<()> {
    if !s["phases"].as_array().is_some_and(|p| !p.is_empty()) {
        return Ok(());
    }
    ensure!(complete(tx, item, s)?, "Current phase tasks are incomplete");
    if stop_required(s)? {
        s["step"] = json!("milestone")
    } else {
        advance(s)
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn risk_rules_suggest_tags_without_changing_minimal_or_strict_text() {
        let plan = "# Plan\n## Build\n- [ ] T1: [backend] Validate\n";
        let paths = vec!["crates/daemon/migrations/006.sql".into()];
        let rules = vec!["**/migrations/**".into()];
        assert!(suggested(&paths, &rules).unwrap());
        assert!(mark_plan(plan, Preset::Standard, &paths, &rules)
            .unwrap()
            .contains("## Build [review]"));
        assert_eq!(
            mark_plan(plan, Preset::Minimal, &paths, &rules).unwrap(),
            plan
        );
        assert_eq!(
            mark_plan(plan, Preset::Strict, &paths, &rules).unwrap(),
            plan
        );
        let phases = phases(&mark_plan(plan, Preset::Standard, &paths, &rules).unwrap()).unwrap();
        assert_eq!(phases[0]["keys"], json!(["T1"]));
        assert_eq!(phases[0]["tagged"], true);
    }
    #[test]
    fn phases_keep_keys_and_never_allow_future_progress() {
        let p = phases(
            "## First\n- [ ] T1: [backend] One\n## Second [review]\n- [ ] T2: [frontend] Two\n",
        )
        .unwrap();
        let mut s = json!({"phases":p,"preset":"strict"});
        initialize(&mut s);
        assert!(active_key(&s, "T1"));
        assert!(!active_key(&s, "T2"));
        assert!(stop_required(&s).unwrap());
        advance(&mut s);
        assert!(active_key(&s, "T2"));
        assert_eq!(s["step"], "frontend");
    }
}

pub fn valid_gates(db: &Connection, item: &str, s: &Value, head: &str) -> anyhow::Result<bool> {
    if s["preset"] == "strict" {
        let research:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM gates g JOIN work_items i ON i.id=g.item_id WHERE g.item_id=?1 AND g.kind='research' AND g.state='approved' AND g.target_digest=?2 AND g.policy_version=(SELECT max(version) FROM project_settings WHERE project_id=i.project_id))",params![item,s["research_sha"].as_str().unwrap_or("")],|r|r.get(0))?;
        if !research {
            return Ok(false);
        }
    }
    let preset: Preset =
        serde_json::from_value(s.get("preset").cloned().unwrap_or(json!("minimal")))?;
    for (index, phase) in s["phases"].as_array().unwrap_or(&vec![]).iter().enumerate() {
        if !preset.milestone(phase["tagged"] == true) {
            continue;
        }
        let approved:Option<String>=db.query_row("SELECT g.target_sha FROM gates g JOIN milestone_context c ON c.gate_id=g.id JOIN work_items i ON i.id=g.item_id WHERE g.item_id=?1 AND c.phase_index=?2 AND g.state='approved' AND g.target_digest=?3 AND g.policy_version=(SELECT max(version) FROM project_settings WHERE project_id=i.project_id) ORDER BY g.created_at DESC LIMIT 1",params![item,index,s["plan_sha"].as_str()],|r|r.get(0)).optional()?;
        let Some(sha) = approved else {
            return Ok(false);
        };
        let root = crate::dispatcher::item(db, item)?.1;
        if crate::git::git(&root, &["merge-base", "--is-ancestor", &sha, head]).is_err() {
            return Ok(false);
        }
    }
    Ok(true)
}
