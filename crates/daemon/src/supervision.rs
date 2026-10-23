//! Launch barrier and read-only recovery fencing; never signal a recovered PID.
use std::path::Path;
#[cfg(unix)]
pub struct Barrier(std::os::unix::net::UnixStream);
#[cfg(unix)]
pub fn wrap(
    command: tokio::process::Command,
) -> anyhow::Result<(tokio::process::Command, String, Barrier)> {
    use std::os::fd::AsRawFd;
    let (reader, writer) = std::os::unix::net::UnixStream::pair()?;
    let id = uuid::Uuid::new_v4().to_string();
    let mut wrapped = tokio::process::Command::new("/bin/sh");
    wrapped.args(["-c","IFS= read -r ready <&3; [ \"$ready\" = plantool-exec ] || exit 125; exec 3<&-; exec \"$@\"",&id]).arg(command.as_std().get_program()).args(command.as_std().get_args());
    // Fixed shell code passes arguments literally; stdin remains available for
    // app-server and worker credentials. No provider runs before release.
    unsafe {
        wrapped.pre_exec(move || {
            if libc::dup2(reader.as_raw_fd(), 3) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            if libc::fcntl(3, libc::F_SETFD, 0) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    Ok((wrapped, id, Barrier(writer)))
}
#[cfg(unix)]
impl Barrier {
    pub fn release(mut self, home: &Path, id: &str, pid: u32) -> anyhow::Result<()> {
        use std::io::Write;
        let stamp = crate::dispatcher::process_stamp(pid)?;
        crate::database::open(home)?.execute("INSERT INTO supervised_processes(id,pid,process_started) VALUES(?1,?2,?3) ON CONFLICT(id) DO UPDATE SET pid=excluded.pid,process_started=excluded.process_started",rusqlite::params![id,pid,stamp])?;
        self.0.write_all(b"plantool-exec\n")?;
        Ok(())
    }
}
pub fn reconcile(home: &Path) -> anyhow::Result<()> {
    let db = crate::database::open(home)?;
    let rows = {
        let mut q = db.prepare("SELECT id,pid FROM supervised_processes")?;
        let rows = q
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, u32>(1)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };
    if rows.is_empty() {
        crate::database::set(&db, "model_recovery_fence", &false)?;
        return Ok(());
    }
    let mut fenced = false;
    #[cfg(unix)]
    let groups = {
        let result = std::process::Command::new("/bin/ps")
            .args(["-axo", "pgid="])
            .output()?;
        anyhow::ensure!(
            result.status.success(),
            "Cannot inspect process groups; recovery remains unavailable"
        );
        String::from_utf8(result.stdout)?
            .split_whitespace()
            .filter_map(|s| s.parse::<u32>().ok())
            .collect::<std::collections::HashSet<_>>()
    };
    for (id, pid) in rows {
        #[cfg(unix)]
        if groups.contains(&pid) {
            fenced = true;
            continue;
        }
        #[cfg(not(unix))]
        {
            let _ = pid;
            fenced = true;
            continue;
        }
        db.execute("DELETE FROM supervised_processes WHERE id=?1", [id])?;
    }
    crate::database::set(&db, "model_recovery_fence", &fenced)?;
    Ok(())
}
