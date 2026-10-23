//! Hold one OS advisory lock before touching a home's database or recovery state.
use std::{
    fs::{File, OpenOptions},
    path::Path,
};
pub struct DaemonLock {
    _file: File,
}
impl DaemonLock {
    pub fn acquire(home: &Path) -> anyhow::Result<Self> {
        let parent = home
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        std::fs::create_dir_all(parent)?;
        let mut name = home
            .file_name()
            .ok_or_else(|| anyhow::anyhow!("PlanTool home needs a directory name"))?
            .to_os_string();
        name.push(".daemon.lock");
        // A sibling survives the clean-break whole-home rename. Never unlink a lock
        // file: replacing its inode could allow two processes to lock different files.
        let path = parent.join(name);
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        }
        let file = options.open(path)?;
        fs2::FileExt::try_lock_exclusive(&file)
            .map_err(|_| anyhow::anyhow!("Another daemon already owns this PlanTool home"))?;
        Ok(Self { _file: file })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lock_excludes_a_second_daemon_and_survives_home_backup() {
        let t = tempfile::tempdir().unwrap();
        let home = t.path().join("home");
        std::fs::create_dir_all(&home).unwrap();
        let first = DaemonLock::acquire(&home).unwrap();
        assert!(DaemonLock::acquire(&home).is_err());
        std::fs::rename(&home, t.path().join("backup")).unwrap();
        assert!(DaemonLock::acquire(&home).is_err());
        drop(first);
        assert!(DaemonLock::acquire(&home).is_ok());
    }
}
