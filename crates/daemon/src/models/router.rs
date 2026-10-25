//! Chain selection happens only at attempt boundaries, never during a run.
use crate::database;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path};
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Route {
    pub adapter: String,
    pub provider: String,
    pub model: String,
    #[serde(default)]
    pub budget: Option<RouteBudget>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteBudget {
    pub window: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Budget {
    pub window: String,
    #[serde(default)]
    pub requests: Option<u64>,
    #[serde(default)]
    pub reserve_for: Vec<String>,
    #[serde(default)]
    pub reserve_pct: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Diversity {
    pub different_family_than: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Chains {
    pub routes: BTreeMap<String, Route>,
    pub budgets: BTreeMap<String, Budget>,
    pub chains: BTreeMap<String, Vec<String>>,
    pub diversity: BTreeMap<String, Diversity>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Selected {
    pub name: String,
    pub route: Route,
    pub same_family_warning: bool,
}
pub fn config(home: &Path) -> anyhow::Result<Chains> {
    Ok(
        database::get(&database::open(home)?, "model_chains")?.unwrap_or(serde_yaml::from_str(
            include_str!("../../../../agents/chains.default.yaml"),
        )?),
    )
}
impl Chains {
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.routes.is_empty() && self.routes.len() <= 128,
            "Invalid route count"
        );
        for (name, route) in &self.routes {
            anyhow::ensure!(
                !name.is_empty() && !route.model.is_empty() && route.model.len() <= 200,
                "Invalid model route"
            );
            anyhow::ensure!(
                [
                    "openai",
                    "openrouter",
                    "ollama-cloud",
                    "opencode-zen",
                    "ollama"
                ]
                .contains(&route.provider.as_str()),
                "Unknown provider"
            );
            anyhow::ensure!(
                ["codex", "opencode"].contains(&route.adapter.as_str()),
                "Unknown adapter"
            );
            anyhow::ensure!(
                (route.adapter == "codex") == (route.provider == "openai"),
                "OpenAI subscription routes must use Codex"
            );
        }
        for (role, chain) in &self.chains {
            anyhow::ensure!(!chain.is_empty(), "Empty role chain");
            for name in chain {
                let route = self
                    .routes
                    .get(name)
                    .ok_or_else(|| anyhow::anyhow!("Unknown chain route"))?;
                anyhow::ensure!(
                    route.adapter != "codex"
                        || [
                            "cto",
                            "pm",
                            "backend",
                            "frontend",
                            "devops",
                            "qa",
                            "reviewer",
                            "docs",
                            "supervisor"
                        ]
                        .contains(&role.as_str()),
                    "Unknown Codex role chain"
                );
                // Every role can use the signed-in app-server adapter. The dispatcher
                // still enforces read-only versus coding capabilities per step.
            }
        }
        for budget in self.budgets.values() {
            anyhow::ensure!(
                ["daily", "provider", "hourly"].contains(&budget.window.as_str())
                    && budget.reserve_pct <= 100,
                "Invalid budget policy"
            );
        }
        Ok(())
    }
}
pub fn family(model: &str, provider: &str) -> String {
    if provider == "openai" {
        return "openai".into();
    }
    let lower = model.to_ascii_lowercase();
    for (fragment, family) in [
        ("minimax", "minimax"),
        ("glm", "zai"),
        ("z-ai", "zai"),
        ("poolside", "poolside"),
        ("laguna", "poolside"),
        ("gemma", "google"),
        ("google/", "google"),
        ("nemotron", "nvidia"),
        ("devstral", "mistral"),
        ("mistral", "mistral"),
        ("qwen", "qwen"),
    ] {
        if lower.contains(fragment) {
            return family.into();
        }
    }
    lower.split('/').next().unwrap_or("unknown").into()
}
pub fn allowed(db: &rusqlite::Connection, project: &str) -> anyhow::Result<Vec<String>> {
    let value:Option<String>=db.query_row("SELECT allowed_providers_json FROM project_settings WHERE project_id=?1 ORDER BY version DESC LIMIT 1",[project],|r|r.get(0)).optional()?;
    Ok(value
        .map(|s| serde_json::from_str(&s))
        .transpose()?
        .unwrap_or_else(|| {
            [
                "openai",
                "openrouter",
                "ollama-cloud",
                "opencode-zen",
                "ollama",
            ]
            .map(String::from)
            .to_vec()
        }))
}
fn window(budget: Option<&Budget>, now: i64) -> (&'static str, i64, i64) {
    match budget.map(|b| b.window.as_str()).unwrap_or("provider") {
        "daily" => ("daily", now / 86400 * 86400, (now / 86400 + 1) * 86400),
        "hourly" => ("hourly", now / 3600 * 3600, (now / 3600 + 1) * 3600),
        _ => ("provider", 0, now + 3600),
    }
}
pub fn select(
    home: &Path,
    project: &str,
    item: &str,
    role: &str,
    now: i64,
) -> anyhow::Result<Selected> {
    let cfg = config(home)?;
    cfg.validate()?;
    let db = database::open(home)?;
    let allowed = allowed(&db, project)?;
    let mut candidates = Vec::new();
    let mut excluded = Vec::new();
    for name in cfg
        .chains
        .get(role)
        .ok_or_else(|| anyhow::anyhow!("Unknown role chain"))?
    {
        let route = cfg.routes.get(name).unwrap();
        if !allowed.contains(&route.provider) {
            excluded.push(format!(
                "{name}: provider {} is disallowed by project policy",
                route.provider
            ));
            continue;
        }
        let health: Option<(bool, Option<String>)> = db
            .query_row(
                "SELECT available,model FROM route_health WHERE route=?1",
                [name],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let Some((true, resolved)) = health else {
            excluded.push(format!(
                "{name}: catalog availability is missing/failed; verify this route in Settings"
            ));
            continue;
        };
        let mut route = route.clone();
        if route.model == "auto" {
            let Some(model) = resolved else {
                excluded.push(format!("{name}: auto model unresolved; verify routes"));
                continue;
            };
            route.model = model;
        }
        let budget = cfg.budgets.get(&route.provider);
        let (kind, start, _) = window(budget, now);
        let exhausted: Option<i64> = db.query_row(
            "SELECT max(CAST(exhausted_until AS INTEGER)) FROM budgets WHERE route IN(?1,?2) AND CAST(exhausted_until AS INTEGER)>?3",
            params![name, route.provider, now],
            |r| r.get(0),
        )?;
        if let Some(reset) = exhausted {
            let when = chrono::DateTime::from_timestamp(reset, 0)
                .map(|d| d.to_rfc3339())
                .unwrap_or_else(|| reset.to_string());
            excluded.push(format!("{name}: cooling down until {when}"));
            continue;
        }
        let used:u64=db.query_row("SELECT coalesce(sum(used_requests),0) FROM budgets WHERE route=?1 AND window_kind=?2 AND window_start=?3",params![route.provider,kind,start],|r|r.get(0))?;
        if let Some(budget) = budget {
            if let Some(max) = budget.requests {
                let limit = if budget.reserve_for.contains(&role.to_string()) {
                    max
                } else {
                    max.saturating_mul(100 - budget.reserve_pct) / 100
                };
                if used >= limit {
                    excluded.push(format!("{name}: {} requests used; role allowance {limit} for {kind} window (reserves apply)",used));
                    continue;
                }
            }
        }
        let mut same = false;
        if let Some(diversity) = cfg.diversity.get(role) {
            for other in &diversity.different_family_than {
                let prior: Option<String> = db
                    .query_row(
                        "SELECT family FROM role_families WHERE item_id=?1 AND role=?2",
                        params![item, other],
                        |r| r.get(0),
                    )
                    .optional()?;
                same |= prior.as_deref() == Some(&family(&route.model, &route.provider));
            }
        }
        candidates.push(Selected {
            name: name.clone(),
            route,
            same_family_warning: same,
        });
    }
    candidates
        .iter()
        .find(|s| !s.same_family_warning)
        .cloned()
        .or_else(|| candidates.into_iter().next())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "No available, allowed route within budget for {role}. {}",
                excluded.join("; ")
            )
        })
}
/// Earliest automatic retry among permitted routes. Permanent access/policy errors need an owner.
pub fn retry_at(home: &Path, project: &str, role: &str, now: i64) -> anyhow::Result<Option<i64>> {
    let cfg = config(home)?;
    cfg.validate()?;
    let db = database::open(home)?;
    let permitted = allowed(&db, project)?;
    let mut times = vec![];
    for name in cfg
        .chains
        .get(role)
        .ok_or_else(|| anyhow::anyhow!("Unknown role chain"))?
    {
        let route = &cfg.routes[name];
        if !permitted.contains(&route.provider) {
            continue;
        }
        let health: Option<(bool, Option<String>)> = db
            .query_row(
                "SELECT available,error FROM route_health WHERE route=?1",
                [name],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if health
            .as_ref()
            .and_then(|h| h.1.as_ref())
            .is_some_and(|e| e.starts_with("Owner "))
        {
            continue;
        }
        if !health.as_ref().is_some_and(|h| h.0) {
            times.push(now + 300);
            continue;
        }
        let cooldown:Option<i64>=db.query_row("SELECT max(CAST(exhausted_until AS INTEGER)) FROM budgets WHERE route IN(?1,?2) AND CAST(exhausted_until AS INTEGER)>?3",params![name,route.provider,now],|r|r.get(0))?;
        let mut next = cooldown.unwrap_or(now + 30);
        if let Some(budget) = cfg.budgets.get(&route.provider) {
            let (kind, start, end) = window(Some(budget), now);
            let used:u64=db.query_row("SELECT coalesce(sum(used_requests),0) FROM budgets WHERE route=?1 AND window_kind=?2 AND window_start=?3",params![route.provider,kind,start],|r|r.get(0))?;
            if let Some(max) = budget.requests {
                let limit = if budget.reserve_for.contains(&role.to_string()) {
                    max
                } else {
                    max.saturating_mul(100 - budget.reserve_pct) / 100
                };
                if used >= limit {
                    if budget.window == "provider" {
                        continue;
                    }
                    next = next.max(end);
                }
            }
        }
        times.push(next);
    }
    Ok(times.into_iter().min())
}
/// Explicit diagnostic requests use only the unreserved provider allowance.
pub fn diagnostic_capacity(home: &Path, selected: &Selected) -> anyhow::Result<()> {
    let cfg = config(home)?;
    if let Some(budget) = cfg.budgets.get(&selected.route.provider) {
        if let Some(max) = budget.requests {
            let (kind, start, _) = window(Some(budget), crate::database::unix_now());
            let used:u64=database::open(home)?.query_row("SELECT coalesce(sum(used_requests),0) FROM budgets WHERE route=?1 AND window_kind=?2 AND window_start=?3",params![selected.route.provider,kind,start],|r|r.get(0))?;
            anyhow::ensure!(
                used < max.saturating_mul(100 - budget.reserve_pct) / 100,
                "Diagnostic budget exhausted; reserved requests remain for implementation/review"
            );
        }
    }
    Ok(())
}
pub fn count(
    home: &Path,
    selected: &Selected,
    requests: u64,
    tokens: u64,
    now: i64,
) -> anyhow::Result<()> {
    let cfg = config(home)?;
    let (kind, start, _) = window(cfg.budgets.get(&selected.route.provider), now);
    let db = database::open(home)?;
    db.execute("INSERT INTO budgets(route,window_kind,window_start,used_requests,used_tokens) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(route,window_kind,window_start) DO UPDATE SET used_requests=used_requests+excluded.used_requests,used_tokens=used_tokens+excluded.used_tokens",params![selected.route.provider,kind,start,requests,tokens])?;
    Ok(())
}
pub fn exhaust(
    home: &Path,
    selected: &Selected,
    reset: Option<i64>,
    now: i64,
) -> anyhow::Result<()> {
    let cfg = config(home)?;
    let (kind, start, end) = window(cfg.budgets.get(&selected.route.provider), now);
    let until = reset.filter(|r| *r > now).unwrap_or(end);
    let mut db = database::open(home)?;
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    tx.execute("INSERT INTO budgets(route,window_kind,window_start,exhausted_until) VALUES(?1,?2,?3,?4) ON CONFLICT(route,window_kind,window_start) DO UPDATE SET exhausted_until=max(coalesce(budgets.exhausted_until,0),excluded.exhausted_until)",params![selected.name,kind,start,until])?;
    database::audit(
        &tx,
        "daemon",
        "route.exhausted",
        &selected.name,
        &serde_json::json!({"until":until}),
    )?;
    tx.commit()?;
    Ok(())
}
pub fn quota(status: u16, body: &str) -> bool {
    if status == 429 {
        return true;
    }
    if body.lines().any(|line| line.starts_with("data:")) {
        return body
            .lines()
            .filter_map(|line| line.strip_prefix("data:"))
            .any(|chunk| quota(status, chunk.trim()));
    }
    let value = serde_json::from_str::<serde_json::Value>(body).ok();
    let text = if let Some(value) = value {
        if !value["error"].is_null() {
            value["error"].to_string()
        } else if value["type"] == "error" || status >= 400 {
            body.to_string()
        } else {
            return false;
        }
    } else {
        body.to_string()
    };
    let lower = text.to_ascii_lowercase();
    [
        "quota",
        "rate_limit",
        "usage limit",
        "usage_limit",
        "insufficient_quota",
        "limit reached",
    ]
    .iter()
    .any(|s| lower.contains(s))
}

pub fn provider_failed(home: &Path, selected: &Selected, now: i64) -> anyhow::Result<bool> {
    let db = database::open(home)?;
    db.execute("INSERT INTO route_failures VALUES(?1,1,?2) ON CONFLICT(route) DO UPDATE SET failures=CASE WHEN last_failure<?2-3600 THEN 1 ELSE failures+1 END,last_failure=?2",params![selected.name,now])?;
    let count: u64 = db.query_row(
        "SELECT failures FROM route_failures WHERE route=?1",
        [&selected.name],
        |r| r.get(0),
    )?;
    if count >= 2 {
        exhaust(home, selected, None, now)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

#[cfg(test)]
mod fallback_tests {
    use super::*;
    #[test]
    fn cooldown_moves_down_pm_chain_and_recovers_without_bypassing_policy() {
        let t = tempfile::tempdir().unwrap();
        let home = t.path();
        let db = database::open(home).unwrap();
        db.execute(
            "INSERT INTO projects VALUES('p','o/r',NULL,'/checkout','eos','private','now')",
            [],
        )
        .unwrap();
        let cfg = config(home).unwrap();
        for name in &cfg.chains["pm"] {
            db.execute("INSERT INTO route_health(route,available,model,verified_at) VALUES(?1,1,'resolved-model',0)",[name]).unwrap();
        }
        let first = select(home, "p", "i", "pm", 100).unwrap();
        assert_eq!(first.name, cfg.chains["pm"][0]);
        exhaust(home, &first, Some(3700), 100).unwrap();
        let second = select(home, "p", "i", "pm", 101).unwrap();
        assert_eq!(second.name, cfg.chains["pm"][1]);
        assert_eq!(select(home, "p", "i", "pm", 3701).unwrap().name, first.name);
        db.execute("INSERT INTO project_settings(project_id,version,allowed_providers_json,effective_at,set_by) VALUES('p',1,'[\"ollama\"]','now','owner')",[]).unwrap();
        assert_eq!(
            select(home, "p", "i", "pm", 3701).unwrap().route.provider,
            "ollama"
        );
        db.execute("UPDATE route_health SET available=0", [])
            .unwrap();
        assert!(select(home, "p", "i", "pm", 3701).is_err());
    }
}
