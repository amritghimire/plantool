use crate::client::{open_browser, Client};
use clap::Args as ClapArgs;

#[derive(ClapArgs)]
pub struct Args {
    /// Session ref: <slug>, or <repo_slug>/<slug>.
    pub reference: String,
    #[arg(long)]
    pub no_open: bool,
}

pub fn run(a: Args) -> anyhow::Result<()> {
    let c = Client::connect()?;
    let (key, _) = c.resolve_key(&a.reference)?;
    let url = c.session_url(&key);
    println!("{url}");
    if !a.no_open {
        open_browser(&url);
    }
    Ok(())
}
