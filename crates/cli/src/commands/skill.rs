use clap::Args as ClapArgs;
use std::path::PathBuf;

pub struct Shipped {
    pub name: &'static str,
    pub content: &'static str,
}

pub const SKILLS: [Shipped; 25] = [
    Shipped {
        name: "plantool",
        content: include_str!("../../../../agents/skills/plantool/SKILL.md"),
    },
    Shipped {
        name: "research",
        content: include_str!("../../../../agents/skills/research/SKILL.md"),
    },
    Shipped {
        name: "plan",
        content: include_str!("../../../../agents/skills/plan/SKILL.md"),
    },
    Shipped {
        name: "implement",
        content: include_str!("../../../../agents/skills/implement/SKILL.md"),
    },
    Shipped {
        name: "acceptance-check",
        content: include_str!("../../../../agents/skills/acceptance-check/SKILL.md"),
    },
    Shipped {
        name: "api-contract",
        content: include_str!("../../../../agents/skills/api-contract/SKILL.md"),
    },
    Shipped {
        name: "architecture",
        content: include_str!("../../../../agents/skills/architecture/SKILL.md"),
    },
    Shipped {
        name: "ci",
        content: include_str!("../../../../agents/skills/ci/SKILL.md"),
    },
    Shipped {
        name: "code-review",
        content: include_str!("../../../../agents/skills/code-review/SKILL.md"),
    },
    Shipped {
        name: "deps",
        content: include_str!("../../../../agents/skills/deps/SKILL.md"),
    },
    Shipped {
        name: "discuss",
        content: include_str!("../../../../agents/skills/discuss/SKILL.md"),
    },
    Shipped {
        name: "docs-update",
        content: include_str!("../../../../agents/skills/docs-update/SKILL.md"),
    },
    Shipped {
        name: "escalate",
        content: include_str!("../../../../agents/skills/escalate/SKILL.md"),
    },
    Shipped {
        name: "moderate",
        content: include_str!("../../../../agents/skills/moderate/SKILL.md"),
    },
    Shipped {
        name: "plan-review",
        content: include_str!("../../../../agents/skills/plan-review/SKILL.md"),
    },
    Shipped {
        name: "qa",
        content: include_str!("../../../../agents/skills/qa/SKILL.md"),
    },
    Shipped {
        name: "report-progress",
        content: include_str!("../../../../agents/skills/report-progress/SKILL.md"),
    },
    Shipped {
        name: "review-guide",
        content: include_str!("../../../../agents/skills/review-guide/SKILL.md"),
    },
    Shipped {
        name: "scan",
        content: include_str!("../../../../agents/skills/scan/SKILL.md"),
    },
    Shipped {
        name: "security-review",
        content: include_str!("../../../../agents/skills/security-review/SKILL.md"),
    },
    Shipped {
        name: "sprint-plan",
        content: include_str!("../../../../agents/skills/sprint-plan/SKILL.md"),
    },
    Shipped {
        name: "supervise",
        content: include_str!("../../../../agents/skills/supervise/SKILL.md"),
    },
    Shipped {
        name: "test-first",
        content: include_str!("../../../../agents/skills/test-first/SKILL.md"),
    },
    Shipped {
        name: "triage",
        content: include_str!("../../../../agents/skills/triage/SKILL.md"),
    },
    Shipped {
        name: "ui-check",
        content: include_str!("../../../../agents/skills/ui-check/SKILL.md"),
    },
];

#[derive(ClapArgs)]
pub struct Args {
    /// Which skill to print: plantool (default), research, plan or implement. `list` names them,
    /// `install` writes them as slash commands for your agent.
    pub name: Option<String>,
    /// With `install`: the skills directory (default: ~/.claude/skills, or ~/.codex/skills with --agent codex).
    #[arg(long)]
    pub dir: Option<PathBuf>,
    /// With `install`: claude or codex; picks the default directory.
    #[arg(long, default_value = "claude")]
    pub agent: String,
    /// With `install`: overwrite a skill that already exists with different content.
    #[arg(long)]
    pub force: bool,
}

pub fn find(name: &str) -> Option<&'static Shipped> {
    let n = name.strip_prefix("plantool-").unwrap_or(name);
    SKILLS.iter().find(|s| s.name == n)
}

fn installed_name(s: &Shipped) -> String {
    if s.name == "plantool" {
        "plantool".to_string()
    } else {
        format!("plantool-{}", s.name)
    }
}

fn printable(s: &Shipped) -> String {
    s.content.replace(
        "$ARGUMENTS",
        "<session ref> (see the prompt you were given, or `plantool list --repo . --json`)",
    )
}

pub fn run(a: Args) -> anyhow::Result<()> {
    match a.name.as_deref().unwrap_or("plantool") {
        "list" => {
            for s in &SKILLS {
                println!("{:<10} plantool skill {}", s.name, s.name);
            }
            Ok(())
        }
        "install" => install(a.dir, &a.agent, a.force),
        name => match find(name) {
            Some(s) => {
                print!("{}", printable(s));
                Ok(())
            }
            None => anyhow::bail!("unknown skill {name}; one of plantool, research, plan, implement, or `list` / `install`"),
        },
    }
}

fn default_dir(agent: &str) -> anyhow::Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("no home directory"))?;
    Ok(match agent {
        "claude" => home.join(".claude").join("skills"),
        "codex" => home.join(".codex").join("skills"),
        other => anyhow::bail!("unknown agent {other}; use claude or codex, or give --dir"),
    })
}

fn install(dir: Option<PathBuf>, agent: &str, force: bool) -> anyhow::Result<()> {
    let dir = match dir {
        Some(d) => crate::client::absolute(&d),
        None => default_dir(agent)?,
    };
    let mut kept = 0;
    for s in &SKILLS {
        let target = dir.join(installed_name(s)).join("SKILL.md");
        if let Ok(existing) = std::fs::read_to_string(&target) {
            if existing == s.content {
                println!("up to date  {}", target.display());
                continue;
            }
            if !force {
                println!(
                    "kept        {}  (differs; pass --force to overwrite)",
                    target.display()
                );
                kept += 1;
                continue;
            }
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&target, s.content)?;
        println!("installed   {}", target.display());
    }
    println!();
    println!("Slash commands: /plantool, /plantool-research <ref>, /plantool-plan <ref>, /plantool-implement <ref>.");
    println!("Agents without slash commands can read the same text with `plantool skill <name>`.");
    if kept > 0 {
        println!("{kept} file(s) were left as they are.");
    }
    Ok(())
}
