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
