use anyhow::{bail, Context};
use clap::Args as ClapArgs;
use std::io::Read;
use std::path::{Path, PathBuf};

#[derive(ClapArgs)]
pub struct Args {
    /// Check for a newer release without installing it.
    #[arg(long)]
    pub check: bool,
    /// Install this tag instead of the latest release.
    #[arg(long)]
    pub tag: Option<String>,
}

const REPOSITORY: &str = match option_env!("PLANTOOL_RELEASE_REPOSITORY") {
    Some(r) => r,
    None => "amritghimire/plantool",
};

fn platform() -> anyhow::Result<&'static str> {
    Ok(match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "darwin-arm64",
        ("macos", "x86_64") => "darwin-x64",
        ("linux", "x86_64") => "linux-x64",
        ("linux", "aarch64") => "linux-arm64",
        ("windows", "x86_64") => "windows-x64",
        (os, arch) => bail!("no prebuilt binary for {os}/{arch}; build from source"),
    })
}

#[derive(serde::Deserialize)]
struct Release {
    tag_name: String,
    assets: Vec<Asset>,
}

#[derive(serde::Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
    size: u64,
}

fn http() -> reqwest::blocking::Client {
    reqwest::blocking::Client::builder().user_agent(format!("plantool/{}", crate::VERSION)).timeout(std::time::Duration::from_secs(120)).build().expect("http client")
}

pub fn run(a: Args) -> anyhow::Result<()> {
    let plat = platform()?;
    let client = http();
    let url = match &a.tag {
        Some(t) => format!("https://api.github.com/repos/{REPOSITORY}/releases/tags/{t}"),
        None => format!("https://api.github.com/repos/{REPOSITORY}/releases/latest"),
    };
    let release: Release = match &a.tag {
        Some(_) => client.get(&url).send()?.error_for_status().context("fetching the release from GitHub")?.json()?,
        None => {
            let resp = client.get(&url).send()?;
            if resp.status() == reqwest::StatusCode::NOT_FOUND {
                let mut list: Vec<Release> = client.get(format!("https://api.github.com/repos/{REPOSITORY}/releases?per_page=1")).send()?.error_for_status()?.json()?;
                if list.is_empty() {
                    bail!("{REPOSITORY} has no releases yet");
                }
                list.remove(0)
            } else {
                resp.error_for_status().context("fetching the release from GitHub")?.json()?
            }
        }
    };
    let current = crate::VERSION.trim_start_matches('v');
    let latest = release.tag_name.trim_start_matches('v');
    println!("current: {current}\nlatest:  {latest}");
    if a.check {
        return Ok(());
    }
    if latest == current && a.tag.is_none() {
        println!("already up to date");
        return Ok(());
    }
    if plat.starts_with("windows") {
        bail!("automatic update is not supported on Windows yet; download plantool-{plat}.zip from https://github.com/{REPOSITORY}/releases/tag/{}", release.tag_name);
    }
    let archive_name = format!("plantool-{plat}.tar.gz");
    let asset = release.assets.iter().find(|x| x.name == archive_name).with_context(|| format!("release {} has no {archive_name}", release.tag_name))?;
    let sums = release.assets.iter().find(|x| x.name == "SHA256SUMS.txt").context("release has no SHA256SUMS.txt")?;
    println!("downloading {archive_name} ({} bytes)", asset.size);
    let bytes = client.get(&asset.browser_download_url).send()?.error_for_status()?.bytes()?.to_vec();
    let sums_text = client.get(&sums.browser_download_url).send()?.error_for_status()?.text()?;
    let expected = sums_text
        .lines()
        .find_map(|l| {
            let mut it = l.split_whitespace();
            let sum = it.next()?;
            let name = it.next()?.trim_start_matches('*');
            (name == archive_name).then(|| sum.to_string())
        })
        .context("archive is not listed in SHA256SUMS.txt")?;
    let actual = {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        h.update(&bytes);
        format!("{:x}", h.finalize())
    };
    if actual != expected {
        bail!("checksum mismatch for {archive_name}: expected {expected}, got {actual}");
    }
    let exe = std::env::current_exe()?;
    let exe = std::fs::canonicalize(&exe).unwrap_or(exe);
    let staged = extract_binary(&bytes, &exe)?;
    let backup = exe.with_extension("old");
    let _ = std::fs::remove_file(&backup);
    std::fs::rename(&exe, &backup).with_context(|| format!("cannot replace {}", exe.display()))?;
    if let Err(e) = std::fs::rename(&staged, &exe) {
        let _ = std::fs::rename(&backup, &exe);
        return Err(e).context("installing the new binary");
    }
    let _ = std::fs::remove_file(&backup);
    let out = std::process::Command::new(&exe).arg("--version").output()?;
    println!("installed {}", String::from_utf8_lossy(&out.stdout).trim());
    println!("restart the daemon with `plantool daemon stop` to pick up the new version");
    Ok(())
}

fn extract_binary(archive: &[u8], exe: &Path) -> anyhow::Result<PathBuf> {
    let gz = flate2::read::GzDecoder::new(archive);
    let mut tar = tar::Archive::new(gz);
    let staged = exe.with_extension(format!("new.{}", std::process::id()));
    for entry in tar.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.to_path_buf();
        if path.file_name().and_then(|n| n.to_str()) == Some("plantool") {
            let mut buf = Vec::new();
            entry.read_to_end(&mut buf)?;
            std::fs::write(&staged, &buf)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755))?;
            }
            return Ok(staged);
        }
    }
    bail!("the archive does not contain a plantool binary")
}
