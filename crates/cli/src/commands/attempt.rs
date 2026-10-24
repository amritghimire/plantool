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
