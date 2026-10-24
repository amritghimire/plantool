use clap::{Args as ClapArgs, Subcommand};
#[derive(ClapArgs)]
pub struct Args {
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    /// Print the JSON Schema for a typed attempt report.
    Schema,
    Artifact {
        #[arg(long)]
        file: std::path::PathBuf,
    },
    Report {
        #[arg(long)]
        file: std::path::PathBuf,
    },
}
pub fn run(args: Args) -> anyhow::Result<()> {
    match args.command {
        Command::Schema => {
            print!("{}", plantool_contract::result_schema());
            Ok(())
        }
        Command::Artifact { file } => {
            anyhow::ensure!(
                std::env::var_os("PLANTOOL_RUN_TOKEN").is_some(),
                "A hosted attempt token is required"
            );
            let item =
                std::env::var("PLANTOOL_ITEM").or_else(|_| std::env::var("PLANTOOL_SESSION"))?;
            let attempt = std::env::var("PLANTOOL_ATTEMPT")?;
            let content = std::fs::read_to_string(file)?;
            let result:serde_json::Value=crate::client::Client::connect()?.post("/internal/v1/store_artifact",&serde_json::json!({"item":item,"attempt_id":attempt,"kind":"evidence","content":content}))?;
            println!("{result}");
            Ok(())
        }
        Command::Report { file } => {
            anyhow::ensure!(
                std::env::var_os("PLANTOOL_RUN_TOKEN").is_some(),
                "A hosted attempt token is required"
            );
            let body: serde_json::Value = serde_json::from_slice(&std::fs::read(file)?)?;
            let result: serde_json::Value =
                crate::client::Client::connect()?.post("/internal/v1/report_result", &body)?;
            println!("{result}");
            Ok(())
        }
    }
}
