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
    /// Optional per-step skill hints in ~/.plantool/prompts/<step>.extra.md.
    PromptExtras {
        /// Download the recommended hints from the plantool repository.
        #[arg(long)]
        install: bool,
        /// Replace extra files you already have.
        #[arg(long, requires = "install")]
        force: bool,
        /// Branch or tag to download from.
        #[arg(long = "ref", default_value = "main", requires = "install")]
        git_ref: String,
    },
}
pub fn run(args: Args) -> anyhow::Result<()> {
    match args.cmd {
        Command::Difftool { value, confirm } => {
            let client = Client::connect()?;
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
        Command::PromptExtras {
            install,
            force,
            git_ref,
        } => {
            let home = crate::client::home();
            print_json(&if install {
                plantool_daemon::prompts::install_extras(&home, &git_ref, force)?
            } else {
                plantool_daemon::prompts::extras_status(&home)
            })
        }
    }
}
