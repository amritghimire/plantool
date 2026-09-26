use crate::client::{home, Client};
use clap::{Args as ClapArgs, Subcommand};
use plantool_daemon::config::{default_port, Config};

#[derive(ClapArgs)]
pub struct Args {
    #[command(subcommand)]
    pub cmd: DaemonCmd,
}

#[derive(Subcommand)]
pub enum DaemonCmd {
    /// Start the daemon in the background if it is not running.
    Start,
    /// Stop the running daemon.
    Stop,
    /// Stop the daemon and start it again from the current binary (picks up an upgrade).
    Restart {
        /// Restart even when hosted runs are live (they are stopped; a new run can resume the provider session).
        #[arg(long)]
        force: bool,
    },
    /// Show whether a daemon is running.
    Status,
}

#[derive(ClapArgs)]
pub struct ServeArgs {
    #[arg(long)]
    pub port: Option<u16>,
}

pub fn run(a: Args) -> anyhow::Result<()> {
    let c = Client::bare(default_port());
    match a.cmd {
        DaemonCmd::Start => {
            c.ensure_daemon()?;
            println!("daemon running on {}", c.base);
        }
        DaemonCmd::Stop => {
            if c.health().is_none() {
                println!("no daemon on {}", c.base);
                return Ok(());
            }
            let _: serde_json::Value = c.post("/shutdown", &serde_json::json!({}))?;
            println!("stopping daemon on {}", c.base);
        }
        DaemonCmd::Restart { force } => {
            if let Some(h) = c.health() {
                let live: Vec<serde_json::Value> = c.get::<Vec<serde_json::Value>>("/api/sessions").unwrap_or_default();
                let running = live.iter().flat_map(|s| s["runs"].as_array().cloned().unwrap_or_default()).filter(|r| matches!(r["status"].as_str(), Some("starting" | "running" | "waiting" | "idle"))).count();
                if running > 0 && !force {
                    anyhow::bail!("{running} hosted run(s) are live and would be stopped; finish them or pass --force");
                }
                let _: serde_json::Value = c.post("/shutdown", &serde_json::json!({}))?;
                println!("stopping daemon {} on {}", h["version"].as_str().unwrap_or(""), c.base);
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
                while c.health().is_some() && std::time::Instant::now() < deadline {
                    std::thread::sleep(std::time::Duration::from_millis(150));
                }
                if c.health().is_some() {
                    anyhow::bail!("the daemon did not stop in time");
                }
            }
            c.ensure_daemon()?;
            let v = c.health().and_then(|h| h["version"].as_str().map(|s| s.to_string())).unwrap_or_default();
            println!("daemon {v} running on {}", c.base);
        }
        DaemonCmd::Status => match c.health() {
            Some(h) => println!("{}", serde_json::to_string_pretty(&h)?),
            None => println!("no daemon on {}", c.base),
        },
    }
    Ok(())
}

pub fn serve(a: ServeArgs) -> anyhow::Result<()> {
    let mut config = Config::from_env(crate::VERSION);
    if let Some(p) = a.port {
        config.port = p;
    }
    config.home = home();
    std::fs::create_dir_all(&config.home)?;
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build()?;
    rt.block_on(async move {
        plantool_daemon::init_tracing();
        plantool_daemon::serve(config).await
    })
}
