mod client;
mod commands;

use clap::{Parser, Subcommand};

pub const VERSION: &str = match option_env!("PLANTOOL_BUILD_VERSION") {
    Some(v) => v,
    None => env!("CARGO_PKG_VERSION"),
};

#[derive(Parser)]
#[command(name = "plantool", version = VERSION, about = "Research, plan and implement with your coding agent, reviewed in the browser.")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create a one-time code for registering the first browser passkey.
    Setup,
    /// Read global settings (changes belong in the browser).
    Settings(commands::settings::Args),
    /// Export a session as a portable zip.
    Export(commands::archive::ExportArgs),
    /// Import a session zip into a local repository.
    Import(commands::archive::ImportArgs),
    /// Create (or reopen) a session for a piece of work and open it in the browser.
    New(commands::new::Args),
    /// List sessions (the inbox).
    List(commands::list::Args),
    /// Open a session in the browser.
    Open(commands::open::Args),
    /// Start a hosted research run for a session.
    Research(commands::run::Args),
    /// Start a hosted planning run for a session.
    Plan(commands::run::Args),
    /// Start a hosted implementation run for a session.
    Implement(commands::run::Args),
    /// Have an agent critically review the research or plan and post findings as comments.
    Critique(commands::run::Args),
    /// Open the change review (difftool, or git difftool) for a session.
    Changes(commands::changes::Args),
    /// Progress across sessions.
    Status(commands::status::Args),
    /// Inspect and annotate a session (agent-facing).
    Session(commands::session::Args),
    /// Print the agent skills (plantool, research, plan, implement) or install them as slash commands.
    Skill(commands::skill::Args),
    /// Manage the background daemon.
    Daemon(commands::daemon::Args),
    /// Run the daemon in the foreground.
    Serve(commands::daemon::ServeArgs),
    /// Download, verify and install the latest release.
    Update(commands::update::Args),
}

fn main() {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Setup => setup(),
        Command::Settings(a) => commands::settings::run(a),
        Command::Export(a) => commands::archive::export(a),
        Command::Import(a) => commands::archive::import(a),
        Command::New(a) => commands::new::run(a),
        Command::List(a) => commands::list::run(a),
        Command::Open(a) => commands::open::run(a),
        Command::Research(a) => commands::run::run(a, "research"),
        Command::Plan(a) => commands::run::run(a, "plan"),
        Command::Implement(a) => commands::run::run(a, "implement"),
        Command::Critique(a) => commands::run::run(a, "critique"),
        Command::Changes(a) => commands::changes::run(a),
        Command::Status(a) => commands::status::run(a),
        Command::Session(a) => commands::session::run(a),
        Command::Skill(a) => commands::skill::run(a),
        Command::Daemon(a) => commands::daemon::run(a),
        Command::Serve(a) => commands::daemon::serve(a),
        Command::Update(a) => commands::update::run(a),
    };
    if let Err(e) = result {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}

fn setup() -> anyhow::Result<()> {
    client::ensure_owner_process()?;
    let c = client::Client::bare(plantool_daemon::config::default_port());
    c.ensure_daemon()?;
    let _: serde_json::Value = reqwest::blocking::Client::builder().no_proxy().build()?.get(format!("{}/api/auth/status", c.base)).send()?.error_for_status()?.json().map_err(|_| anyhow::anyhow!("The running daemon does not support passkeys. Run `plantool daemon restart` with this binary first."))?;
    let code = plantool_daemon::auth::setup_code(&client::home())?;
    println!("One-time setup code (expires in 10 minutes): {code}");
    let url = format!("{}/setup", c.base);
    println!("Open {url} and enter the code to register your passkey.");
    client::open_browser(&url);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cli_rejects_human_confirmation_commands() {
        for args in [
            vec![
                "plantool",
                "session",
                "set",
                "--session",
                "s",
                "--base",
                "main",
                "--confirm",
            ],
            vec!["plantool", "import", "s.zip", "--repo", ".", "--confirm"],
            vec!["plantool", "settings", "difftool", "built-in", "--confirm"],
            vec![
                "plantool",
                "session",
                "comment",
                "resolve",
                "--session",
                "s",
                "--id",
                "one",
            ],
            vec![
                "plantool",
                "session",
                "comment",
                "rm",
                "--session",
                "s",
                "--id",
                "one",
            ],
            vec!["plantool", "implement", "s", "--permission", "allow-all"],
            vec!["plantool", "implement", "s", "--step-by-step"],
        ] {
            assert!(Cli::try_parse_from(args).is_err());
        }
        for args in [
            vec![
                "plantool",
                "session",
                "set",
                "--session",
                "s",
                "--base",
                "main",
            ],
            vec![
                "plantool",
                "session",
                "propose",
                "--session",
                "s",
                "--branch",
                "feature",
            ],
            vec!["plantool", "export", "s", "-o", "s.zip"],
            vec!["plantool", "import", "s.zip", "--repo", "."],
            vec!["plantool", "settings", "difftool"],
            vec!["plantool", "daemon", "stop"],
            vec!["plantool", "daemon", "restart"],
        ] {
            assert!(Cli::try_parse_from(args).is_ok());
        }
    }
}
