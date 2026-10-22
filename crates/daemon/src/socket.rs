//! Private Unix transport for non-human clients. Browser authority is TCP-only.
use std::path::{Path, PathBuf};
#[derive(Clone)]
pub struct SocketTransport;
pub fn path(home: &Path) -> PathBuf {
    home.join("run/daemon.sock")
}

#[cfg(unix)]
mod unix {
    use super::*;
    use std::os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, PermissionsExt};
    pub struct SocketFile {
        path: PathBuf,
        dev: u64,
        ino: u64,
    }
    impl Drop for SocketFile {
        fn drop(&mut self) {
            if std::fs::symlink_metadata(&self.path)
                .is_ok_and(|m| m.dev() == self.dev && m.ino() == self.ino)
            {
                let _ = std::fs::remove_file(&self.path);
            }
        }
    }
    pub async fn bind(home: &Path) -> anyhow::Result<(tokio::net::UnixListener, SocketFile)> {
        let dir = home.join("run");
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&dir)?;
        let metadata = std::fs::symlink_metadata(&dir)?;
        anyhow::ensure!(
            metadata.is_dir() && !metadata.file_type().is_symlink(),
            "daemon run directory must be a real directory"
        );
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
        let path = path(home);
        if let Ok(old) = std::fs::symlink_metadata(&path) {
            anyhow::ensure!(
                old.file_type().is_socket(),
                "refusing to replace non-socket {}",
                path.display()
            );
            match tokio::net::UnixStream::connect(&path).await {
                Ok(_) => anyhow::bail!("a daemon is already listening on {}", path.display()),
                Err(e) if e.kind() == std::io::ErrorKind::ConnectionRefused => {}
                Err(e) => return Err(e.into()),
            }
            let current = std::fs::symlink_metadata(&path)?;
            anyhow::ensure!(
                old.dev() == current.dev() && old.ino() == current.ino(),
                "daemon socket changed during startup"
            );
            std::fs::remove_file(&path)?;
        }
        let listener = tokio::net::UnixListener::bind(&path)?;
        let metadata = std::fs::symlink_metadata(&path)?;
        let owned = SocketFile {
            path: path.clone(),
            dev: metadata.dev(),
            ino: metadata.ino(),
        };
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        Ok((listener, owned))
    }
}
#[cfg(unix)]
pub use unix::bind;

#[cfg(unix)]
pub fn router(state: crate::AppState) -> axum::Router {
    crate::routes::router(state).layer(axum::middleware::from_fn(
        |mut req: axum::extract::Request, next: axum::middleware::Next| async move {
            req.extensions_mut().insert(SocketTransport);
            req.headers_mut()
                .insert("x-plantool-cli", "1".parse().unwrap());
            next.run(req).await
        },
    ))
}
