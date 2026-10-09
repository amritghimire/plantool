use crate::client::{print_json, Client};
use clap::Args as ClapArgs;

#[derive(ClapArgs)]
pub struct Args {
    /// Filter: a stage name, "stale", or a substring of the slug.
    pub filter: Option<String>,
    #[arg(long)]
    pub json: bool,
}

pub fn run(a: Args) -> anyhow::Result<()> {
    let c = Client::connect()?;
    let sessions: Vec<serde_json::Value> = c.get("/api/sessions")?;
    let now = chrono_now();
    let mut rows = Vec::new();
    for s in sessions {
        let key = s
            .get("key")
            .and_then(|k| k.as_str())
            .unwrap_or("?")
            .to_string();
        let stage = s
            .pointer("/state/stage")
            .and_then(|k| k.as_str())
            .unwrap_or("?")
            .to_string();
        let updated = s
            .pointer("/state/updated_at")
            .and_then(|k| k.as_str())
            .unwrap_or("")
            .to_string();
        let (done, total) = s
            .get("docs")
            .and_then(|d| d.as_array())
            .map(|docs| {
                docs.iter()
                    .filter(|d| d.get("kind").and_then(|k| k.as_str()) == Some("plan"))
                    .flat_map(|d| {
                        d.get("progress")
                            .and_then(|p| p.as_array())
                            .cloned()
                            .unwrap_or_default()
                    })
                    .fold((0u64, 0u64), |(d, t), p| {
                        (
                            d + p.get("done").and_then(|x| x.as_u64()).unwrap_or(0),
                            t + p.get("total").and_then(|x| x.as_u64()).unwrap_or(0),
                        )
                    })
            })
            .unwrap_or((0, 0));
        let age_days = age_in_days(&updated, &now);
        let stale = age_days.map(|d| d >= 7.0).unwrap_or(false) && stage != "done";
        let keep = match a.filter.as_deref() {
            None => true,
            Some("stale") => stale,
            Some(f) => stage == f || key.contains(f),
        };
        if keep {
            rows.push(serde_json::json!({ "key": key, "stage": stage, "done": done, "total": total, "updated_at": updated, "stale": stale }));
        }
    }
    if a.json {
        return print_json(&rows);
    }
    if rows.is_empty() {
        println!("nothing matches");
        return Ok(());
    }
    for r in rows {
        let pct = if r["total"].as_u64().unwrap_or(0) > 0 {
            format!("{}/{}", r["done"], r["total"])
        } else {
            "-".into()
        };
        println!(
            "{:40} {:22} {:>8} {}{}",
            r["key"].as_str().unwrap_or(""),
            r["stage"].as_str().unwrap_or(""),
            pct,
            r["updated_at"]
                .as_str()
                .unwrap_or("")
                .chars()
                .take(10)
                .collect::<String>(),
            if r["stale"].as_bool().unwrap_or(false) {
                "  stale"
            } else {
                ""
            }
        );
    }
    Ok(())
}

fn chrono_now() -> String {
    plantool_core::now()
}

fn age_in_days(updated: &str, now: &str) -> Option<f64> {
    let parse = |s: &str| chrono::DateTime::parse_from_rfc3339(s).ok();
    let (u, n) = (parse(updated)?, parse(now)?);
    Some((n - u).num_seconds() as f64 / 86400.0)
}
