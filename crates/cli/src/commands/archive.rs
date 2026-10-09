use crate::client::{absolute, print_json, urlencode, Client};
use clap::Args;
use std::path::PathBuf;

#[derive(Args)]
pub struct ExportArgs {
    pub session: String,
    #[arg(short, long)]
    pub output: PathBuf,
    #[arg(long)]
    pub transcripts: bool,
    #[arg(long)]
    pub revisions: bool,
    #[arg(long)]
    pub note: Option<String>,
}
#[derive(Args)]
pub struct ImportArgs {
    pub file: PathBuf,
    #[arg(long)]
    pub repo: PathBuf,
    #[arg(long)]
    pub workspace: Option<PathBuf>,
    /// Restore archived approvals; without this the imported plan needs owner review.
    #[arg(long)]
    pub confirm: bool,
}
pub fn export(args: ExportArgs) -> anyhow::Result<()> {
    let client = Client::connect()?;
    let (key, _) = client.resolve_key(&args.session)?;
    let mut url = format!(
        "/api/sessions/{key}/export?transcripts={}&revisions={}",
        args.transcripts, args.revisions
    );
    if let Some(note) = args.note {
        url.push_str(&format!("&note={}", urlencode(&note)));
    }
    let bytes = client.get_bytes(&url)?;
    std::fs::write(absolute(&args.output), bytes)?;
    println!("Exported {}", args.output.display());
    Ok(())
}
pub fn import(args: ImportArgs) -> anyhow::Result<()> {
    let client = Client::connect()?;
    let mut url = format!(
        "/api/import?repo={}&confirm={}",
        urlencode(&absolute(&args.repo).to_string_lossy()),
        args.confirm
    );
    if let Some(workspace) = args.workspace {
        url.push_str(&format!(
            "&workspace={}",
            urlencode(&absolute(&workspace).to_string_lossy())
        ));
    }
    let result = client.post_bytes(&url, std::fs::read(absolute(&args.file))?)?;
    print_json(&result)
}
