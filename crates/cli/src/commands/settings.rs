use crate::client::{print_json, Client};
use clap::{Args as ClapArgs, Subcommand};
#[derive(ClapArgs)]
pub struct Args {
    #[command(subcommand)]
    cmd: Command,
}
#[derive(Subcommand)]
enum Command {
    /// Read the default diff tool. Change it in browser Settings.
    Difftool,
}
pub fn run(args: Args) -> anyhow::Result<()> {
    let client = Client::connect()?;
    match args.cmd {
        Command::Difftool => {
            print_json(&client.get::<serde_json::Value>("/api/settings/difftool")?)
        }
    }
}
