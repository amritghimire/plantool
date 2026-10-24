use crate::client::{print_json, Client};
use clap::{Args as ClapArgs, Subcommand};
#[derive(ClapArgs)]
pub struct Args {
    #[command(subcommand)]
    pub command: TaskCommand,
}
#[derive(Subcommand)]
pub enum TaskCommand {
    /// Report completion against an approved task without editing the plan.
    Done {
        item: String,
        key: String,
        #[arg(long)]
        json: bool,
    },
}
pub fn run(args: Args) -> anyhow::Result<()> {
    match args.command {
        TaskCommand::Done { item, key, json } => {
            anyhow::ensure!(
                plantool_core::tasks::valid_key(&key),
                "Use a task key such as T1"
            );
            let c = Client::connect()?;
            let (item, _) = c.resolve_key(&item)?;
            let response: serde_json::Value = c.post(
                &format!("/api/sessions/{item}/tasks/{key}/done"),
                &serde_json::json!({}),
            )?;
            if json {
                print_json(&response)?
            } else {
                println!("completed {item} {key}")
            }
            Ok(())
        }
    }
}
