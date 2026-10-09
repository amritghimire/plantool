use crate::client::{print_json, Client};
use clap::{Args as ClapArgs, Subcommand};
use serde_json::{json, Value};
#[derive(ClapArgs)]
pub struct Args {
    #[command(subcommand)]
    cmd: Command,
}
#[derive(Subcommand)]
enum Command {
    Difftool {
        value: Option<String>,
        #[arg(long)]
        confirm: bool,
    },
}
pub fn run(args: Args) -> anyhow::Result<()> {
    let client = Client::connect()?;
    match args.cmd {
        Command::Difftool { value, confirm } => {
            let result: Value = match value {
                Some(value) => {
                    if !confirm {
                        anyhow::bail!("changing the global default requires --confirm");
                    }
                    client.post(
                        "/api/settings/difftool",
                        &json!({"value":value,"confirm":true}),
                    )?
                }
                None => client.get("/api/settings/difftool")?,
            };
            print_json(&result)
        }
    }
}
