pub mod assets;
pub mod changes;
pub mod commits;
pub mod config;
pub mod events;
pub mod git;
pub mod prompts;
pub mod providers;
pub mod registry;
pub mod routes;
pub mod runs;
pub mod security;
pub mod store;
pub mod watcher;

use std::net::SocketAddr;
use std::sync::Arc;

pub use config::Config;
pub use registry::Registry;

pub fn init_tracing() {
    use tracing_subscriber::EnvFilter;
    let filter = EnvFilter::try_from_env("PLANTOOL_LOG").unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt().with_env_filter(filter).with_target(false).try_init();
}

#[derive(Clone)]
pub struct AppState {
    pub registry: Arc<Registry>,
    pub config: Arc<Config>,
    pub runs: Arc<runs::RunManager>,
    pub shutdown: tokio::sync::mpsc::Sender<()>,
}

pub async fn serve(config: Config) -> anyhow::Result<()> {
    let config = Arc::new(config);
    let registry = Arc::new(Registry::load(config.home.clone())?);
    let (shutdown_tx, mut shutdown_rx) = tokio::sync::mpsc::channel::<()>(1);
    let runs = Arc::new(runs::RunManager::default());
    for s in registry.all() {
        runs.mark_orphans(&s);
    }
    let state = AppState { registry: registry.clone(), config: config.clone(), runs, shutdown: shutdown_tx };

    let _watcher = watcher::start(registry.clone(), config.home.clone());

    let app = routes::router(state.clone());
    let addr = SocketAddr::from(([127, 0, 0, 1], config.port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    store::write_daemon_info(&config.home, &store::DaemonInfo {
        pid: std::process::id(),
        port: config.port,
        protocol: plantool_core::PROTOCOL_VERSION,
        version: config.version.clone(),
    })?;
    tracing::info!("plantool daemon listening on http://{addr}");
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            let _ = shutdown_rx.recv().await;
            tracing::info!("shutdown requested");
        })
        .await?;
    let _ = store::remove_daemon_info(&config.home);
    Ok(())
}
