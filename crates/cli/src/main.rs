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
        Command::New(a) => commands::new::run(a),
        Command::List(a) => commands::list::run(a),
        Command::Open(a) => commands::open::run(a),
        Command::Research(a) => commands::run::run(a, "research"),
        Command::Plan(a) => commands::run::run(a, "plan"),
        Command::Implement(a) => commands::run::run(a, "implement"),
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
